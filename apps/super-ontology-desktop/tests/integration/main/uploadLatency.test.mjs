import fs from "node:fs/promises";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createUploadService } from "@so-main/upload-service.mjs";

// A drop must answer at once and show rows quickly even when the disk is slow (a cold cache, a network or iCloud folder: every stat costs a
// round trip). The old walk awaited one stat per file before the upload request answered, so a 1,600 file folder at 1 ms a stat took over two
// seconds to show anything and ten thousand files took ten.
const DRIVE_ID = "00000000-0000-4000-8000-000000000001";
let work; let root; let source; let pushes;

const sleep = (ms) => new Promise((resolve) => { setTimeout(resolve, ms); });
/** `fs/promises` with a fixed delay on every stat and directory read: a disk with a cold cache. */
const slowDisk = (ms) => ({
  ...fs,
  lstat: async (...args) => { await sleep(ms); return fs.lstat(...args); },
  stat: async (...args) => { await sleep(ms); return fs.stat(...args); },
  readdir: async (...args) => { await sleep(ms); return fs.readdir(...args); },
});

beforeEach(async () => {
  work = await mkdtemp(path.join(os.tmpdir(), "so-latency-"));
  root = path.join(work, "drive"); source = path.join(work, "source");
  await mkdir(root); await mkdir(source);
  pushes = [];
});
afterEach(async () => { await sleep(150); await rm(work, { recursive: true, force: true }); });

describe("a drop on a slow disk", () => {
  it("answers the request at once and shows the first rows long before an old-style serial walk could have", async () => {
    for (let dir = 0; dir < 40; dir += 1) {
      await mkdir(path.join(source, "P", `d${dir}`), { recursive: true });
      await Promise.all(Array.from({ length: 40 }, (_, file) => writeFile(path.join(source, "P", `d${dir}`, `f${file}.md`), "x")));
    }
    const client = { request: vi.fn(async (name) => (name === "driveGet" ? { value: { id: DRIVE_ID, backend: "fs", config: { root } } } : { value: { status: "queued" } })) };
    const service = createUploadService({ client, broadcast: (channel, payload) => pushes.push({ at: performance.now(), payload }), fsPromises: slowDisk(1) });
    const t0 = performance.now();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "P")] });
    const answered = performance.now() - t0;
    expect(started).toEqual({ batchId: expect.any(String), streaming: true });
    expect(answered).toBeLessThan(150);
    await service.scanned(started.batchId);
    const firstRows = pushes.find((entry) => (entry.payload.added ?? []).length > 0);
    const firstRowAfter = firstRows.at - t0;
    // 1,600 files at 1 ms a stat in a row is 1.6 s and more; the files of a folder are stat-ed 64 at a time, so 40 folders take about 0.2 s.
    expect(firstRowAfter).toBeLessThan(1_200);
    expect(firstRows.payload.added).toHaveLength(1600);
    // The count of files found starts moving as soon as the first folder has been read, not after the first throttled interval.
    const firstCount = pushes.find((entry) => (entry.payload.batch?.found ?? 0) > 0);
    expect(firstCount.at - t0).toBeLessThan(90);
    service.dispose();
  }, 30_000);
});
