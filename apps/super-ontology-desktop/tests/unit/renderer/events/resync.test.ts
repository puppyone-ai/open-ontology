import { describe, expect, it } from "vitest";
import { createSuperDriveStore } from "@so-model/superDriveStore";
import { folderKey } from "@so-model/superDriveRoute";
import { createFakeApi, DRIVE_ID, makeDrive, makeFile, makeSnapshot, manualScheduler, okResult } from "../../../support/fakeApi";

const page = (paths: string[], asOf: number, next: string | null = null) => ({ items: paths.map((path) => makeFile(path)), next_cursor: next, as_of_seq: asOf });

async function readyStore(options: Parameters<typeof createFakeApi>[0] = {}) {
  const fake = createFakeApi(options);
  const clock = manualScheduler();
  const store = createSuperDriveStore({ api: fake.api, scheduler: clock.scheduler, createId: () => "op_fixed_0123456789" });
  await store.start();
  return { ...fake, store, clock };
}

describe("resync", () => {
  it("reloads drives, the current folder, then continues events from the newest as_of_seq", async () => {
    const order: string[] = [];
    const { api, store } = await readyStore({
      listDrives: async () => { order.push("drives"); return okResult([makeDrive()]); },
      driveTree: async () => { order.push("tree"); return okResult({ dirs: [], files: page([], 0) }); },
      listFiles: async () => { order.push("files"); return okResult(page(["a.md", "b.md"], 77)); },
      subscribeEvents: async (input: { sinceSeq?: number } = {}) => {
        order.push(`subscribe:${input.sinceSeq ?? "-"}`);
        return okResult({ instanceId: "1:a", lastSeq: input.sinceSeq ?? 0, connection: { state: "connected", attempt: 0, nextRetryAt: null, lastSeq: 0 } });
      },
    });
    await store.actions.loadFolder({ driveId: DRIVE_ID, path: "sub" });
    order.length = 0;
    store.handlePush({ channel: "resync-required", payload: { instanceId: "1:a", reason: "cursor_expired", headSeq: 900 } });
    expect(store.getState().connection.resyncing).toBe(true);
    await store.resync("cursor_expired");
    expect(order).toEqual(["drives", "tree", "files", "subscribe:77"]);
    expect(store.getState().connection.resyncing).toBe(false);
    expect(store.getState().cursor.appliedSeq).toBe(77);
    expect(api.subscribeEvents).toHaveBeenLastCalledWith({ sinceSeq: 77 });
  });

  it("keeps the other loaded folders but marks them stale for a later refetch", async () => {
    const { store } = await readyStore({ listFiles: async () => okResult(page(["a.md"], 40)) });
    await store.actions.loadFolder({ driveId: DRIVE_ID, path: "" });
    await store.actions.loadFolder({ driveId: DRIVE_ID, path: "other" });
    expect(store.getState().folders[folderKey(DRIVE_ID, "other")].stale).toBe(false);
    await store.resync("gap");
    expect(store.getState().folders[folderKey(DRIVE_ID, "other")].stale).toBe(true);
    expect(store.getState().folders[folderKey(DRIVE_ID, "")].stale).toBe(false);
    expect(store.getState().files["f-a.md"]).toBeDefined();
  });

  it("falls back to the engine events_head when no folder was loaded and keeps optimistic marks", async () => {
    const { api, store } = await readyStore({ readEngine: async () => okResult(makeSnapshot({ health: { events_head: 123 } })) });
    store.handlePush({ channel: "engine-changed", payload: makeSnapshot({ revision: 5, health: { events_head: 123 } }) });
    store.handlePush({ channel: "events", payload: { instanceId: "1:a", lastSeq: 3, events: [{ seq: 3, type: "file.state_changed", at: "x", drive_id: DRIVE_ID, data: { file: makeFile("a.md") } }] } });
    const state = store.getState();
    expect(state.files["f-a.md"]).toBeDefined();
    await store.resync("engine_restarted");
    expect(api.subscribeEvents).toHaveBeenLastCalledWith({ sinceSeq: 123 });
    expect(store.getState().files["f-a.md"]).toBeDefined();
  });

  it("collapses concurrent resyncs into one pass", async () => {
    const { api, store } = await readyStore();
    const before = api.listDrives.mock.calls.length;
    await Promise.all([store.resync("gap"), store.resync("gap"), store.resync("cursor_expired")]);
    expect(api.listDrives.mock.calls.length - before).toBe(1);
  });

  it("does nothing against an engine that is not live and still clears the resyncing flag", async () => {
    const { api, store } = await readyStore({ readEngine: async () => okResult(makeSnapshot({ status: "restarting", reason: "drive_change" })) });
    const before = api.listDrives.mock.calls.length;
    await store.resync("gap");
    expect(api.listDrives.mock.calls.length).toBe(before);
    expect(store.getState().connection.resyncing).toBe(false);
  });

  it("announces how many changes were caught up after a reconnect, then lets the notice expire", async () => {
    const { store, clock } = await readyStore();
    store.handlePush({ channel: "connection-changed", payload: { state: "reconnecting", attempt: 1, nextRetryAt: null, lastSeq: 0 } });
    expect(store.getState().connection.state).toBe("reconnecting");
    store.handlePush({ channel: "connection-changed", payload: { state: "connected", attempt: 0, nextRetryAt: null, lastSeq: 0 } });
    const events = [1, 2, 3].map((seq) => ({ seq, type: "drive.updated", at: "x", drive_id: DRIVE_ID, data: { drive: makeDrive({ revision: seq }) } }));
    store.handlePush({ channel: "events", payload: { instanceId: "1:a", lastSeq: 3, events } });
    expect(store.getState().notices).toEqual([]);
    clock.advance(450);
    expect(store.getState().notices).toMatchObject([{ kind: "caught_up", values: { count: 3 } }]);
    clock.advance(3_100);
    expect(store.getState().notices).toEqual([]);
  });

  it("shows nothing when a reconnect had nothing to catch up", async () => {
    const { store, clock } = await readyStore();
    store.handlePush({ channel: "connection-changed", payload: { state: "reconnecting", attempt: 1, nextRetryAt: null, lastSeq: 0 } });
    store.handlePush({ channel: "connection-changed", payload: { state: "connected", attempt: 0, nextRetryAt: null, lastSeq: 0 } });
    clock.advance(1_000);
    expect(store.getState().notices).toEqual([]);
  });

  it("applies pushed engine snapshots only when they are newer", async () => {
    const { store } = await readyStore();
    store.handlePush({ channel: "engine-changed", payload: makeSnapshot({ revision: 9, status: "degraded" }) });
    store.handlePush({ channel: "engine-changed", payload: makeSnapshot({ revision: 4, status: "stopped" }) });
    expect(store.getState().engine).toMatchObject({ revision: 9, status: "degraded" });
  });
});
