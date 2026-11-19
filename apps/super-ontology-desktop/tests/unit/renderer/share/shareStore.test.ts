import { describe, expect, it } from "vitest";
import { applyEvents } from "@so-model/superDriveEvents";
import { buildShareChains } from "@so-model/superDriveOntologyStore";
import { createInitialState } from "@so-model/superDriveState";
import { createSuperDriveStore } from "@so-model/superDriveStore";
import { createFakeApi, DRIVE_ID, failResult, makeShare, manualScheduler, okResult } from "../../../support/fakeApi";

function setup(overrides: Parameters<typeof createFakeApi>[0] = {}) {
  const fake = createFakeApi(overrides);
  const store = createSuperDriveStore({ api: fake.api, scheduler: manualScheduler().scheduler, createId: (prefix = "op") => `${prefix}_0123456789abcdef` });
  return { ...fake, store };
}
const at = (day: number) => `2026-11-${String(day).padStart(2, "0")}T00:00:00Z`;

describe("share chains", () => {
  it("links every refreshed share to its root, oldest first", () => {
    const chains = buildShareChains([
      makeShare("c", { refreshed_from: "b", created_at: at(3) }), makeShare("b", { refreshed_from: "a", created_at: at(2), state: "refreshed" }), makeShare("a", { created_at: at(1), state: "refreshed" }), makeShare("x"),
    ]);
    expect(chains).toEqual({ a: ["a", "b", "c"] });
  });

  it("keeps a chain whose root is not loaded (older than the page)", () => {
    expect(buildShareChains([makeShare("b", { refreshed_from: "gone", created_at: at(2) })])).toEqual({ gone: ["b"] });
  });
});

describe("share list", () => {
  it("walks every page, sorts newest first and rebuilds the chains", async () => {
    const pages = [
      { items: [makeShare("a", { created_at: at(1) })], next_cursor: "n1", as_of_seq: 1 },
      { items: [makeShare("b", { created_at: at(2), refreshed_from: "a" })], next_cursor: null, as_of_seq: 1 },
    ];
    let call = 0;
    const { store, api } = setup({ listShares: async () => okResult(pages[call++]) });
    await store.actions.loadShares({ driveId: DRIVE_ID });
    expect(api.listShares).toHaveBeenCalledTimes(2);
    expect(store.getState().shares.order).toEqual(["b", "a"]);
    expect(store.getState().shares.chains).toEqual({ a: ["a", "b"] });
    expect(store.getState().shares.loaded).toBe(true);
  });

  it("explains a list that failed and can be read again", async () => {
    const { store } = setup({ listShares: async () => failResult("DOCS_ENGINE_UNAVAILABLE", { retryable: true }) });
    await store.actions.loadShares();
    expect(store.getState().shares.error?.code).toBe("DOCS_ENGINE_UNAVAILABLE");
    expect(store.getState().shares.loading).toBe(false);
  });
});

