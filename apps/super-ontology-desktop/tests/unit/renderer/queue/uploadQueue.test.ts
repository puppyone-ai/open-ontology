import { describe, expect, it, vi } from "vitest";
import type { UploadBatchSnapshot, UploadProgressPush } from "../../../../src/shared/super-ontology/bridge";
import {
  addingOf, applyUploadProgress, createUploadQueue, DONE_RETAIN_MS, handoffStatus, HANDOFF_PATIENCE_MS, initialUploadQueueState, isOverLimit,
  SETTLED_ROWS_KEPT, summarizeDrop, summarizeUploads, type BatchInfo, type UploadItem, type UploadQueueState,
} from "@so-model/queue/uploadQueue";
import { createFakeApi, okResult, failResult } from "../../../support/fakeApi";

const BATCH = "00000000-0000-4000-8000-0000000000b1";
const item = (overrides: Partial<UploadItem> = {}): UploadItem => ({
  id: "i1", batchId: "b1", driveId: "d1", name: "a.md", relative: "a.md", folder: "", source: "/drop/a.md", size: 100, state: "waiting",
  bytesDone: 0, code: null, drivePath: null, finishedAt: null, overLimit: false, ...overrides,
});
const batchInfo = (overrides: Partial<BatchInfo> = {}): BatchInfo => ({
  id: "b1", driveId: "d1", folder: "", phase: "copying", found: 0, bytes: 0, queued: 0, counts: { done: 0, failed: 0, skipped: 0, cancelled: 0 },
  skipped: { hidden: 0, links: 0, junk: 0, temporary: 0, special: 0, unreadable: 0, gitignore: 0, ooignore: 0 }, samples: [], roots: [], large: [], emptyFolders: 0, problem: null, ...overrides,
});
const snapshot = (overrides: Partial<UploadBatchSnapshot> = {}): UploadBatchSnapshot => ({
  phase: "copying", found: 0, bytes: 0, queued: 0, counts: { done: 0, failed: 0, skipped: 0, cancelled: 0 },
  skipped: { hidden: 0, links: 0, junk: 0, temporary: 0, special: 0, gitignore: 0, ooignore: 0 }, skippedSamples: [], roots: [], large: [], emptyFolders: 0, unreadable: 0, ...overrides,
});
const stateWith = (items: UploadItem[], extra: Partial<UploadQueueState> = {}): UploadQueueState => ({ ...initialUploadQueueState(), items, batches: [batchInfo()], ...extra });
const progress = (state: string, extra: Record<string, unknown> = {}): UploadProgressPush => ({ batchId: "b1", items: [{ itemId: "i1", state, bytesDone: 50, bytesTotal: 100, ...extra }] });

