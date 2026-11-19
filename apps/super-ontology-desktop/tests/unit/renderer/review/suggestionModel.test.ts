import { describe, expect, it, vi } from "vitest";
import { canUndo, digestView, groupBySource, impactedShares, suggestionLine } from "@so-model/review/suggestionModel";
import { createSuggestionStore } from "@so-model/review/suggestionStore";
import { roleAvailability, suggestSummary } from "@so-model/share/shareRoleModel";
import { SHARE_A, SHARE_B, makeDigest, makeSuggestion } from "../../../support/suggestionFixtures";

const ok = <T,>(value: T) => ({ ok: true as const, value });

describe("suggestion model (F-S7-01)", () => {
  it("words a suggestion as one line and knows what can be undone", () => {
    expect(suggestionLine(makeSuggestion(1))).toBe("Plan 1 decision Option 1");
    expect(suggestionLine(makeSuggestion(2, { payload: { ...makeSuggestion(2).payload, object: undefined, value: "done", valid_from: "2026-11-01" } }))).toBe("Plan 2 decision done (2026-11-01)");
    for (const status of ["auto_accepted", "superseded_silently", "accepted"]) expect(canUndo({ status } as never), status).toBe(true);
    for (const status of ["pending", "rejected", "undone", "duplicate"]) expect(canUndo({ status } as never), status).toBe(false);
  });

  it("groups by source and counts what a person has to do: one operation per group", () => {
    const pending = [...Array.from({ length: 30 }, (_, index) => makeSuggestion(index + 1)), ...Array.from({ length: 20 }, (_, index) => makeSuggestion(index + 100, { share_id: SHARE_B }))];
    const view = digestView(makeDigest(pending, [makeSuggestion(500, { status: "auto_accepted" })]));
    expect(view.pending.map((group) => group.shareId).sort()).toEqual([SHARE_A, SHARE_B].sort());
    expect(view.pendingCount).toBe(50);
    expect(view.humanOperations).toBe(2);
    expect(view.inEffect).toHaveLength(1);
    expect(groupBySource([]).length).toBe(0);
  });

  it("lists the tasks a suggestion touches", () => {
    expect(impactedShares(makeSuggestion(1))).toEqual([{ shareId: SHARE_B, task: "Follow plan two", groupId: null, sections: ["Decisions"], version: 2 }]);
  });
});

describe("a typical day of 50 suggestions needs at most 5 operations (30 L5 acceptance 3)", () => {
  it("40 accepted by rule need none, 10 pending from two sources need two group decisions", async () => {
    const effect = Array.from({ length: 40 }, (_, index) => makeSuggestion(index + 1, { status: "auto_accepted" }));
    const pending = [...Array.from({ length: 6 }, (_, index) => makeSuggestion(index + 100)), ...Array.from({ length: 4 }, (_, index) => makeSuggestion(index + 200, { share_id: SHARE_B }))];
    let digest = makeDigest(pending, effect);
    const api = {
      suggestionDigest: vi.fn(async () => ok(digest)), suggestionDiff: vi.fn(),
      decideSuggestion: vi.fn(),
      decideSuggestionsBySource: vi.fn(async ({ shareId }: { shareId: string }) => {
        digest = { ...digest, pending: digest.pending.filter((item) => item.share_id !== shareId), counts: {}, total: 0 };
        return ok({ results: [], decided: 1, failed: 0 });
      }),
    };
    const store = createSuggestionStore({ api: api as never });
    await store.load();
    let operations = 0;
    for (const group of store.getState().view!.pending) { await store.decideSource(group.shareId, "accept"); operations += 1; }
    expect(operations).toBeLessThanOrEqual(5);
    expect(api.decideSuggestionsBySource).toHaveBeenCalledTimes(2);
    expect(store.getState().view!.pendingCount).toBe(0);
  });

  it("shows a decision at once and puts it back when the engine refuses", async () => {
    const pending = [makeSuggestion(1), makeSuggestion(2)];
    const gate: { release?: () => void } = {};
    const api = {
      suggestionDigest: vi.fn(async () => ok(makeDigest(pending))), suggestionDiff: vi.fn(),
      decideSuggestion: vi.fn(() => new Promise((resolve) => { gate.release = () => resolve({ ok: false, error: { code: "DOCS_SUGGEST_NOT_PENDING", retryable: false, details: {} } }); })),
      decideSuggestionsBySource: vi.fn(),
    };
    const store = createSuggestionStore({ api: api as never });
    await store.load();
    const decision = store.decide(pending[0]!, "accept");
    expect(store.getState().view!.pendingCount).toBe(1);
    expect(store.getState().busy.has(pending[0]!.id)).toBe(true);
    gate.release?.();
    expect((await decision).ok).toBe(false);
    expect(store.getState().view!.pendingCount).toBe(2);
    expect(store.getState().error?.code).toBe("DOCS_SUGGEST_NOT_PENDING");
    expect(store.getState().busy.size).toBe(0);
  });
});

describe("share roles (F-S7-02)", () => {
  it("editor needs a folder Drive; the other roles do not care", () => {
    expect(roleAvailability("editor", "fs").available).toBe(true);
    expect(roleAvailability("editor", "s3")).toEqual({ available: false, reasonMessageId: "super-drive.share.role.editor.needsFolderDrive" });
    expect(roleAvailability("suggester", "s3").available).toBe(true);
    expect(roleAvailability("editor", undefined).available).toBe(true);
  });
  it("summarises suggestions of a share: may write, collected, waiting", () => {
    const list = [makeSuggestion(1), makeSuggestion(2, { submitted_by: "credential", status: "accepted" }), makeSuggestion(3, { share_id: SHARE_B })];
    expect(suggestSummary({ id: SHARE_A, role: "suggester" }, list)).toEqual({ canSuggest: true, collected: 1, pending: 1, total: 2 });
    expect(suggestSummary({ id: SHARE_A, role: "viewer" }, list).canSuggest).toBe(false);
  });
});