describe("share operations", () => {
  it("refresh turns the old share into `refreshed`, links the new one and clears the pending mark", async () => {
    const { store } = setup({
      listShares: async () => okResult({ items: [makeShare("s1", { state: "drifted", created_at: at(1) })], next_cursor: null, as_of_seq: 1 }),
      refreshShare: async () => okResult(makeShare("s2", { refreshed_from: "s1", created_at: at(2) })),
    });
    await store.actions.loadShares();
    const result = await store.actions.refreshShare("s1");
    expect(result.ok).toBe(true);
    const { shares } = store.getState();
    expect(shares.byId.s1.state).toBe("refreshed");
    expect(shares.byId.s2.refreshed_from).toBe("s1");
    expect(shares.order[0]).toBe("s2");
    expect(shares.chains).toEqual({ s1: ["s1", "s2"] });
    expect(shares.pending.s1).toBeUndefined();
  });

  it("revoke returns the result with local_removed and the notice code", async () => {
    const { store } = setup({
      listShares: async () => okResult({ items: [makeShare("s1")], next_cursor: null, as_of_seq: 1 }),
      revokeShare: async () => okResult({ share: makeShare("s1", { state: "revoked" }), local_removed: false, notice_code: "DOCS_SHARE_READ_CONTENT_IRREVOCABLE" }),
    });
    await store.actions.loadShares();
    const result = await store.actions.revokeShare("s1", { reason: "done" });
    expect(result).toMatchObject({ ok: true, value: { local_removed: false, notice_code: "DOCS_SHARE_READ_CONTENT_IRREVOCABLE" } });
    expect(store.getState().shares.byId.s1.state).toBe("revoked");
  });

  it("mount keeps the operation id on failure so a repeat is a replay", async () => {
    const { store, api } = setup({ mountShare: async () => failResult("SO_TIMEOUT", { retryable: true }) });
    const first = await store.actions.mountShare({ compile: { driveId: DRIVE_ID }, targetGrantId: "grant-0001", expiresInSeconds: 3600 });
    expect(first).toMatchObject({ ok: false, operationId: "sh_0123456789abcdef" });
    await store.actions.mountShare({ compile: { driveId: DRIVE_ID }, targetGrantId: "grant-0001", expiresInSeconds: 3600, operationId: "sh_0123456789abcdef" });
    expect(api.mountShare.mock.calls.map((call) => call[0].operationId)).toEqual(["sh_0123456789abcdef", "sh_0123456789abcdef"]);
  });

  it("a group refresh names the members that failed", async () => {
    const { store } = setup({ refreshShareGroup: async () => okResult({ results: [{ id: "s1", status: "ok" }, { id: "s2", status: "conflict" }] }) });
    expect(await store.actions.refreshShareGroup("00000000-0000-4000-8000-0000000000a1")).toEqual({ ok: true, value: { failed: ["s2"] } });
  });
});

describe("share events", () => {
  const base = { at: at(1), drive_id: DRIVE_ID };
  it("keeps the update level of the newest entry and marks the log stale", () => {
    const next = applyEvents(createInitialState(), [{ ...base, seq: 1, type: "share.update_appended", data: { share_id: "s1", seq: 4, level: "block" } }]);
    expect(next.shares.updates.s1).toMatchObject({ latestLevel: "block", stale: true });
  });
  it("adds Agent pulls up", () => {
    const next = applyEvents(createInitialState(), [{ ...base, seq: 1, type: "refs.accessed", data: { share_id: "s1", count: 2 } }, { ...base, seq: 2, type: "refs.accessed", data: { share_id: "s1", count: 3 } }]);
    expect(next.shares.accessed.s1).toBe(5);
  });
  it("marks a drifted share and keeps the chain of a refreshed one", () => {
    const first = applyEvents(createInitialState(), [{ ...base, seq: 1, type: "share.created", data: { share: makeShare("s1") } }]);
    const drifted = applyEvents(first, [{ ...base, seq: 2, type: "share.state_changed", data: { share: makeShare("s1", { state: "drifted", drift: { facts_changed: 3, files_changed: 1 } }) } }]);
    expect(drifted.shares.byId.s1.state).toBe("drifted");
    const refreshed = applyEvents(drifted, [{ ...base, seq: 3, type: "share.created", data: { share: makeShare("s2", { refreshed_from: "s1" }) } }]);
    expect(refreshed.shares.chains).toEqual({ s1: ["s1", "s2"] });
  });
  it("ignores a share event that carries no Share", () => {
    const loaded = applyEvents(createInitialState(), [{ ...base, seq: 1, type: "share.created", data: { share: makeShare("s1") } }]);
    const next = applyEvents(loaded, [{ ...base, seq: 2, type: "share.state_changed", data: { share_id: "s1", to: "drifted" } }]);
    expect(next.shares.byId.s1.state).toBe("active");
  });
});