describe("upload progress folding", () => {
  it("moves an item through copying to done and records the final path and the time", () => {
    const start = stateWith([item()]);
    const copying = applyUploadProgress(start, progress("copying"), 10);
    expect(copying.items[0]).toMatchObject({ state: "copying", bytesDone: 50, finishedAt: null });
    const done = applyUploadProgress(copying, progress("done", { drivePath: "docs/a (2).md", bytesDone: 100 }), 20);
    expect(done.items[0]).toMatchObject({ state: "done", drivePath: "docs/a (2).md", finishedAt: 20 });
    expect(applyUploadProgress(done, progress("done", { drivePath: "docs/a (2).md", bytesDone: 100 }), 99)).toBe(done);
  });

  it("ignores unknown batches and items, and reopens a dismissed panel on a failure", () => {
    const state = stateWith([item()], { dismissed: true });
    expect(applyUploadProgress(state, { batchId: "zzz", items: [{ itemId: "i1", state: "done", bytesDone: 1, bytesTotal: 1 }] }, 1)).toBe(state);
    expect(applyUploadProgress(state, { batchId: "b1", items: [{ itemId: "zzz", state: "done", bytesDone: 1, bytesTotal: 1 }] }, 1)).toBe(state);
    const failed = applyUploadProgress(state, progress("failed", { code: "SO_UPLOAD_DISK_FULL" }), 5);
    expect(failed.items[0]).toMatchObject({ state: "failed", code: "SO_UPLOAD_DISK_FULL" });
    expect(failed.dismissed).toBe(false);
    expect(summarizeUploads(failed)).toMatchObject({ active: 0, failed: 1 });
  });

  it("takes the batch's counts, adds the rows it released under the batch's folder, and sums what was skipped", () => {
    const start = { ...initialUploadQueueState(), batches: [batchInfo({ folder: "in" })] };
    const next = applyUploadProgress(start, {
      batchId: "b1", items: [],
      added: [{ itemId: "x1", name: "a.md", relative: "P/sub/a.md", size: 10, source: "/s/P/sub/a.md" }, { itemId: "x2", name: "b.md", relative: "b.md", size: 20, source: "/s/b.md" }],
      batch: snapshot({ phase: "scanning", found: 2, queued: 2, skipped: { hidden: 1, links: 0, junk: 4, temporary: 0, special: 0, gitignore: 2, ooignore: 1 }, unreadable: 3, skippedSamples: [{ name: "node_modules", reason: "junk" }] }),
    }, 1);
    expect(next.items.map((entry) => [entry.id, entry.folder, entry.state])).toEqual([["x1", "in/P/sub", "waiting"], ["x2", "in", "waiting"]]);
    expect(next.batches[0]).toMatchObject({ phase: "scanning", found: 2, queued: 2 });
    expect(next.skipped).toEqual({ hidden: 1, links: 0, junk: 4, temporary: 0, special: 0, unreadable: 3, gitignore: 2, ooignore: 1 });
    expect(next.skippedSamples).toEqual([{ name: "node_modules", reason: "junk" }]);
    // The same rows again are not added twice.
    expect(applyUploadProgress(next, { batchId: "b1", items: [], added: [{ itemId: "x1", name: "a.md", relative: "P/sub/a.md", size: 10, source: "/s/P/sub/a.md" }] }, 2).items).toHaveLength(2);
  });

  it("keeps the list short while the counts stay exact: only the newest finished rows, every failed row and all work in flight", () => {
    let state: UploadQueueState = { ...initialUploadQueueState(), batches: [batchInfo()] };
    let id = 0;
    for (let round = 0; round < 25; round += 1) {
      const rows = Array.from({ length: 1000 }, () => { id += 1; return { itemId: `r${id}`, name: `f${id}.md`, relative: `f${id}.md`, size: 1, source: `/s/f${id}.md` }; });
      state = applyUploadProgress(state, { batchId: "b1", items: [], added: rows, batch: snapshot({ queued: id, found: id }) }, round);
      state = applyUploadProgress(state, { batchId: "b1", items: rows.map((row, index) => ({ itemId: row.itemId, state: index === 0 ? "failed" : "done", bytesDone: 1, bytesTotal: 1 })), batch: snapshot({ queued: id, found: id, counts: { done: id - round - 1, failed: round + 1, skipped: 0, cancelled: 0 } }) }, round);
    }
    expect(state.items.filter((entry) => entry.state === "failed")).toHaveLength(25);
    expect(state.items.filter((entry) => entry.state === "done").length).toBeLessThanOrEqual(SETTLED_ROWS_KEPT);
    expect(state.batches[0]).toMatchObject({ queued: 25_000, found: 25_000 });
    expect(addingOf(state.batches[0]!)).toBe(0);
  });
});

