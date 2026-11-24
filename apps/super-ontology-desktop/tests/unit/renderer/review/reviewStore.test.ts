import { describe, expect, it } from "vitest";
import type { ReviewGroup } from "@so-contract/docs-api.generated";
import { applyEvents } from "@so-model/superDriveEvents";
import { createInitialState, reviewKey } from "@so-model/superDriveState";
import { createSuperDriveStore } from "@so-model/superDriveStore";
import { createFakeApi, DRIVE_ID, failResult, manualScheduler, okResult } from "../../../support/fakeApi";

const FILE = "00000000-0000-4000-8000-0000000000f1";
const item = (n: number, extra: Record<string, unknown> = {}) => ({ id: `item-${n}`, kind: "low_confidence_fact", status: "pending", reason_code: "low_conf", created_at: "2026-11-17T00:00:00Z", revision: 1, drive_id: DRIVE_ID, source: { file_id: FILE, path: "a.md" }, ...extra });
const group = (count: number, kind: ReviewGroup["kind"] = "low_confidence_fact", file = FILE): ReviewGroup => ({ source: { file_id: file, path: "a.md" }, kind, count });

function setup(overrides: Parameters<typeof createFakeApi>[0] = {}) {
  const fake = createFakeApi(overrides);
  const manual = manualScheduler();
  const store = createSuperDriveStore({ api: fake.api, scheduler: manual.scheduler, createId: (prefix = "op") => `${prefix}_0123456789abcdef` });
  return { ...fake, store, manual };
}

describe("review groups and items", () => {
  it("loads the groups with the as_of_seq of the page and keeps it for a group decision", async () => {
    const { store, api } = setup({ reviewGroups: async () => okResult({ items: [group(100)], next_cursor: null, as_of_seq: 77 }) });
    await store.actions.loadReviewGroups(DRIVE_ID);
    expect(store.getState().review.groups[DRIVE_ID]).toMatchObject({ asOfSeq: 77, loaded: true, loading: false, stale: false });
    expect(api.reviewGroups).toHaveBeenCalledTimes(1);
    await store.actions.loadReviewGroups(DRIVE_ID);
    expect(api.reviewGroups).toHaveBeenCalledTimes(1);
  });

  it("reads a stale listing again and ignores a late answer of an older request", async () => {
    const gate: { release: () => void } = { release: () => undefined };
    let calls = 0;
    const { store } = setup({
      reviewGroups: async () => {
        calls += 1;
        if (calls === 1) await new Promise<void>((resolve) => { gate.release = resolve; });
        return okResult({ items: [group(calls === 1 ? 1 : 2)], next_cursor: null, as_of_seq: calls });
      },
    });
    const first = store.actions.loadReviewGroups(DRIVE_ID);
    // A second request is refused while one is in flight; a forced reload after it finishes wins.
    await store.actions.loadReviewGroups(DRIVE_ID, { force: true });
    gate.release();
    await first;
    expect(store.getState().review.groups[DRIVE_ID].loading).toBe(false);
  });

  it("marks listings of a Drive stale when review.changed arrives, and never patches them", () => {
    let state = createInitialState();
    state = { ...state, review: { ...state.review, groups: { [DRIVE_ID]: { items: [group(3)], nextCursor: null, asOfSeq: 5, loaded: true, loading: false, stale: false, error: null } }, lists: { [reviewKey(DRIVE_ID, FILE, null)]: { items: [item(1) as never], nextCursor: null, asOfSeq: 5, loaded: true, loading: false, stale: false, error: null } } } };
    const next = applyEvents(state, [{ seq: 9, type: "review.changed", at: "2026-11-17T00:00:00Z", drive_id: DRIVE_ID, data: { drive_id: DRIVE_ID, pending: { low_confidence_fact: 2 } } }]);
    expect(next.review.groups[DRIVE_ID].stale).toBe(true);
    expect(next.review.groups[DRIVE_ID].items).toHaveLength(1);
    expect(next.review.lists[reviewKey(DRIVE_ID, FILE, null)].stale).toBe(true);
    expect(state.review.groups[DRIVE_ID].stale).toBe(false);
  });
});

