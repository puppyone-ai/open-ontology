import { describe, expect, it } from "vitest";
import { createSuperDriveStore } from "@so-model/superDriveStore";
import { createFakeApi, DRIVE_ID, failResult, makeDrive, makeFile, manualScheduler, okResult } from "../../../support/fakeApi";

async function setup(overrides: Parameters<typeof createFakeApi>[0] = {}) {
  const fake = createFakeApi(overrides);
  const store = createSuperDriveStore({ api: fake.api, scheduler: manualScheduler().scheduler, createId: () => "op_fixed_0123456789" });
  await store.start();
  return { ...fake, store };
}

describe("Drive edits are optimistic with rollback", () => {
  it("shows the change at once and replaces it with the engine's Drive", async () => {
    let release: (value: unknown) => void = () => undefined;
    const { api, store } = await setup({ updateDrive: () => new Promise((resolve) => { release = resolve; }) });
    const pending = store.actions.updateDrive(DRIVE_ID, { name: "Renamed", modelEnabled: true });
    expect(store.getState().drives.byId[DRIVE_ID]).toMatchObject({ name: "Renamed", model_enabled: true, revision: 1 });
    release(okResult(makeDrive({ name: "Renamed", model_enabled: true, revision: 2 })));
    expect((await pending).ok).toBe(true);
    expect(store.getState().drives.byId[DRIVE_ID].revision).toBe(2);
    expect(api.updateDrive).toHaveBeenCalledWith({ driveId: DRIVE_ID, revision: 1, patch: { name: "Renamed", model_enabled: true } });
  });

  it("maps pause, daily limit, excludes and the auto-publish threshold to the DrivePatch fields", async () => {
    const { api, store } = await setup();
    await store.actions.updateDrive(DRIVE_ID, { paused: true, llmDailyCalls: 50, exclude: ["*.bak"], autoPublishBp: 8000 });
    expect(api.updateDrive).toHaveBeenCalledWith({
      driveId: DRIVE_ID, revision: 1, patch: { paused: true, llm_daily_calls: 50, exclude: ["*.bak"], review: { auto_publish_bp: 8000 } },
    });
  });

  it("rolls back on a failure and reports the error", async () => {
    const { store } = await setup({ updateDrive: async () => failResult("DOCS_SOURCE_ROOT_MISSING") });
    const result = await store.actions.updateDrive(DRIVE_ID, { paused: true });
    expect(result).toMatchObject({ ok: false, error: { code: "DOCS_SOURCE_ROOT_MISSING" } });
    expect(store.getState().drives.byId[DRIVE_ID]).toMatchObject({ status: "active", revision: 1 });
  });

  it("on a 412 reloads the latest Drive and tells the caller it changed elsewhere", async () => {
    const { api, store } = await setup({
      updateDrive: async () => failResult("DOCS_ENGINE_PRECONDITION_FAILED", { details: { current_revision: 9 } }),
      getDrive: async () => okResult(makeDrive({ name: "Changed elsewhere", revision: 9 })),
    });
    const result = await store.actions.updateDrive(DRIVE_ID, { name: "Mine" });
    expect(result).toMatchObject({ ok: false, error: { code: "DOCS_ENGINE_PRECONDITION_FAILED", details: { current_revision: 9 } } });
    expect(store.getState().drives.byId[DRIVE_ID]).toMatchObject({ name: "Changed elsewhere", revision: 9 });
    expect(api.getDrive).toHaveBeenCalledWith(DRIVE_ID);
  });

  it("does not guess for registration, deletion and sync: nothing changes before the engine answers", async () => {
    let release: (value: unknown) => void = () => undefined;
    const { store } = await setup({ createDrive: () => new Promise((resolve) => { release = resolve; }) });
    const before = store.getState().drives.order.length;
    const pending = store.actions.createDrive({ name: "New", rootGrantId: "grant-0001-0001" });
    expect(store.getState().drives.order.length).toBe(before);
    release(okResult(makeDrive({ id: "00000000-0000-4000-8000-0000000000aa", name: "New" })));
    await pending;
    expect(store.getState().drives.order.length).toBe(before + 1);
    expect(store.getState().route.driveId).toBe("00000000-0000-4000-8000-0000000000aa");
  });

  it("reuses one operationId for a retried create and keeps the old Drive list on failure", async () => {
    const { api, store } = await setup({ createDrive: async () => failResult("SO_TIMEOUT", { retryable: true }) });
    const first = await store.actions.createDrive({ name: "New", rootGrantId: "grant-0001-0001", operationId: "op_retry_0123456789" });
    const second = await store.actions.createDrive({ name: "New", rootGrantId: "grant-0001-0001", operationId: "op_retry_0123456789" });
    expect(first).toMatchObject({ ok: false, error: { code: "SO_TIMEOUT", retryable: true } });
    expect(second.ok).toBe(false);
    expect(api.createDrive.mock.calls.map(([input]) => input.operationId)).toEqual(["op_retry_0123456789", "op_retry_0123456789"]);
    expect(store.getState().drives.order).toEqual([DRIVE_ID]);
  });

  it("generates a valid operationId when the caller gives none", async () => {
    const { api, store } = await setup();
    await store.actions.createDrive({ name: "New", rootGrantId: "grant-0001-0001" });
    expect(api.createDrive.mock.calls[0][0].operationId).toBe("op_fixed_0123456789");
  });

  it("reports a deleting Drive from the engine response and does not remove it early", async () => {
    const { store } = await setup();
    await store.actions.deleteDrive(DRIVE_ID);
    expect(store.getState().drives.byId[DRIVE_ID].status).toBe("deleting");
  });
});

