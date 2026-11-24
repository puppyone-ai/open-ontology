import { mkdir, mkdtemp, readdir, rm, truncate, utimes, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createUploadService, LARGE_FOLDER_FILES, SPACE_RESERVE_BYTES } from "@so-main/upload-service.mjs";
import { createSyntheticTree } from "../../support/synthetic-tree.mjs";

// The walk streams (answers at once, reports as it goes), has no cap, pauses for a choice on big folders, applies the engine's ignore rules,
// and bounds what it keeps in memory.
const DRIVE_ID = "00000000-0000-4000-8000-000000000001";
let work; let root; let source; let pushes; let requests;

let services = [];
function make({ fsPromises, now } = {}) {
  requests = [];
  const client = { request: vi.fn(async (name, options) => { requests.push([name, options]); return name === "driveGet" ? { value: { id: DRIVE_ID, backend: "fs", config: { root } } } : { value: { status: "queued" } }; }) };
  const service = createUploadService({ client, broadcast: (channel, payload) => pushes.push(payload), ...(fsPromises ? { fsPromises } : {}), ...(now ? { now } : {}) });
  services.push(service);
  return service;
}
const snapshots = (batchId) => pushes.filter((push) => push.batchId === batchId).map((push) => push.batch);
const until = async (check, what, ms = 60_000) => {
  const limit = Date.now() + ms;
  for (;;) { const value = check(); if (value) return value; if (Date.now() > limit) throw new Error(`timed out: ${what}`); await new Promise((resolve) => { setTimeout(resolve, 10); }); }
};
const phase = (batchId) => snapshots(batchId).at(-1)?.phase;
const put = async (file, text = "x") => { await mkdir(path.dirname(file), { recursive: true }); await writeFile(file, text); };
const many = async (directory, count, prefix = "f") => {
  await mkdir(directory, { recursive: true });
  for (let from = 0; from < count; from += 250) await Promise.all(Array.from({ length: Math.min(250, count - from) }, (_, index) => writeFile(path.join(directory, `${prefix}${from + index}.md`), `n${from + index}`)));
};
const countFiles = async (directory) => (await readdir(directory, { recursive: true, withFileTypes: true })).filter((entry) => entry.isFile()).length;

beforeEach(async () => {
  work = await mkdtemp(path.join(os.tmpdir(), "so-stream-"));
  root = path.join(work, "drive"); source = path.join(work, "source");
  await mkdir(root); await mkdir(source);
  pushes = [];
});
afterEach(async () => {
  for (const service of services) service.dispose();
  services = [];
  await new Promise((resolve) => { setTimeout(resolve, 80); });
  await rm(work, { recursive: true, force: true });
});

describe("a drop answers at once and reports while it walks", () => {
  it("returns the batch id before the walk has found anything, then reports the files found so far", async () => {
    for (let dir = 0; dir < 60; dir += 1) await many(path.join(source, "P", `d${dir}`), 25);
    let tick = 0;
    const service = make({ now: () => { tick += 150; return tick; } });
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "P")] });
    expect(started).toEqual({ batchId: expect.any(String), streaming: true });
    expect(pushes[0].batch).toMatchObject({ phase: "scanning", found: 0 });
    await service.scanned(started.batchId);
    const found = snapshots(started.batchId).map((entry) => entry.found);
    expect(found).toEqual([...found].sort((a, b) => a - b));
    expect(found.at(-1)).toBe(1500);
    expect(found.filter((count, index) => index > 0 && count > found[index - 1]).length).toBeGreaterThan(1);
    await until(() => phase(started.batchId) === "done", "the small drop copies by itself");
    expect(await countFiles(root)).toBe(1500);
  });

  it("releases the rows of a small folder as pushes with `added`, and the copy states after them", async () => {
    await many(path.join(source, "S"), 30);
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "S")] });
    await until(() => phase(started.batchId) === "done", "done");
    const added = pushes.flatMap((push) => push.added ?? []);
    expect(added).toHaveLength(30);
    expect(added[0]).toMatchObject({ itemId: expect.any(String), name: expect.any(String), relative: expect.stringMatching(/^S\//u), size: expect.any(Number) });
    expect(snapshots(started.batchId).at(-1)).toMatchObject({ phase: "done", found: 30, queued: 30, counts: { done: 30, failed: 0 } });
  });
});