describe("what a drop is doing, for the toast", () => {
  const ids = ["b1"];
  it("is preparing until the walk has finished, then waits for a choice on a big folder, copies, and is done", () => {
    expect(summarizeDrop([], ids).phase).toBe("preparing");
    expect(summarizeDrop([batchInfo({ phase: "scanning", found: 40 })], ids)).toMatchObject({ phase: "preparing", found: 40 });
    const large = [{ path: "/p/Big", name: "Big", files: 25_000, bytes: 5, state: "pending" as const }];
    const choice = summarizeDrop([batchInfo({ phase: "choice", found: 25_000, large })], ids);
    expect(choice).toMatchObject({ phase: "choice", found: 25_000 });
    expect(choice.pending).toEqual([{ ...large[0], batchId: "b1" }]);
    expect(summarizeDrop([batchInfo({ phase: "copying", queued: 10, counts: { done: 4, failed: 0, skipped: 1, cancelled: 0 } })], ids)).toMatchObject({ phase: "copying", queued: 10, settled: 5 });
    expect(summarizeDrop([batchInfo({ phase: "done", queued: 3, counts: { done: 3, failed: 0, skipped: 0, cancelled: 0 }, roots: [{ path: "/p/P", name: "P", kind: "directory", files: 3 }] })], ids))
      .toMatchObject({ phase: "done", done: 3, folders: [{ path: "/p/P", name: "P", files: 3 }] });
    expect(summarizeDrop([batchInfo({ phase: "cancelled" })], ids).phase).toBe("cancelled");
  });

  it("counts skipped items of every reason and carries the batch's problem", () => {
    const summary = summarizeDrop([batchInfo({ phase: "done", skipped: { hidden: 1, links: 0, junk: 4, temporary: 0, special: 0, unreadable: 2, gitignore: 3, ooignore: 1 }, problem: { code: "SO_UPLOAD_NOT_ENOUGH_SPACE", needed: 9, free: 3 } })], ids);
    expect(summary.skipped).toBe(11);
    expect(summary.problem).toEqual({ code: "SO_UPLOAD_NOT_ENOUGH_SPACE", needed: 9, free: 3 });
  });

  it("shows the files found so far as the files being added while the walk goes on", () => {
    expect(addingOf(batchInfo({ phase: "scanning", found: 1200, queued: 300 }))).toBe(1200);
    expect(addingOf(batchInfo({ phase: "copying", queued: 300, counts: { done: 100, failed: 0, skipped: 0, cancelled: 0 } }))).toBe(200);
  });
});

describe("hand-over to the engine (20-frontend §3.3)", () => {
  it("shows waiting, then retains 5 s once the engine lists the file, and offers sync after 30 s", () => {
    const done = item({ state: "done", drivePath: "a.md", finishedAt: 1000 });
    expect(handoffStatus(item({ state: "copying" }), false, 0)).toBe("uploading");
    expect(handoffStatus(done, false, 1000 + 1)).toBe("waiting_engine");
    expect(handoffStatus(done, false, 1000 + HANDOFF_PATIENCE_MS)).toBe("needs_sync");
    expect(handoffStatus(done, true, 1000 + 10)).toBe("retained");
    expect(handoffStatus(done, true, 1000 + DONE_RETAIN_MS)).toBe("handed_over");
    expect(handoffStatus(item({ state: "failed" }), false, 0)).toBe("gone");
  });
});

describe("size limits", () => {
  it("uses 64 MiB for text and 200 MiB otherwise", () => {
    expect(isOverLimit("n.md", 65 * 1024 * 1024)).toBe(true);
    expect(isOverLimit("n.md", 63 * 1024 * 1024)).toBe(false);
    expect(isOverLimit("n.pdf", 100 * 1024 * 1024)).toBe(false);
    expect(isOverLimit("n.pdf", 201 * 1024 * 1024)).toBe(true);
  });
});

