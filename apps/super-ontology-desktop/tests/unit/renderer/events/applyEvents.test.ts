import { describe, expect, it } from "vitest";
import { applyEvents } from "@so-model/superDriveEvents";
import { folderKey } from "@so-model/superDriveRoute";
import { createEmptyFolderPage, createInitialState, type SuperDriveState } from "@so-model/superDriveState";
import type { EngineEvent } from "@so-shared/bridge";
import { DRIVE_ID, makeDrive, makeFile } from "../../../support/fakeApi";

const at = "2026-11-17T00:00:00Z";
const event = (seq: number | undefined, type: string, data: Record<string, unknown>, drive_id: string | null = DRIVE_ID): EngineEvent => ({
  ...(seq === undefined ? {} : { seq }), type, at, drive_id, data,
});
const stateChanged = (seq: number, file: ReturnType<typeof makeFile>) => event(seq, "file.state_changed", { file });

function baseState(paths: string[] = ["a.md", "c.md"]): SuperDriveState {
  const state = createInitialState();
  const files = Object.fromEntries(paths.map((path) => [`f-${path}`, { ...makeFile(path), seq: 5 }]));
  return {
    ...state,
    drives: { ...state.drives, byId: { [DRIVE_ID]: makeDrive({ counts: { queued: paths.length } }) }, order: [DRIVE_ID], loaded: true },
    files,
    folders: { [folderKey(DRIVE_ID, "")]: { ...createEmptyFolderPage(), ids: paths.map((path) => `f-${path}`), asOfSeq: 5 } },
    cursor: { appliedSeq: 5 },
  };
}

describe("applyEvents: file.state_changed", () => {
  it("overwrites a record only with a newer seq and ignores older or equal ones", () => {
    const state = baseState();
    const newer = applyEvents(state, [stateChanged(7, makeFile("a.md", { state: "parsing" }))]);
    expect(newer.files["f-a.md"]).toMatchObject({ state: "parsing", seq: 7 });
    const stale = applyEvents(newer, [stateChanged(6, makeFile("a.md", { state: "queued" })), stateChanged(7, makeFile("a.md", { state: "queued" }))]);
    expect(stale.files["f-a.md"].state).toBe("parsing");
    expect(stale.files).toBe(newer.files);
  });

  it("keeps files_total equal to the sum of the per-state counts (Home showed 0 files after an upload)", () => {
    const state = baseState([]);
    state.drives.byId[DRIVE_ID] = makeDrive({ counts: {}, files_total: 0 });
    const added = applyEvents(state, [stateChanged(7, makeFile("a.md", { state: "queued" })), stateChanged(8, makeFile("b.md", { state: "stored" }))]);
    expect(added.drives.byId[DRIVE_ID].files_total).toBe(2);
    const moved = applyEvents(added, [stateChanged(9, makeFile("a.md", { state: "published" }))]);
    expect(moved.drives.byId[DRIVE_ID].files_total).toBe(2);
  });

  it("drops an event older than the page snapshot it would overwrite", () => {
    const state = baseState();
    expect(applyEvents(state, [stateChanged(4, makeFile("a.md", { state: "published" }))]).files["f-a.md"].state).toBe("queued");
  });

  it("inserts a new file into the loaded folder at its sorted position and keeps the order for later inserts", () => {
    const state = baseState();
    const next = applyEvents(state, [stateChanged(6, makeFile("b.md")), stateChanged(7, makeFile("0.md")), stateChanged(8, makeFile("d.md"))]);
    expect(next.folders[folderKey(DRIVE_ID, "")].ids).toEqual(["f-0.md", "f-a.md", "f-b.md", "f-c.md", "f-d.md"]);
    expect(next.files["f-b.md"].seq).toBe(6);
    expect(state.folders[folderKey(DRIVE_ID, "")].ids).toEqual(["f-a.md", "f-c.md"]);
  });

  it("does not duplicate a file that is already listed and updates counts by moving it between states", () => {
    const state = baseState();
    const next = applyEvents(state, [stateChanged(6, makeFile("a.md", { state: "parsing" })), stateChanged(7, makeFile("a.md", { state: "published" }))]);
    expect(next.folders[folderKey(DRIVE_ID, "")].ids).toEqual(["f-a.md", "f-c.md"]);
    expect(next.drives.byId[DRIVE_ID].counts).toMatchObject({ queued: 1, published: 1 });
    expect(next.drives.byId[DRIVE_ID].counts.parsing).toBe(0);
  });

  it("only counts a file in no loaded folder, and never negative", () => {
    const state = baseState();
    const next = applyEvents(state, [stateChanged(6, makeFile("deep/x.md", { state: "stored" }))]);
    expect(next.folders[folderKey(DRIVE_ID, "")].ids).toEqual(["f-a.md", "f-c.md"]);
    expect(next.files["f-deep/x.md"]).toBeDefined();
    expect(next.drives.byId[DRIVE_ID].counts).toMatchObject({ stored: 1, queued: 2 });
  });

  it("marks filtered listings of that Drive stale instead of patching them", () => {
    const state = baseState();
    const filtered = folderKey(DRIVE_ID, "", { states: ["published"] });
    state.folders[filtered] = { ...createEmptyFolderPage(), asOfSeq: 5 };
    const next = applyEvents(state, [stateChanged(6, makeFile("b.md"))]);
    expect(next.folders[filtered].stale).toBe(true);
    expect(next.folders[folderKey(DRIVE_ID, "")].stale).toBe(false);
  });

  it("invalidates cached details of the file", () => {
    const state = baseState();
    state.details.set("f-a.md", { cached: true });
    applyEvents(state, [stateChanged(6, makeFile("a.md", { state: "parsed" }))]);
    expect(state.details.has("f-a.md")).toBe(false);
  });
});