describe("there is no cap", () => {
  it("walks more than the old limit of 10,000 files without refusing anything, and waits for a choice instead of copying", async () => {
    await many(path.join(source, "Huge"), 10_300);
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "Huge")] });
    const plan = await service.scanned(started.batchId);
    expect(plan.large).toMatchObject([{ name: "Huge", files: 10_300, state: "pending" }]);
    expect(plan.items).toEqual([]);
    expect(snapshots(started.batchId).at(-1)).toMatchObject({ phase: "choice", found: 10_300, queued: 0 });
    expect(snapshots(started.batchId).at(-1).error).toBeUndefined();
    expect(await readdir(root)).toEqual([]);
    expect(requests.some(([name]) => name === "driveSync")).toBe(false);
  }, 60_000);

  it("copies a big folder as a stream when told to, with a bounded queue and exact counts", async () => {
    await many(path.join(source, "Big"), LARGE_FOLDER_FILES + 300);
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "Big")] });
    await service.scanned(started.batchId);
    expect(service.decide({ batchId: started.batchId, choice: "copy" })).toEqual({ copying: 1, skipped: 0 });
    await until(() => phase(started.batchId) === "done", "streamed copy", 120_000);
    const total = LARGE_FOLDER_FILES + 300;
    expect(snapshots(started.batchId).at(-1)).toMatchObject({ queued: total, counts: { done: total, failed: 0 } });
    // The walk never ran far ahead of the copy: what was queued and not yet settled stayed within the bounded queue and the workers.
    const ahead = snapshots(started.batchId).map((entry) => entry.queued - entry.counts.done - entry.counts.failed - entry.counts.skipped - entry.counts.cancelled);
    expect(Math.max(...ahead)).toBeLessThanOrEqual(1000 + 64);
    expect(await countFiles(root)).toBe(total);
    expect(requests.filter(([name]) => name === "driveSync").length).toBeGreaterThanOrEqual(1);
  }, 180_000);

  it("leaves a big folder alone when told to skip it (the person links it instead)", async () => {
    await many(path.join(source, "Big"), LARGE_FOLDER_FILES + 5);
    await put(path.join(source, "loose.md"));
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "Big"), path.join(source, "loose.md")] });
    await service.scanned(started.batchId);
    expect(service.decide({ batchId: started.batchId, choice: "skip", paths: [path.join(source, "Big")] })).toEqual({ copying: 0, skipped: 1 });
    await until(() => phase(started.batchId) === "done", "done");
    expect(await readdir(root)).toEqual(["loose.md"]);
    expect(snapshots(started.batchId).at(-1).large).toMatchObject([{ name: "Big", state: "skipped" }]);
  }, 60_000);

  it("counts bytes too: one folder over 1 GiB is a big folder even with few files", async () => {
    await put(path.join(source, "Videos", "a.md"));
    await truncate(path.join(source, "Videos", "movie.bin"), 0).catch(async () => { await writeFile(path.join(source, "Videos", "movie.bin"), ""); });
    await truncate(path.join(source, "Videos", "movie.bin"), 1024 ** 3 + 1);
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "Videos")] });
    const plan = await service.scanned(started.batchId);
    expect(plan.large).toMatchObject([{ name: "Videos", files: 2, state: "pending" }]);
    expect(await readdir(root)).toEqual([]);
  });

  it("a folder at the threshold still copies without asking, and a dropped folder called build is not skipped for its name", async () => {
    await many(path.join(source, "build"), LARGE_FOLDER_FILES);
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "build")] });
    await service.scanned(started.batchId);
    expect(snapshots(started.batchId).at(-1).large).toEqual([]);
    await until(() => phase(started.batchId) === "done", "done", 120_000);
    expect(await countFiles(path.join(root, "build"))).toBe(LARGE_FOLDER_FILES);
  }, 180_000);
});

describe("ignore rules while walking", () => {
  it("skips code project folders, .gitignore and .ooignore, and reports each by reason", async () => {
    const project = path.join(source, "proj");
    await put(path.join(project, "src", "main.ts"));
    await put(path.join(project, "src", "gen", "api.ts"));
    await put(path.join(project, "src", "old.pyc"));
    await put(path.join(project, "target", "debug", "app"));
    await put(path.join(project, "dist", "bundle.js"));
    await put(path.join(project, "build", "out.txt"));
    await put(path.join(project, "venv", "lib.py"));
    await put(path.join(project, "node_modules", "x", "i.js"));
    await put(path.join(project, ".venv", "lib.py"));
    await put(path.join(project, "logs", "run.log"));
    await put(path.join(project, "logs", "keep.md"));
    await put(path.join(project, "secrets", "key.txt"));
    await put(path.join(project, "README.md"));
    await put(path.join(project, ".gitignore"), "src/gen/\n*.log\n");
    await put(path.join(project, ".ooignore"), "secrets/\n");
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [project] });
    const plan = await service.scanned(started.batchId);
    expect(plan.items.map((item) => item.relative).sort()).toEqual(["proj/README.md", "proj/logs/keep.md", "proj/src/main.ts"]);
    // hidden: .venv, .gitignore, .ooignore; junk: node_modules, target, dist, build, venv, old.pyc
    expect(plan.skipped).toEqual({ hidden: 3, links: 0, junk: 6, temporary: 0, special: 0, gitignore: 2, ooignore: 1 });
    expect(plan.skippedSamples.length).toBeLessThanOrEqual(12);
    await until(() => phase(started.batchId) === "done", "done");
    expect(await countFiles(root)).toBe(3);
  });

  it("a nested .gitignore only reaches its own folder", async () => {
    await put(path.join(source, "p", "a", ".gitignore"), "*.tmp2\n");
    await put(path.join(source, "p", "a", "x.tmp2"));
    await put(path.join(source, "p", "b", "x.tmp2"));
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "p")] });
    expect((await service.scanned(started.batchId)).items.map((item) => item.relative)).toEqual(["p/b/x.tmp2"]);
    await until(() => phase(started.batchId) === "done", "done");
  });

  it("handles a synthetic code tree: counts what the rules keep and what they skip, and asks before copying (the 25,000 file run is the T1 drop-large)", async () => {
    const tree = path.join(work, "synthetic");
    const made = await createSyntheticTree(tree, { kept: 2_500, litter: 100 });
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [tree] });
    await service.scanned(started.batchId);
    const last = snapshots(started.batchId).at(-1);
    expect(last).toMatchObject({ phase: "choice", found: made.kept });
    expect(last.error).toBeUndefined();
    expect(last.skipped).toMatchObject(made.skipped);
  }, 120_000);
});

