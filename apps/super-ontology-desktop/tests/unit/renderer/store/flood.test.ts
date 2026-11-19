import { describe, expect, it } from "vitest";
import { FLOOD_SLICE, FLOOD_THRESHOLD, createSuperDriveStore } from "@so-model/superDriveStore";
import { folderKey } from "@so-model/superDriveRoute";
import { createFakeApi, DRIVE_ID, makeDrive, makeFile, manualScheduler, okResult } from "../../../support/fakeApi";

// F-S13-01: a cold start that reports thousands of changes is folded a slice per turn, not in one piece, and the list says so.
const events = (count: number, from = 1, state = "published") => Array.from({ length: count }, (_, index) => ({
  seq: from + index, type: "file.state_changed", at: "x", drive_id: DRIVE_ID, data: { file: makeFile(`f-${String(from + index).padStart(5, "0")}.md`, { state: state as never }) },
}));
async function readyStore() {
  const fake = createFakeApi({ listFiles: async () => okResult({ items: [], next_cursor: null, as_of_seq: 0 }), listDrives: async () => okResult([makeDrive()]) });
  const clock = manualScheduler();
  const store = createSuperDriveStore({ api: fake.api, scheduler: clock.scheduler, createId: () => "op_fixed_0123456789" });
  await store.start();
  await store.actions.loadFolder({ driveId: DRIVE_ID, path: "" });
  return { store, clock };
}
const push = (store: Awaited<ReturnType<typeof readyStore>>["store"], list: ReturnType<typeof events>, lastSeq = list.at(-1)!.seq) => store.handlePush({ channel: "events", payload: { instanceId: "1:a", lastSeq, events: list } });
const filesIn = (store: Awaited<ReturnType<typeof readyStore>>["store"]) => Object.keys(store.getState().files).length;

describe("event floods", () => {
  it("folds an ordinary push at once and never raises the flood flag", async () => {
    const { store } = await readyStore();
    push(store, events(FLOOD_THRESHOLD));
    expect(filesIn(store)).toBe(FLOOD_THRESHOLD);
    expect(store.getState().flood).toBeUndefined();
  });

  it("folds a flood a slice at a time, in order, with the flood flag up until it has settled", async () => {
    const { store, clock } = await readyStore();
    push(store, events(5_000));
    expect(store.getState().flood).toBe(true);
    expect(filesIn(store)).toBe(FLOOD_SLICE);
    expect(store.getState().connection.lastSeq).toBe(FLOOD_SLICE);
    for (let slice = 2; slice <= 5; slice += 1) {
      clock.advance(16);
      expect(filesIn(store)).toBe(slice * FLOOD_SLICE);
    }
    expect(store.getState().connection.lastSeq).toBe(5_000);
    expect(store.getState().flood).toBe(true);
    clock.advance(16 + 400);
    expect(store.getState().flood).toBe(false);
    expect(filesIn(store)).toBe(5_000);
  });

  it("works pushes of a flood off one slice per turn too, so the list renders between them", async () => {
    const { store, clock } = await readyStore();
    push(store, events(1_000));
    push(store, events(1_000, 1_001));
    push(store, events(1_000, 2_001));
    expect(filesIn(store)).toBe(1_000);
    clock.advance(16);
    expect(filesIn(store)).toBe(2_000);
    clock.advance(16);
    expect(filesIn(store)).toBe(3_000);
  });

  it("puts a push that arrives during a flood behind it, so the order of events is kept", async () => {
    const { store, clock } = await readyStore();
    push(store, events(2_500));
    push(store, [{ seq: 2_501, type: "file.state_changed", at: "x", drive_id: DRIVE_ID, data: { file: makeFile("f-00001.md", { state: "deleted" }) } }]);
    expect(store.getState().files["f-f-00001.md"].state).toBe("published");
    clock.advance(16 * 4);
    expect(store.getState().files["f-f-00001.md"].state).toBe("deleted");
    expect(store.getState().connection.lastSeq).toBe(2_501);
  });

  it("a reload drops what is still waiting and puts the flag down", async () => {
    const { store, clock } = await readyStore();
    push(store, events(4_000));
    expect(filesIn(store)).toBe(FLOOD_SLICE);
    await store.resync("gap");
    expect(store.getState().flood).toBe(false);
    clock.advance(1_000);
    expect(filesIn(store)).toBe(FLOOD_SLICE);
  });

  it("keeps the list ids of a loaded folder in step with the slices", async () => {
    const { store, clock } = await readyStore();
    push(store, events(1_500, 1, "queued"));
    clock.advance(16);
    expect(store.getState().folders[folderKey(DRIVE_ID, "")].ids).toHaveLength(1_500);
  });
});