describe("upload queue", () => {
  const files = [{ name: "a.md" }, { name: "b.md" }] as unknown as File[];
  const row = (name: string) => ({ itemId: `id-${name}`, name, relative: name, size: 10, source: `/drop/${name}` });

  it("starts a batch from dropped files at once and follows pushes", async () => {
    const { api, push } = createFakeApi();
    const queue = createUploadQueue({ api, now: () => 7 });
    queue.subscribe(() => undefined);
    const outcome = await queue.enqueueFiles("d1", "docs", files);
    expect(api.startUpload).toHaveBeenCalledWith({ driveId: "d1", folder: "docs", paths: ["/drop/a.md", "/drop/b.md"] });
    expect(outcome).toEqual({ batchIds: [BATCH], error: null });
    // Nothing is listed until the main process reports it; the batch is there from the start answer on.
    expect(queue.getState().items).toEqual([]);
    expect(queue.getState().batches).toMatchObject([{ id: BATCH, driveId: "d1", folder: "docs", phase: "scanning", found: 0 }]);
    push({ channel: "upload-progress", payload: { batchId: BATCH, items: [], added: [row("a.md"), row("b.md")], batch: snapshot({ phase: "copying", found: 2, queued: 2 }) } });
    expect(queue.getState().items.map((entry) => [entry.name, entry.state, entry.folder])).toEqual([["a.md", "waiting", "docs"], ["b.md", "waiting", "docs"]]);
    push({ channel: "upload-progress", payload: { batchId: BATCH, items: [{ itemId: "id-a.md", state: "done", bytesDone: 10, bytesTotal: 10, drivePath: "docs/a.md" }] } });
    expect(queue.getState().items[0]).toMatchObject({ state: "done", drivePath: "docs/a.md" });
  });

  it("counts files without a path and reports a failed start", async () => {
    const { api } = createFakeApi({ pathsForFiles: () => ({ paths: [], unreadable: 2 }) });
    const queue = createUploadQueue({ api });
    await queue.enqueueFiles("d1", "", files);
    expect(api.startUpload).not.toHaveBeenCalled();
    expect(queue.getState().skipped.unreadable).toBe(2);
    const failing = createFakeApi({ startUpload: async () => failResult("SO_UPLOAD_UNSUPPORTED_BACKEND") });
    const other = createUploadQueue({ api: failing.api });
    const outcome = await other.enqueueFiles("d1", "", files);
    expect(other.getState().error?.code).toBe("SO_UPLOAD_UNSUPPORTED_BACKEND");
    expect(outcome.error?.code).toBe("SO_UPLOAD_UNSUPPORTED_BACKEND");
  });

  it("cancels one item or all, stops whole drops, retries a failed item into its own folder, and prunes handed-over items", async () => {
    const { api, push } = createFakeApi();
    const queue = createUploadQueue({ api });
    queue.subscribe(() => undefined);
    await queue.enqueueFiles("d1", "docs", files);
    push({ channel: "upload-progress", payload: { batchId: BATCH, items: [], added: [row("a.md"), row("b.md")], batch: snapshot({ queued: 2, found: 2 }) } });
    const [a, b] = queue.getState().items;
    await queue.cancel(a!.id);
    expect(api.cancelUpload).toHaveBeenCalledWith({ batchId: BATCH, itemId: a!.id });
    await queue.cancelAll();
    expect(api.cancelUpload).toHaveBeenLastCalledWith({ batchId: BATCH });
    await queue.cancelBatches([BATCH]);
    expect(api.cancelUpload).toHaveBeenLastCalledWith({ batchId: BATCH });
    push({ channel: "upload-progress", payload: { batchId: BATCH, items: [{ itemId: b!.id, state: "failed", bytesDone: 0, bytesTotal: 10, code: "SO_UPLOAD_DISK_FULL" }] } });
    api.startUpload.mockClear();
    await queue.retry(b!.id);
    expect(api.startUpload).toHaveBeenCalledWith({ driveId: "d1", folder: "docs", paths: [b!.source] });
    queue.prune(() => true);
    expect(queue.getState().items.every((entry) => entry.state !== "cancelled")).toBe(true);
  });

  it("passes the person's answer for big folders on to the main process", async () => {
    const decideUpload = vi.fn(async () => okResult({ copying: 1, skipped: 0 }));
    const { api } = createFakeApi({ decideUpload });
    const queue = createUploadQueue({ api });
    expect(await queue.decide(BATCH, "copy")).toBe(true);
    expect(decideUpload).toHaveBeenCalledWith({ batchId: BATCH, choice: "copy" });
    await queue.decide(BATCH, "skip", ["/p/Big"]);
    expect(decideUpload).toHaveBeenLastCalledWith({ batchId: BATCH, choice: "skip", paths: ["/p/Big"] });
  });
});

