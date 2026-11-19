import { describe, expect, it } from "vitest";
import { createSuperDriveStore, FOLDER_PAGE_LIMIT } from "@so-model/superDriveStore";
import { folderKey, hashFilter, parseFolderKey } from "@so-model/superDriveRoute";
import { selectFolder, selectFolderFiles } from "@so-model/superDriveSelectors";
import { createFakeApi, DRIVE_ID, failResult, makeFile, manualScheduler, okResult } from "../../../support/fakeApi";

const names = (from: number, to: number) => Array.from({ length: to - from }, (_, index) => `f${String(from + index).padStart(5, "0")}.md`);
const pageOf = (paths: string[], asOf: number, next: string | null) => ({ items: paths.map((path) => makeFile(path)), next_cursor: next, as_of_seq: asOf });

function setup(overrides: Parameters<typeof createFakeApi>[0] = {}) {
  const fake = createFakeApi(overrides);
  const store = createSuperDriveStore({ api: fake.api, scheduler: manualScheduler().scheduler });
  return { ...fake, store };
}

describe("folder keys", () => {
  it("hash equal filters equal and the default filter to the empty string", () => {
    expect(hashFilter(undefined)).toBe("");
    expect(hashFilter({})).toBe("");
    expect(hashFilter({ sort: "path", recursive: false, states: [] })).toBe("");
    expect(hashFilter({ states: ["b", "a"] })).toBe(hashFilter({ states: ["a", "b"] }));
    expect(hashFilter({ q: "x" })).not.toBe(hashFilter({ q: "y" }));
    expect(parseFolderKey(folderKey("d", "a/b", { q: "x" }))).toMatchObject({ driveId: "d", path: "a/b" });
  });
});