describe("duplicates are found by size and time before bytes", () => {
  it("takes the same size and time as the same file without reading the bytes, and hashes when the time differs", async () => {
    await put(path.join(source, "a.md"), "aaaa");
    await put(path.join(source, "b.md"), "bbbb");
    const service = make();
    const first = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "a.md"), path.join(source, "b.md")] });
    await until(() => phase(first.batchId) === "done", "first copy");
    // The copy kept the modification time: the second drop is a duplicate by size and time alone. Different bytes with the same size and
    // time are therefore taken for the same file (which is what proves the bytes were not read).
    await writeFile(path.join(root, "a.md"), "XXXX");
    const sourceTime = new Date((await import("node:fs/promises")).stat(path.join(source, "a.md")).then((stat) => stat.mtime) && Date.now());
    await utimes(path.join(source, "a.md"), sourceTime, sourceTime);
    await utimes(path.join(root, "a.md"), sourceTime, sourceTime);
    await utimes(path.join(root, "b.md"), new Date("2020-01-01"), new Date("2020-01-01"));
    pushes = [];
    const second = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "a.md"), path.join(source, "b.md")] });
    await until(() => phase(second.batchId) === "done", "second drop");
    expect(snapshots(second.batchId).at(-1).counts).toMatchObject({ skipped: 2, done: 0 });
    expect(await readdir(root)).toEqual(["a.md", "b.md"]);
  });
});

describe("the one real limit: disk space", () => {
  it("refuses a drop that needs more room than the disk has, per file, with a clear code and the numbers", async () => {
    await many(path.join(source, "P"), 5);
    const realFs = await import("node:fs/promises");
    const small = { ...realFs, statfs: async () => ({ bavail: 100, bsize: 1 }) };
    const service = make({ fsPromises: small });
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "P")] });
    await until(() => phase(started.batchId) === "done", "done");
    const last = snapshots(started.batchId).at(-1);
    expect(last.error).toMatchObject({ code: "SO_UPLOAD_NOT_ENOUGH_SPACE", free: 100 });
    expect(last.error.needed).toBeGreaterThan(0);
    expect(last.counts).toMatchObject({ failed: 5, done: 0 });
    expect(pushes.flatMap((push) => push.items).filter((item) => item.code === "SO_UPLOAD_NOT_ENOUGH_SPACE").length).toBeGreaterThanOrEqual(5);
    expect(await readdir(root)).toEqual([]);
    expect(SPACE_RESERVE_BYTES).toBeGreaterThan(0);
  });

  it("does not refuse when the free space is unknown or enough", async () => {
    await many(path.join(source, "P"), 3);
    const realFs = await import("node:fs/promises");
    const unknown = { ...realFs, statfs: async () => { throw new Error("no statfs here"); } };
    const started = await make({ fsPromises: unknown }).start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "P")] });
    await until(() => phase(started.batchId) === "done", "done");
    expect(snapshots(started.batchId).at(-1).counts).toMatchObject({ done: 3, failed: 0 });
  });
});

describe("cancelling", () => {
  it("stops a walk and a pending choice", async () => {
    await many(path.join(source, "Big"), LARGE_FOLDER_FILES + 10);
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "Big")] });
    await service.scanned(started.batchId);
    service.cancel({ batchId: started.batchId });
    await until(() => phase(started.batchId) === "cancelled", "cancelled");
    expect(await readdir(root)).toEqual([]);
  }, 60_000);
});
