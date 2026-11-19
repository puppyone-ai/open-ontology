import { describe, expect, it } from "vitest";
import { applyEvents } from "@so-model/superDriveEvents";
import { folderKey } from "@so-model/superDriveRoute";
import { createEmptyFolderPage, createInitialState, type SuperDriveState } from "@so-model/superDriveState";
import type { EngineEvent } from "@so-shared/bridge";
import { DRIVE_ID, makeDrive, makeFile } from "../../../support/fakeApi";

const at = "2026-11-17T00:00:00Z";
const event = (seq: number, type: string, data: Record<string, unknown>): EngineEvent => ({ seq, type, at, drive_id: DRIVE_ID, data });
const changed = (seq: number, path: string, state: string, extra: Record<string, unknown> = {}) => event(seq, "file.state_changed", { file: makeFile(path, { state: state as never, ...extra }) });
const rootIds = (state: SuperDriveState) => state.folders[folderKey(DRIVE_ID, "")].ids;
const names = (state: SuperDriveState) => rootIds(state).map((id) => state.files[id]?.path);

function empty(backend: "fs" | "s3"): SuperDriveState {
  const state = createInitialState();
  return {
    ...state,
    drives: { ...state.drives, byId: { [DRIVE_ID]: makeDrive({ backend, counts: {} }) }, order: [DRIVE_ID], loaded: true },
    folders: { [folderKey(DRIVE_ID, "")]: { ...createEmptyFolderPage(), asOfSeq: 1 } },
    cursor: { appliedSeq: 1 },
  };
}

// 30 L3 acceptance 5, front half: add, change, rename, delete look the same on screen whichever backend feeds the events.
describe.each(["fs", "s3"] as const)("a file's life on a %s Drive", (backend) => {
  it("appears, changes, is renamed in place and ends as deleted or removed", () => {
    let state = applyEvents(empty(backend), [changed(2, "plan.md", "stored")]);
    expect(names(state)).toEqual(["plan.md"]);
    state = applyEvents(state, [changed(3, "plan.md", "queued"), changed(4, "plan.md", "published")]);
    expect(state.files["f-plan.md"].state).toBe("published");
    expect(state.drives.byId[DRIVE_ID].counts).toMatchObject({ published: 1 });

    state = applyEvents(state, [event(5, "file.renamed", { file_id: "f-plan.md", old_path: "plan.md", new_path: "plan-v2.md" })]);
    expect(names(state)).toEqual(["plan-v2.md"]);
    expect(state.files["f-plan.md"]).toMatchObject({ path: "plan-v2.md", name: "plan-v2.md", state: "published" });

    state = applyEvents(state, [changed(6, "plan-v2.md", "deleted", { id: "f-plan.md" })]);
    expect(state.files["f-plan.md"].state).toBe("deleted");
    expect(names(state)).toEqual(["plan-v2.md"]);
    expect(state.drives.byId[DRIVE_ID].counts).toMatchObject({ deleted: 1, published: 0 });

    state = applyEvents(state, [event(7, "file.removed", { file_id: "f-plan.md" })]);
    expect(rootIds(state)).toEqual([]);
    expect(state.files["f-plan.md"]).toBeUndefined();
  });
});

describe("what a rename and a removal do to the open details", () => {
  const open = (state: SuperDriveState, fileId: string): SuperDriveState => ({ ...state, route: { ...state.route, detailsFileId: fileId }, selection: { ...state.selection, ids: new Set([fileId]), anchorId: fileId } });

  it("a rename drops the cached detail (it still shows the old path) and keeps the selection", () => {
    let state = applyEvents(empty("fs"), [changed(2, "a.md", "published")]);
    state = open(state, "f-a.md");
    state.details.set("f-a.md", { path: "a.md" });
    const next = applyEvents(state, [event(3, "file.renamed", { file_id: "f-a.md", old_path: "a.md", new_path: "b.md" })]);
    expect(next.details.get("f-a.md")).toBeUndefined();
    expect(next.selection.ids.has("f-a.md")).toBe(true);
    expect(next.route.detailsFileId).toBe("f-a.md");
  });

  it("a removal closes the pane that was open on the file and clears the selection", () => {
    let state = applyEvents(empty("s3"), [changed(2, "a.md", "published"), changed(3, "b.md", "published")]);
    state = open(state, "f-a.md");
    const next = applyEvents(state, [event(4, "file.removed", { file_id: "f-a.md" })]);
    expect(next.route.detailsFileId).toBeNull();
    expect(next.selection.ids.size).toBe(0);
    const other = applyEvents(open(state, "f-b.md"), [event(4, "file.removed", { file_id: "f-a.md" })]);
    expect(other.route.detailsFileId).toBe("f-b.md");
  });
});

describe("a file in a new subfolder (T1 formats-and-failures found the root listing staying empty)", () => {
  it("marks the loaded ancestors that lack the folder row as stale, and leaves ones that have it", () => {
    let state = empty("fs");
    state = { ...state, folders: { ...state.folders, [folderKey(DRIVE_ID, "a")]: { ...createEmptyFolderPage(), asOfSeq: 1, dirs: [{ path: "a/b", files: 1, counts: {} }] } } };
    const next = applyEvents(state, [changed(2, "a/b/c/new.md", "stored")]);
    expect(next.folders[folderKey(DRIVE_ID, "")].stale).toBe(true);
    expect(next.folders[folderKey(DRIVE_ID, "a")].stale).toBe(false);
    const direct = applyEvents(empty("fs"), [changed(2, "top.md", "stored")]);
    expect(direct.folders[folderKey(DRIVE_ID, "")].stale).toBe(false);
  });
});