describe("applyEvents: renames, removals and bulk changes", () => {
  it("moves a renamed file between folders", () => {
    const state = baseState(["a.md", "sub/b.md"]);
    state.folders[folderKey(DRIVE_ID, "sub")] = { ...createEmptyFolderPage(), ids: ["f-sub/b.md"], asOfSeq: 5 };
    const next = applyEvents(state, [event(6, "file.renamed", { file_id: "f-sub/b.md", old_path: "sub/b.md", new_path: "z.md" })]);
    expect(next.folders[folderKey(DRIVE_ID, "sub")].ids).toEqual([]);
    expect(next.folders[folderKey(DRIVE_ID, "")].ids).toEqual(["f-a.md", "f-sub/b.md"]);
    expect(next.files["f-sub/b.md"]).toMatchObject({ path: "z.md", name: "z.md", seq: 6 });
  });

  it("removes a file everywhere and clears it from the selection", () => {
    const state = baseState();
    state.selection = { driveId: DRIVE_ID, ids: new Set(["f-a.md", "f-c.md"]), anchorId: "f-a.md", allMatching: false };
    const next = applyEvents(state, [event(6, "file.removed", { file_id: "f-a.md" })]);
    expect(next.files["f-a.md"]).toBeUndefined();
    expect(next.folders[folderKey(DRIVE_ID, "")].ids).toEqual(["f-c.md"]);
    expect([...next.selection.ids]).toEqual(["f-c.md"]);
    expect(next.selection.anchorId).toBeNull();
    expect(next.drives.byId[DRIVE_ID].counts.queued).toBe(1);
  });

  it("marks every loaded folder of the Drive stale on files.bulk_changed and leaves other Drives alone", () => {
    const state = baseState();
    const other = folderKey("other-drive", "");
    state.folders[other] = { ...createEmptyFolderPage() };
    const next = applyEvents(state, [event(6, "files.bulk_changed", { drive_id: DRIVE_ID, count: 300, states: {} })]);
    expect(next.folders[folderKey(DRIVE_ID, "")].stale).toBe(true);
    expect(next.folders[other].stale).toBe(false);
  });
});