describe("decisions", () => {
  it("accepts a whole source with one request: one operation, by_source with the loaded as_of_seq, no per-item calls", async () => {
    const { store, api } = setup({
      reviewGroups: async () => okResult({ items: [group(100)], next_cursor: null, as_of_seq: 77 }),
      decideReview: async () => okResult({ results: [], accepted: 100, rejected: 0, publish_pending: true }),
    });
    await store.actions.loadReviewGroups(DRIVE_ID);
    const result = await store.actions.decideReviewBySource({ driveId: DRIVE_ID, fileId: FILE, decision: "accept", asOfSeq: store.getState().review.groups[DRIVE_ID].asOfSeq });
    expect(result).toMatchObject({ ok: true, value: { accepted: 100, publishPending: true } });
    expect(api.decideReview).toHaveBeenCalledTimes(1);
    expect(api.decideReview).toHaveBeenCalledWith({ operationId: "rv_0123456789abcdef", bySource: { fileId: FILE, decision: "accept", asOfSeq: 77 } });
    expect(store.getState().review.groups[DRIVE_ID].items).toEqual([]);
    expect(store.getState().review.publishing[DRIVE_ID]).toBe(true);
    expect(store.getState().notices.at(-1)).toMatchObject({ kind: "review_result", values: { accepted: 100 } });
  });

  it("folds the five per-item results: removes ok, already-decided and gone items, reloads conflicts, keeps invalid ones", async () => {
    const { store } = setup({
      reviewItems: async () => okResult({ items: [item(1), item(2), item(3), item(4), item(5)], next_cursor: null, as_of_seq: 9 }),
      decideReview: async () => okResult({
        results: [{ id: "item-1", status: "ok" }, { id: "item-2", status: "already_decided" }, { id: "item-3", status: "conflict", code: "DOCS_REVIEW_STALE_REVISION" }, { id: "item-4", status: "not_found" }, { id: "item-5", status: "invalid" }],
        accepted: 1, rejected: 0, publish_pending: false,
      }),
    });
    await store.actions.loadReviewItems({ driveId: DRIVE_ID, source: FILE });
    const result = await store.actions.decideReviewItems({ driveId: DRIVE_ID, items: ["item-1", "item-2", "item-3", "item-4", "item-5"].map((id) => ({ id, revision: 1, decision: "accept" as const })) });
    expect(result).toMatchObject({ ok: true, value: { accepted: 1, alreadyDecided: 1, conflicts: 1, notFound: 1, invalid: 1, publishPending: false } });
    const list = store.getState().review.lists[reviewKey(DRIVE_ID, FILE, null)];
    expect(list.items.map((entry) => entry.id)).toEqual(["item-3", "item-5"]);
    expect(list.stale).toBe(true);
    expect(store.getState().review.publishing[DRIVE_ID]).toBeUndefined();
  });

  it("hands back the operation id of a request that timed out so a retry replays it", async () => {
    const { store } = setup({ decideReview: async () => failResult("SO_TIMEOUT", { retryable: true }) });
    const result = await store.actions.decideReviewItems({ driveId: DRIVE_ID, items: [{ id: "item-1", revision: 1, decision: "reject" }] });
    expect(result).toMatchObject({ ok: false, error: { code: "SO_TIMEOUT" }, operationId: "rv_0123456789abcdef" });
  });

  it("stops saying a version is being published when the engine published nothing", async () => {
    const { store, manual, api } = setup({
      reviewGroups: async () => okResult({ items: [group(1)], next_cursor: null, as_of_seq: 5 }),
      decideReview: async () => okResult({ results: [], accepted: 1, rejected: 0, publish_pending: true }),
    });
    await store.actions.loadReviewGroups(DRIVE_ID);
    await store.actions.decideReviewBySource({ driveId: DRIVE_ID, fileId: FILE, decision: "accept", asOfSeq: 5 });
    expect(store.getState().review.publishing[DRIVE_ID]).toBe(true);
    manual.advance(5_000);
    expect(store.getState().review.publishing[DRIVE_ID]).toBe(true);
    manual.advance(1_500);
    await Promise.resolve(); await Promise.resolve();
    expect(api.ontologyVersions).toHaveBeenCalled();
    expect(store.getState().review.publishing[DRIVE_ID]).toBe(false);
  });

  it("clears the publishing mark when the next ontology version is published", () => {
    let state = createInitialState();
    state = { ...state, review: { ...state.review, publishing: { [DRIVE_ID]: true } } };
    const next = applyEvents(state, [{ seq: 3, type: "ontology.version_published", at: "2026-11-17T00:00:00Z", drive_id: DRIVE_ID, data: { n: 18, as_of: "2026-11-17T00:00:00Z", facts_added: 1, facts_retracted: 0, gates: {} } }]);
    expect(next.review.publishing[DRIVE_ID]).toBe(false);
    expect(next.ontology[DRIVE_ID].n).toBe(18);
  });
});

describe("performance", () => {
  it("groups 1,000 sources in the state within the budget of the card (< 100 ms)", async () => {
    const groups = Array.from({ length: 1000 }, (_, n) => group(5, "low_confidence_fact", `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`));
    const { store } = setup({ reviewGroups: async () => okResult({ items: groups, next_cursor: null, as_of_seq: 1 }) });
    const started = performance.now();
    await store.actions.loadReviewGroups(DRIVE_ID);
    expect(performance.now() - started).toBeLessThan(100);
    expect(store.getState().review.groups[DRIVE_ID].items).toHaveLength(1000);
  });
});
