import { chmod, mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createUploadService, isIgnoredName, numberedName } from "@so-main/upload-service.mjs";
import { parseSuperOntologyRequest } from "@so-shared/requests.mjs";

const DRIVE_ID = "00000000-0000-4000-8000-000000000001";
let work;
let root;
let source;
let pushes;
let requests;

function make({ backend = "fs", fsPromises } = {}) {
  requests = [];
  const client = {
    request: vi.fn(async (name, options) => {
      requests.push([name, options]);
      if (name === "driveGet") return { value: { id: DRIVE_ID, backend, config: { root } } };
      return { value: { status: "queued" } };
    }),
  };
  return createUploadService({ client, broadcast: (channel, payload) => pushes.push([channel, payload]), ...(fsPromises ? { fsPromises } : {}) });
}
const lastProgress = () => pushes.filter(([channel]) => channel === "upload-progress").at(-1)?.[1];
/** Pushes carry what changed, so a batch's state is the latest word on each of its items; settled when all of them are final (and the sync was asked for). */
async function settled(started) {
  const final = ["done", "failed", "cancelled", "skipped"];
  for (let attempt = 0; attempt < 300; attempt += 1) {
    const latest = new Map();
    for (const [channel, payload] of pushes) if (channel === "upload-progress" && payload.batchId === started.batchId) for (const item of payload.items) latest.set(item.itemId, item);
    const items = [...latest.values()];
    const syncOwed = items.some((item) => item.state === "done");
    if (items.length === started.items.length && items.every((item) => final.includes(item.state)) && (!syncOwed || requests.some(([name]) => name === "driveSync"))) return { batchId: started.batchId, items };
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error("upload did not settle");
}
const listing = async (directory) => (await readdir(directory)).sort();

beforeEach(async () => {
  work = await mkdtemp(path.join(os.tmpdir(), "so-upload-"));
  root = path.join(work, "drive");
  source = path.join(work, "source");
  await mkdir(root);
  await mkdir(source);
  pushes = [];
});
afterEach(async () => { await rm(work, { recursive: true, force: true }); });

describe("upload service", () => {
  it("copies into the folder through a temporary name and syncs once", async () => {
    await writeFile(path.join(source, "note.md"), "hello");
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "docs/sub", paths: [path.join(source, "note.md")] });
    expect(started.items).toMatchObject([{ name: "note.md", size: 5 }]);
    const progress = await settled(started);
    expect(progress.items[0]).toMatchObject({ state: "done", bytesDone: 5, drivePath: "docs/sub/note.md" });
    expect(await readFile(path.join(root, "docs/sub/note.md"), "utf8")).toBe("hello");
    expect(await listing(path.join(root, "docs/sub"))).toEqual(["note.md"]);
    expect(requests.filter(([name]) => name === "driveSync")).toHaveLength(1);
  });

  it("renames a clash to 'name (2).ext' and never overwrites", async () => {
    await writeFile(path.join(root, "a.txt"), "old");
    await writeFile(path.join(root, "a (2).txt"), "old2");
    await writeFile(path.join(source, "a.txt"), "new");
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "a.txt")] });
    const progress = await settled(started);
    expect(progress.items[0].drivePath).toBe("a (3).txt");
    expect(await readFile(path.join(root, "a.txt"), "utf8")).toBe("old");
    expect(await readFile(path.join(root, "a (3).txt"), "utf8")).toBe("new");
    expect(numberedName("archive.tar.gz", 2)).toBe("archive.tar (2).gz");
    expect(numberedName(".env", 2)).toBe(".env (2)");
  });

  it("keeps the folder structure and skips hidden, partial and linked entries", async () => {
    const tree = path.join(source, "Project");
    await mkdir(path.join(tree, "inner"), { recursive: true });
    await writeFile(path.join(tree, "a.md"), "a");
    await writeFile(path.join(tree, "inner", "b.md"), "b");
    await writeFile(path.join(tree, ".hidden"), "h");
    await writeFile(path.join(tree, "x.part"), "p");
    await writeFile(path.join(tree, "~$lock.docx"), "l");
    await symlink(path.join(tree, "a.md"), path.join(tree, "link.md"));
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "in", paths: [tree] });
    expect(started.skipped).toEqual({ hidden: 1, links: 1, junk: 0, temporary: 2, special: 0 });
    await settled(started);
    expect(await listing(path.join(root, "in/Project"))).toEqual(["a.md", "inner"]);
    expect(await readFile(path.join(root, "in/Project/inner/b.md"), "utf8")).toBe("b");
  });

  it("refuses non-fs drives, traversal and unreadable sources", async () => {
    await expect(make({ backend: "s3" }).start({ driveId: DRIVE_ID, folder: "", paths: [] })).rejects.toMatchObject({ code: "SO_UPLOAD_UNSUPPORTED_BACKEND" });
    expect(() => parseSuperOntologyRequest("upload-start", { driveId: DRIVE_ID, folder: "../out", paths: [] })).toThrow();
    expect(() => parseSuperOntologyRequest("upload-start", { driveId: DRIVE_ID, folder: "a//b", paths: [] })).toThrow();
    const started = await make().start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "missing.txt"), "relative.txt"] });
    expect(started).toMatchObject({ items: [], unreadable: 2 });
  });

  it("does not let a symlinked folder lead out of the Drive", async () => {
    const outside = path.join(work, "outside");
    await mkdir(outside);
    await symlink(outside, path.join(root, "escape"));
    await writeFile(path.join(source, "n.txt"), "n");
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "escape", paths: [path.join(source, "n.txt")] });
    const progress = await settled(started);
    expect(progress.items[0]).toMatchObject({ state: "failed", code: "SO_INVALID_REQUEST" });
    expect(await listing(outside)).toEqual([]);
  });

  it("reports a full disk and a permission error per item and keeps going", async () => {
    await writeFile(path.join(source, "a.txt"), "a");
    await writeFile(path.join(source, "b.txt"), "b");
    const realFs = await import("node:fs/promises");
    let calls = 0;
    const fsPromises = { ...realFs, link: async (...args) => {
      calls += 1;
      if (calls === 1) throw Object.assign(new Error("no space"), { code: "ENOSPC" });
      return realFs.link(...args);
    } };
    const service = make({ fsPromises });
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "a.txt"), path.join(source, "b.txt")] });
    const progress = await settled(started);
    const byState = Object.fromEntries(progress.items.map((item) => [item.state, item]));
    expect(byState.failed.code).toBe("SO_UPLOAD_DISK_FULL");
    expect(byState.done.drivePath).toMatch(/\.txt$/u);
    expect((await listing(root)).filter((name) => name.endsWith(".part"))).toEqual([]);
    // A name that is not in the Drive yet: one already there with the same bytes would be skipped, not refused.
    await writeFile(path.join(source, "c.txt"), "c");
    await chmod(root, 0o500);
    try {
      const denied = await make().start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "c.txt")] });
      const result = await settled(denied);
      expect(result.items[0]).toMatchObject({ state: "failed" });
      expect(["SO_UPLOAD_PERMISSION_DENIED", "SO_UPLOAD_FAILED"]).toContain(result.items[0].code);
    } finally { await chmod(root, 0o700); }
  });

  it("cancels a waiting item and leaves no temporary file", async () => {
    await writeFile(path.join(source, "a.txt"), "a");
    await writeFile(path.join(source, "b.txt"), "b");
    await writeFile(path.join(source, "c.txt"), "c");
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: ["a.txt", "b.txt", "c.txt"].map((name) => path.join(source, name)) });
    service.cancel({ batchId: started.batchId });
    await new Promise((resolve) => setTimeout(resolve, 200));
    const progress = lastProgress();
    expect(progress.items.every((item) => ["done", "cancelled"].includes(item.state))).toBe(true);
    expect((await listing(root)).filter((name) => name.endsWith(".part"))).toEqual([]);
    expect(() => service.cancel({ batchId: "00000000-0000-4000-8000-0000000000ff" })).toThrow();
  });

  it("recognises the ignored names", () => {
    for (const name of [".DS_Store", "a.part", "a.TMP", "~$x.docx", "Thumbs.db", "x.crdownload"]) expect(isIgnoredName(name), name).toBe(true);
    for (const name of ["a.md", "report.partial.pdf", "tmpfile"]) expect(isIgnoredName(name), name).toBe(false);
  });
});