describe("drops of any size and mix", () => {
  const startAnswer = (input: { paths: string[] }) => okResult({ batchId: `00000000-0000-4000-8000-0000000000${String(input.paths.length).padStart(2, "0")}`, streaming: true as const });
  const many = (count: number) => Array.from({ length: count }, (_, index) => ({ name: `f${index}.md` })) as unknown as File[];

  it("splits a drop into requests of at most 1,000 paths, one after the other (each answers at once)", async () => {
    const { api } = createFakeApi({ startUpload: async (input: { paths: string[] }) => startAnswer(input) });
    const queue = createUploadQueue({ api });
    const outcome = await queue.enqueueFiles("d1", "", many(2300));
    expect((api.startUpload as ReturnType<typeof vi.fn>).mock.calls.map(([call]) => call.paths.length)).toEqual([1000, 1000, 300]);
    expect(outcome.error).toBeNull();
    expect(outcome.batchIds).toHaveLength(3);
    expect(queue.getState().batches).toHaveLength(3);
  });

  it("starts nothing for a path that is not absolute, a repeat, or a path inside a dropped folder", async () => {
    const { api } = createFakeApi({ pathsForFiles: () => ({ paths: ["/a/P", "/a/P/x.md", "/a/P", "rel.md", "/a/q.md"], unreadable: 1 }), startUpload: async (input: { paths: string[] }) => startAnswer(input) });
    const queue = createUploadQueue({ api });
    await queue.enqueueFiles("d1", "", many(5));
    expect(api.startUpload).toHaveBeenCalledWith({ driveId: "d1", folder: "", paths: ["/a/P", "/a/q.md"] });
    expect(queue.getState().skipped.unreadable).toBe(2);
  });

  it("stops at the first failed request and reports its error", async () => {
    let calls = 0;
    const { api } = createFakeApi({ startUpload: async (input: { paths: string[] }) => { calls += 1; return calls === 2 ? failResult("SO_UPLOAD_UNSUPPORTED_BACKEND") : startAnswer(input); } });
    const queue = createUploadQueue({ api });
    const outcome = await queue.enqueueFiles("d1", "", many(2500));
    expect(calls).toBe(2);
    expect(outcome.error?.code).toBe("SO_UPLOAD_UNSUPPORTED_BACKEND");
    expect(outcome.batchIds).toHaveLength(1);
  });

  it("applies a push that arrived before its start answer, in order", async () => {
    const fake = createFakeApi({ startUpload: async () => {
      // The main process pushed while its answer was still on the way.
      fake.push({ channel: "upload-progress", payload: { batchId: BATCH, items: [], added: [{ itemId: "n1", name: "n.md", relative: "n.md", size: 5, source: "/drop/n.md" }], batch: snapshot({ queued: 1, found: 1 }) } });
      fake.push({ channel: "upload-progress", payload: { batchId: BATCH, items: [{ itemId: "n1", state: "copying", bytesDone: 2, bytesTotal: 5 }] } });
      fake.push({ channel: "upload-progress", payload: { batchId: BATCH, items: [{ itemId: "n1", state: "done", bytesDone: 5, bytesTotal: 5, drivePath: "n.md" }] } });
      return okResult({ batchId: BATCH, streaming: true as const });
    } });
    const queue = createUploadQueue({ api: fake.api, now: () => 9 });
    queue.subscribe(() => undefined);
    await queue.enqueueFiles("d1", "", [{ name: "n.md" }] as unknown as File[]);
    expect(queue.getState().items[0]).toMatchObject({ state: "done", drivePath: "n.md", finishedAt: 9 });
  });

  it("shows a file that was already there as skipped, finished, with its reason", () => {
    const state = stateWith([item()]);
    const next = applyUploadProgress(state, progress("skipped", { skipReason: "duplicate", bytesDone: 100, drivePath: "a.md" }), 4);
    expect(next.items[0]).toMatchObject({ state: "skipped", skipReason: "duplicate", finishedAt: 4, drivePath: "a.md" });
    expect(summarizeUploads(next)).toMatchObject({ active: 0, failed: 0 });
    expect(handoffStatus(next.items[0]!, false, 99)).toBe("gone");
  });

  it("takes back the copies of a dropped folder through the batches that made them", async () => {
    const discardUpload = vi.fn(async ({ batchId }: { batchId: string }) => okResult({ removed: batchId.endsWith("1") ? 2 : 3 }));
    const { api } = createFakeApi({ discardUpload });
    const queue = createUploadQueue({ api });
    expect(await queue.discard(["x1", "x2"], "/a/P")).toBe(5);
    expect(discardUpload).toHaveBeenCalledWith({ batchId: "x1", folderPath: "/a/P" });
  });

  it("closing the list forgets what finished and what was skipped but keeps work that is running", () => {
    const { api } = createFakeApi();
    const queue = createUploadQueue({ api });
    expect(queue.getState().dismissed).toBe(false);
    queue.dismiss();
    expect(queue.getState()).toMatchObject({ dismissed: true, items: [], skipped: { hidden: 0 } });
  });
});