describe("engine actions", () => {
  it("run through busy and idle, store the snapshot and surface errors", async () => {
    const { api, store } = await setup({ startEngine: async () => failResult("SO_DOCKER_STOPPED", { retryable: true }) });
    const pending = store.actions.startEngine();
    expect(store.getState().engineAction.busy).toBe(true);
    expect(await pending).toMatchObject({ ok: false, error: { code: "SO_DOCKER_STOPPED" } });
    expect(store.getState().engineAction).toEqual({ busy: false, error: expect.objectContaining({ code: "SO_DOCKER_STOPPED" }) });
    await store.actions.runPreflight();
    expect(store.getState().engineAction.error).toBeNull();
    expect(api.preflight).toHaveBeenCalled();
  });

  it("choosing the oo location asks the picker first and does nothing when cancelled", async () => {
    const { api, store } = await setup();
    expect(await store.actions.chooseOoExecutable()).toEqual({ ok: true, value: null });
    expect(api.setOoExecutable).not.toHaveBeenCalled();
    api.pickPath.mockResolvedValueOnce(okResult({ grantId: "grant-0001-0001", displayPath: "/x/oo" }));
    await store.actions.chooseOoExecutable();
    expect(api.setOoExecutable).toHaveBeenCalledWith("grant-0001-0001");
  });
});