describe("folder pages", () => {
  it("loads the first page with subfolders from tree and files from files-list, aligned to as_of_seq", async () => {
    const { api, store } = setup({
      driveTree: async () => okResult({ dirs: [{ path: "Sub", files: 3, counts: { failed: 1 } }], files: pageOf([], 0, null) }),
      listFiles: async () => okResult(pageOf(["a.md", "b.md"], 42, null)),
    });
    const page = await store.actions.loadFolder({ driveId: DRIVE_ID, path: "" });
    expect(page).toMatchObject({ ids: ["f-a.md", "f-b.md"], nextCursor: null, total: 2, asOfSeq: 42, loading: false, stale: false, error: null });
    expect(page?.dirs).toEqual([{ path: "Sub", files: 3, counts: { failed: 1 } }]);
    expect(store.getState().files["f-a.md"].seq).toBe(42);
    expect(api.listFiles).toHaveBeenCalledWith(expect.objectContaining({ driveId: DRIVE_ID, path: "", limit: FOLDER_PAGE_LIMIT }));
  });

  it("pages with keyset cursors, keeps order, de-duplicates and learns the total at the end", async () => {
    const pages: Record<string, ReturnType<typeof pageOf>> = {
      first: pageOf(names(0, 3), 10, "c1"), c1: pageOf(names(2, 6), 12, "c2"), c2: pageOf(names(6, 8), 14, null),
    };
    const { api, store } = setup({ listFiles: async ({ cursor }: { cursor?: string }) => okResult(pages[cursor ?? "first"]) });
    const key = folderKey(DRIVE_ID, "");
    await store.actions.loadFolder({ driveId: DRIVE_ID, path: "" });
    expect(store.getState().folders[key]).toMatchObject({ nextCursor: "c1", total: null });
    await store.actions.loadMore(key);
    await store.actions.loadMore(key);
    await store.actions.loadMore(key); // Nothing left: no request.
    expect(store.getState().folders[key].ids).toEqual(names(0, 8).map((name) => `f-${name}`));
    expect(store.getState().folders[key]).toMatchObject({ nextCursor: null, total: 8 });
    expect(api.listFiles).toHaveBeenCalledTimes(3);
    expect(api.listFiles).toHaveBeenLastCalledWith(expect.objectContaining({ cursor: "c2" }));
  });

  it("holds 10,000 files in 20 pages of 500 and exposes rows through the selector", async () => {
    const total = 10_000;
    const { store } = setup({
      listFiles: async ({ cursor }: { cursor?: string }) => {
        const index = cursor ? Number(cursor) : 0;
        const next = index + FOLDER_PAGE_LIMIT;
        return okResult(pageOf(names(index, Math.min(next, total)), 100, next < total ? String(next) : null));
      },
    });
    const key = folderKey(DRIVE_ID, "");
    await store.actions.loadFolder({ driveId: DRIVE_ID, path: "" });
    while (store.getState().folders[key].nextCursor !== null) await store.actions.loadMore(key);
    const state = store.getState();
    expect(state.folders[key].ids).toHaveLength(total);
    expect(state.folders[key].total).toBe(total);
    const rows = selectFolderFiles(state, selectFolder(state, DRIVE_ID, ""));
    expect(rows).toHaveLength(total);
    expect(rows[total - 1].path).toBe(`f${String(total - 1).padStart(5, "0")}.md`);
  });

  it("does not request a loaded fresh folder again but refetches stale, failed or forced ones", async () => {
    const { api, store } = setup({ listFiles: async () => okResult(pageOf(["a.md"], 5, null)) });
    await store.actions.loadFolder({ driveId: DRIVE_ID, path: "" });
    await store.actions.loadFolder({ driveId: DRIVE_ID, path: "" });
    expect(api.listFiles).toHaveBeenCalledTimes(1);
    await store.actions.reloadFolder(folderKey(DRIVE_ID, ""));
    expect(api.listFiles).toHaveBeenCalledTimes(2);
    store.handlePush({ channel: "events", payload: { instanceId: "i", lastSeq: 9, events: [{ seq: 9, type: "files.bulk_changed", at: "x", drive_id: DRIVE_ID, data: { drive_id: DRIVE_ID } }] } });
    expect(store.getState().folders[folderKey(DRIVE_ID, "")].stale).toBe(true);
    await store.actions.loadFolder({ driveId: DRIVE_ID, path: "" });
    expect(api.listFiles).toHaveBeenCalledTimes(3);
    expect(store.getState().folders[folderKey(DRIVE_ID, "")].stale).toBe(false);
  });

  it("keeps a failed listing readable with its error and retries cleanly", async () => {
    let fail = true;
    const { store } = setup({ listFiles: async () => (fail ? failResult("DOCS_ENGINE_UNAVAILABLE", { retryable: true }) : okResult(pageOf(["a.md"], 3, null))) });
    const key = folderKey(DRIVE_ID, "");
    await store.actions.loadFolder({ driveId: DRIVE_ID, path: "" });
    expect(store.getState().folders[key]).toMatchObject({ loading: false, error: { code: "DOCS_ENGINE_UNAVAILABLE", retryable: true }, ids: [] });
    fail = false;
    await store.actions.loadFolder({ driveId: DRIVE_ID, path: "" });
    expect(store.getState().folders[key]).toMatchObject({ error: null, ids: ["f-a.md"] });
  });

  it("applies a filtered listing flat (no tree call, no subfolders) under its own key", async () => {
    const { api, store } = setup({ listFiles: async () => okResult(pageOf(["x/a.md"], 8, null)) });
    const filter = { states: ["parse_failed"], recursive: true };
    await store.actions.loadFolder({ driveId: DRIVE_ID, path: "", filter });
    expect(api.driveTree).not.toHaveBeenCalled();
    expect(api.listFiles).toHaveBeenCalledWith(expect.objectContaining({ recursive: true, states: ["parse_failed"] }));
    expect(store.getState().folders[folderKey(DRIVE_ID, "", filter)]).toMatchObject({ ids: ["f-x/a.md"], dirs: [] });
    expect(store.getState().folders[folderKey(DRIVE_ID, "")]).toBeUndefined();
  });

  it("ignores a response that a newer request for the same folder has superseded", async () => {
    const resolvers: Array<(value: unknown) => void> = [];
    const { store } = setup({ listFiles: () => new Promise((resolve) => { resolvers.push(resolve); }) });
    const first = store.actions.loadFolder({ driveId: DRIVE_ID, path: "" });
    const second = store.actions.loadFolder({ driveId: DRIVE_ID, path: "", force: true });
    await Promise.resolve();
    resolvers[1](okResult(pageOf(["new.md"], 9, null)));
    await second;
    resolvers[0](okResult(pageOf(["old.md"], 1, null)));
    await first;
    expect(store.getState().folders[folderKey(DRIVE_ID, "")].ids).toEqual(["f-new.md"]);
  });

  it("does not overwrite a record that an event already advanced past the page snapshot", async () => {
    const { store } = setup({ listFiles: async () => okResult(pageOf(["a.md"], 5, null)) });
    store.handlePush({ channel: "events", payload: { instanceId: "i", lastSeq: 9, events: [{ seq: 9, type: "file.state_changed", at: "x", drive_id: DRIVE_ID, data: { file: makeFile("a.md", { state: "published" }) } }] } });
    await store.actions.loadFolder({ driveId: DRIVE_ID, path: "" });
    expect(store.getState().files["f-a.md"]).toMatchObject({ state: "published", seq: 9 });
  });

  it("clears the selection when the folder changes", async () => {
    const { store } = setup();
    await store.start();
    store.actions.setSelection(["x", "y"], "x");
    expect(store.getState().selection.ids.size).toBe(2);
    store.actions.openFolder("sub");
    expect(store.getState().selection.ids.size).toBe(0);
    expect(store.getState().route.path).toBe("sub");
  });
});