describe("applyEvents: drives, review, shares, usage, ontology", () => {
  it("creates, updates (counts and pending review are authoritative) and deletes drives", () => {
    let state = baseState();
    state = applyEvents(state, [event(6, "drive.created", { drive: makeDrive({ id: "d2", name: "Two" }) })]);
    expect(state.drives.order).toEqual([DRIVE_ID, "d2"]);
    state = applyEvents(state, [event(7, "drive.updated", { drive: makeDrive({ counts: { published: 9 }, pending_review: 4, revision: 3 }) })]);
    expect(state.drives.byId[DRIVE_ID]).toMatchObject({ counts: { published: 9 }, pending_review: 4, revision: 3 });
    state = applyEvents(state, [event(8, "drive.deleted", { drive_id: "d2" })]);
    expect(state.drives.order).toEqual([DRIVE_ID]);
    expect(state.drives.byId.d2).toBeUndefined();
  });

  it("records sync and freshness on the Drive", () => {
    let state = baseState();
    state = applyEvents(state, [event(6, "drive.sync_started", { drive_id: DRIVE_ID, sync: { status: "running", generation: 2 } })]);
    expect(state.drives.byId[DRIVE_ID].last_sync).toMatchObject({ status: "running" });
    state = applyEvents(state, [event(7, "drive.freshness_changed", { drive_id: DRIVE_ID, freshness: { state: "processing", ontology_version: 4, as_of: at } })]);
    expect(state.drives.byId[DRIVE_ID].freshness).toMatchObject({ state: "processing", ontology_version: 4 });
  });

  it("takes the pending review total from the freshness and marks a queue that was read as stale", () => {
    let state = applyEvents(baseState(), [event(6, "review.changed", { drive_id: DRIVE_ID, pending: { low_confidence_fact: 3 }, by_source_changed: [] })]);
    state = applyEvents(state, [event(7, "drive.freshness_changed", { drive_id: DRIVE_ID, freshness: { state: "processing", pending_review: 5, as_of: at } })]);
    expect(state.drives.byId[DRIVE_ID].pending_review).toBe(5);
    expect(state.review.pendingByDrive[DRIVE_ID]).toBeUndefined();
    const same = applyEvents(baseState(), [event(6, "drive.freshness_changed", { drive_id: DRIVE_ID, freshness: { state: "up_to_date", pending_review: baseState().drives.byId[DRIVE_ID].pending_review ?? 0, as_of: at } })]);
    expect(same.review.pendingByDrive[DRIVE_ID]).toBeUndefined();
  });

  it("moves a Drive's known ontology version up when the engine publishes without naming a Drive", () => {
    let state = baseState();
    state = { ...state, ontology: { [DRIVE_ID]: { n: 2, asOf: at } } };
    state = applyEvents(state, [event(6, "ontology.version_published", { n: 3, as_of: at, facts_added: 2, facts_retracted: 0, gates: {} }, null)]);
    expect(state.ontology[DRIVE_ID].n).toBe(3);
    expect(state.ontology["*"].n).toBe(3);
  });

  it("leaves every Drive queue alone for a review.changed that names no Drive", () => {
    const state = applyEvents(baseState(), [event(6, "review.changed", { drive_id: null, pending: { low_confidence_fact: 0 }, by_source_changed: ["f1"] }, null)]);
    expect(state.review.pendingByDrive[DRIVE_ID]).toBeUndefined();
  });

  it("updates pending review per kind and the Drive total", () => {
    const next = applyEvents(baseState(), [event(6, "review.changed", { drive_id: DRIVE_ID, pending: { entity_merge: 2, low_confidence_fact: 3 }, by_source_changed: [] })]);
    expect(next.review.pendingByDrive[DRIVE_ID]).toEqual({ entity_merge: 2, low_confidence_fact: 3 });
    expect(next.drives.byId[DRIVE_ID].pending_review).toBe(5);
  });

  it("upserts shares, chains refreshed shares to their root and clears the mount flag", () => {
    const share = (id: string, extra: Record<string, unknown> = {}) => ({ id, state: "active", mount: { present: true }, ...extra });
    let state = applyEvents(baseState(), [event(6, "share.created", { share: share("s1") })]);
    state = applyEvents(state, [event(7, "share.created", { share: share("s2", { refreshed_from: "s1" }) })]);
    state = applyEvents(state, [event(8, "share.created", { share: share("s3", { refreshed_from: "s2" }) })]);
    expect(state.shares.order).toEqual(["s3", "s2", "s1"]);
    expect(state.shares.chains).toEqual({ s1: ["s1", "s2", "s3"] });
    state = applyEvents(state, [event(9, "share.state_changed", { share: share("s1", { state: "refreshed" }) })]);
    expect(state.shares.byId.s1.state).toBe("refreshed");
    expect(state.shares.order).toEqual(["s3", "s2", "s1"]);
    state = applyEvents(state, [event(10, "share.mount_removed", { share_id: "s3", removed: true })]);
    expect((state.shares.byId.s3 as unknown as { mount: { present: boolean } }).mount.present).toBe(false);
  });

  it("tracks usage and the ontology version, and drops cached entities on publish", () => {
    const state = baseState();
    state.entities.set("e1", { cached: true });
    const next = applyEvents(state, [
      event(6, "usage.changed", { drive_id: DRIVE_ID, day: "2026-11-17", calls: 40, limit: 40, limited: true }),
      event(7, "ontology.version_published", { n: 5, as_of: at, facts_added: 1, facts_retracted: 0, gates: {} }),
    ]);
    expect(next.usage[DRIVE_ID]).toEqual({ day: "2026-11-17", calls: 40, limit: 40, limited: true });
    expect(next.ontology[DRIVE_ID]).toEqual({ n: 5, asOf: at });
    expect(state.entities.has("e1")).toBe(false);
  });
});