describe("batch retry is optimistic with per-file rollback", () => {
  const failed = (path: string) => makeFile(path, { state: "parse_failed", state_code: "DOCS_PARSE_TIMEOUT", retryable: true });
  async function withFiles(overrides: Parameters<typeof createFakeApi>[0] = {}, count = 3) {
    const paths = Array.from({ length: count }, (_, index) => `f${index}.md`);
    const fake = await setup({ listFiles: async () => okResult({ items: paths.map(failed), next_cursor: null, as_of_seq: 5 }), ...overrides });
    await fake.store.actions.loadFolder({ driveId: DRIVE_ID, path: "" });
    return { ...fake, ids: paths.map((path) => `f-${path}`) };
  }

  it("shows the files queued at once and keeps that when the engine accepts all of them", async () => {
    let release: (value: unknown) => void = () => undefined;
    const { store, ids, api } = await withFiles({ retryFiles: () => new Promise((resolve) => { release = resolve; }) });
    const pending = store.actions.retryFiles(ids);
    expect(ids.map((id) => store.getState().files[id].state)).toEqual(["queued", "queued", "queued"]);
    expect(store.getState().files[ids[0]].optimistic?.op).toBe("retry");
    release(okResult({ results: ids.map((id) => ({ id, status: "ok" })) }));
    expect(await pending).toEqual({ ok: true, value: { accepted: 3, rejected: 0 } });
    expect(store.getState().files[ids[0]].state).toBe("queued");
    expect(api.retryFiles).toHaveBeenCalledWith({ operationId: "op_fixed_0123456789-0", fileIds: ids, mode: "normal" });
    expect(store.getState().notices.at(-1)).toMatchObject({ kind: "batch_result", values: { accepted: 3, rejected: 0 } });
  });

  it("restores only the files the engine refused, with their old state", async () => {
    const { store, ids } = await withFiles();
    const refusing = await setup({
      listFiles: async () => okResult({ items: ["f0.md", "f1.md", "f2.md"].map(failed), next_cursor: null, as_of_seq: 5 }),
      retryFiles: async () => okResult({ results: [{ id: ids[0], status: "ok" }, { id: ids[1], status: "not_retryable" }, { id: ids[2], status: "conflict" }] }),
    });
    await refusing.store.actions.loadFolder({ driveId: DRIVE_ID, path: "" });
    const result = await refusing.store.actions.retryFiles(ids, { mode: "safe" });
    expect(result).toEqual({ ok: true, value: { accepted: 1, rejected: 2 } });
    const files = refusing.store.getState().files;
    expect(files[ids[0]].state).toBe("queued");
    expect(files[ids[1]]).toMatchObject({ state: "parse_failed", state_code: "DOCS_PARSE_TIMEOUT", retryable: true });
    expect(files[ids[1]].optimistic).toBeUndefined();
    expect(files[ids[2]].state).toBe("parse_failed");
    expect(refusing.api.retryFiles).toHaveBeenCalledWith(expect.objectContaining({ mode: "safe" }));
    void store;
  });

  it("rolls everything back when a batch fails as a whole and reports the error", async () => {
    const { store, ids } = await withFiles({ retryFiles: async () => failResult("DOCS_ENGINE_UNAVAILABLE", { retryable: true }) });
    const result = await store.actions.retryFiles(ids);
    expect(result).toMatchObject({ ok: false, error: { code: "DOCS_ENGINE_UNAVAILABLE" } });
    expect(ids.map((id) => store.getState().files[id].state)).toEqual(["parse_failed", "parse_failed", "parse_failed"]);
  });

  it("cuts a long list into batches of 500, rolling back only the batches not yet accepted", async () => {
    const count = 1200;
    const calls: number[] = [];
    const { store, ids } = await withFiles({
      listFiles: async () => okResult({ items: Array.from({ length: count }, (_, index) => failed(`g${index}.md`)), next_cursor: null, as_of_seq: 5 }),
      retryFiles: async ({ fileIds }: { fileIds: string[] }) => {
        calls.push(fileIds.length);
        return calls.length === 3 ? failResult("SO_TIMEOUT", { retryable: true }) : okResult({ results: fileIds.map((id) => ({ id, status: "ok" })) });
      },
    });
    void ids;
    const all = Array.from({ length: count }, (_, index) => `f-g${index}.md`);
    const result = await store.actions.retryFiles(all);
    expect(calls).toEqual([500, 500, 200]);
    expect(result).toMatchObject({ ok: false, error: { code: "SO_TIMEOUT" } });
    const states = store.getState().files;
    expect(states[all[0]].state).toBe("queued");
    expect(states[all[999]].state).toBe("queued");
    expect(states[all[1000]].state).toBe("parse_failed");
  });

  it("does not undo a newer event that arrived while the request was in flight", async () => {
    let release: (value: unknown) => void = () => undefined;
    const { store, ids, push } = await withFiles({ retryFiles: () => new Promise((resolve) => { release = resolve; }) }, 1);
    const pending = store.actions.retryFiles(ids);
    push({ channel: "events", payload: { instanceId: "1:a", lastSeq: 9, events: [{ seq: 9, type: "file.state_changed", at: "2026-11-17T00:00:01Z", drive_id: DRIVE_ID, data: { file: { ...failed("f0.md"), state: "parsing" } } }] } });
    release(okResult({ results: [{ id: ids[0], status: "conflict" }] }));
    await pending;
    expect(store.getState().files[ids[0]].state).toBe("parsing");
  });

  it("retries everything matching a filter through the main process and reloads the list", async () => {
    const { api, store } = await withFiles({ retryMatching: async () => okResult({ results: [{ id: "a", status: "ok" }, { id: "b", status: "not_retryable" }], matched: 2 }) });
    const result = await store.actions.retryMatching({ driveId: DRIVE_ID, states: ["parse_failed"], recursive: true });
    expect(result).toEqual({ ok: true, value: { matched: 2, accepted: 1, rejected: 1 } });
    expect(api.retryMatching).toHaveBeenCalledWith(expect.objectContaining({ driveId: DRIVE_ID, states: ["parse_failed"], recursive: true, mode: "normal" }));
  });
});
