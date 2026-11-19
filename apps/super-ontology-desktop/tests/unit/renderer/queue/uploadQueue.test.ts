import { describe, expect, it, vi } from "vitest";
import {
  applyUploadProgress, createUploadQueue, DONE_RETAIN_MS, handoffStatus, HANDOFF_PATIENCE_MS, initialUploadQueueState, isOverLimit,
  summarizeUploads, type UploadItem,
} from "@so-model/queue/uploadQueue";
import { createFakeApi, okResult, failResult } from "../../../support/fakeApi";

const item = (overrides: Partial<UploadItem> = {}): UploadItem => ({
  id: "i1", batchId: "b1", driveId: "d1", name: "a.md", relative: "a.md", folder: "", source: "/drop/a.md", size: 100, state: "waiting",
  bytesDone: 0, code: null, drivePath: null, finishedAt: null, overLimit: false, ...overrides,
});
const progress = (state: string, extra: Record<string, unknown> = {}) => ({ batchId: "b1", items: [{ itemId: "i1", state, bytesDone: 50, bytesTotal: 100, ...extra }] });

describe("upload progress folding", () => {
  it("moves an item through copying to done and records the final path and the time", () => {
    const start = { ...initialUploadQueueState(), items: [item()] };
    const copying = applyUploadProgress(start, progress("copying"), 10);
    expect(copying.items[0]).toMatchObject({ state: "copying", bytesDone: 50, finishedAt: null });
    const done = applyUploadProgress(copying, progress("done", { drivePath: "docs/a (2).md", bytesDone: 100 }), 20);
    expect(done.items[0]).toMatchObject({ state: "done", drivePath: "docs/a (2).md", finishedAt: 20 });
    expect(applyUploadProgress(done, progress("done", { drivePath: "docs/a (2).md", bytesDone: 100 }), 99)).toBe(done);
  });

  it("ignores unknown items and reopens a dismissed panel on a failure", () => {
    const state = { ...initialUploadQueueState(), items: [item()], dismissed: true };
    expect(applyUploadProgress(state, { batchId: "b1", items: [{ itemId: "zzz", state: "done", bytesDone: 1, bytesTotal: 1 }] }, 1)).toBe(state);
    const failed = applyUploadProgress(state, progress("failed", { code: "SO_UPLOAD_DISK_FULL" }), 5);
    expect(failed.items[0]).toMatchObject({ state: "failed", code: "SO_UPLOAD_DISK_FULL" });
    expect(failed.dismissed).toBe(false);
    expect(summarizeUploads(failed)).toMatchObject({ active: 0, failed: 1 });
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
  it("starts a batch from dropped files and follows pushes", async () => {
    const { api, push } = createFakeApi();
    const queue = createUploadQueue({ api, now: () => 7 });
    queue.subscribe(() => undefined);
    await queue.enqueueFiles("d1", "docs", files);
    expect(api.startUpload).toHaveBeenCalledWith({ driveId: "d1", folder: "docs", paths: ["/drop/a.md", "/drop/b.md"] });
    expect(queue.getState().items.map((entry) => [entry.name, entry.state, entry.folder])).toEqual([["a.md", "waiting", "docs"], ["b.md", "waiting", "docs"]]);
    const first = queue.getState().items[0];
    push({ channel: "upload-progress", payload: { batchId: first.batchId, items: [{ itemId: first.id, state: "done", bytesDone: 10, bytesTotal: 10, drivePath: "docs/a.md" }] } });
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
    await other.enqueueFiles("d1", "", files);
    expect(other.getState().error?.code).toBe("SO_UPLOAD_UNSUPPORTED_BACKEND");
  });

  it("cancels one item or all, retries a failed item into its own folder, and prunes handed-over items", async () => {
    const { api, push } = createFakeApi();
    const queue = createUploadQueue({ api });
    queue.subscribe(() => undefined);
    await queue.enqueueFiles("d1", "docs", files);
    const [a, b] = queue.getState().items;
    await queue.cancel(a.id);
    expect(api.cancelUpload).toHaveBeenCalledWith({ batchId: a.batchId, itemId: a.id });
    await queue.cancelAll();
    expect(api.cancelUpload).toHaveBeenLastCalledWith({ batchId: a.batchId });
    push({ channel: "upload-progress", payload: { batchId: a.batchId, items: [{ itemId: b.id, state: "failed", bytesDone: 0, bytesTotal: 10, code: "SO_UPLOAD_DISK_FULL" }] } });
    api.startUpload.mockClear();
    await queue.retry(b.id);
    expect(api.startUpload).toHaveBeenCalledWith({ driveId: "d1", folder: "docs", paths: [b.source] });
    queue.prune(() => true);
    expect(queue.getState().items.every((entry) => entry.state !== "cancelled")).toBe(true);
    void okResult; void vi;
  });
});