describe("applyEvents: cursor and transient events", () => {
  it("advances the cursor by persistent events only and never goes back", () => {
    const state = baseState();
    const next = applyEvents(state, [
      stateChanged(9, makeFile("a.md", { state: "parsing" })),
      event(undefined, "file.progress", { file_id: "f-a.md", unit: "page", done: 3, total: 12 }),
      stateChanged(8, makeFile("c.md", { state: "parsed" })),
    ]);
    expect(next.cursor.appliedSeq).toBe(9);
    expect(applyEvents(next, [stateChanged(2, makeFile("a.md"))]).cursor.appliedSeq).toBe(9);
  });

  it("applies file.progress to the row without a seq and ignores unknown files and units", () => {
    const state = baseState();
    const next = applyEvents(state, [
      event(undefined, "file.progress", { file_id: "f-a.md", unit: "page", done: 3, total: 12 }),
      event(undefined, "file.progress", { file_id: "nope", unit: "page", done: 1, total: 2 }),
      event(undefined, "file.progress", { file_id: "f-c.md", unit: "furlong", done: 1, total: 2 }),
    ]);
    expect(next.files["f-a.md"].progress).toEqual({ unit: "page", done: 3, total: 12 });
    expect(next.files["f-c.md"].progress).toBeUndefined();
    expect(next.cursor.appliedSeq).toBe(5);
  });

  it("ignores unknown event types, malformed data, and returns the same state for an empty batch", () => {
    const state = baseState();
    expect(applyEvents(state, [])).toBe(state);
    const next = applyEvents(state, [
      event(6, "something.new", {}), event(7, "file.state_changed", { file: "nope" }),
      event(8, "drive.updated", {}), event(9, "file.renamed", { file_id: 3 }),
    ]);
    expect(next.files).toBe(state.files);
    expect(next.drives).toBe(state.drives);
    expect(next.cursor.appliedSeq).toBe(9);
  });

  it("does not mutate the input state", () => {
    const state = baseState();
    const snapshot = JSON.stringify({ files: state.files, folders: state.folders, drives: state.drives });
    applyEvents(state, [stateChanged(6, makeFile("b.md")), stateChanged(7, makeFile("a.md", { state: "published" })), event(8, "file.removed", { file_id: "f-c.md" })]);
    expect(JSON.stringify({ files: state.files, folders: state.folders, drives: state.drives })).toBe(snapshot);
  });
});
