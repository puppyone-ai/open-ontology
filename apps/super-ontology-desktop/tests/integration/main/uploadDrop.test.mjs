import { chmod, mkdir, mkdtemp, readFile, readdir, rm, stat, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createUploadService, ignoreReason } from "@so-main/upload-service.mjs";

// Folder walking and the rules of a drop (25 §2): any mix of files and folders, the engine's ignore rules, links, empty folders, repeats.
const DRIVE_ID = "00000000-0000-4000-8000-000000000001";
let work; let root; let source; let pushes; let requests;

function make() {
  requests = [];
  const client = { request: vi.fn(async (name, options) => { requests.push([name, options]); return name === "driveGet" ? { value: { id: DRIVE_ID, backend: "fs", config: { root } } } : { value: { status: "queued" } }; }) };
  const service = createUploadService({ client, broadcast: (channel, payload) => pushes.push([channel, payload]) });
  // `start` answers at once and the walk streams; most of these tests read the finished plan the way the old synchronous answer gave it.
  return { ...service, raw: service, start: async (input) => { const started = await service.start(input); return { ...started, ...(await service.scanned(started.batchId)) }; } };
}
const progresses = () => pushes.filter(([channel]) => channel === "upload-progress").map(([, payload]) => payload);
/** The last snapshot that lists every item, once all are settled and the sync was asked for (or nothing was copied). */
async function settled(started) {
  for (let attempt = 0; attempt < 600; attempt += 1) {
    const last = progresses().filter((entry) => entry.batchId === started.batchId).at(-1);
    if (last && last.items.length === started.items.length && last.items.every((item) => ["done", "failed", "cancelled", "skipped"].includes(item.state))) return last;
    if (started.items.length === 0) return { items: [] };
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error("drop did not settle");
}
const tree = async (directory, base = directory) => {
  const out = [];
  for (const entry of (await readdir(directory, { withFileTypes: true })).sort((a, b) => (a.name < b.name ? -1 : 1))) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) { const inner = await tree(full, base); out.push(inner.length === 0 ? `${path.relative(base, full)}/` : null, ...inner); } else out.push(path.relative(base, full));
  }
  return out.filter(Boolean);
};
const put = async (file, text = "x") => { await mkdir(path.dirname(file), { recursive: true }); await writeFile(file, text); };

beforeEach(async () => {
  work = await mkdtemp(path.join(os.tmpdir(), "so-drop-"));
  root = path.join(work, "drive"); source = path.join(work, "source");
  await mkdir(root); await mkdir(source);
  pushes = [];
});
afterEach(async () => { await chmod(path.join(source, "locked"), 0o700).catch(() => {}); await rm(work, { recursive: true, force: true }); });

describe("ignore reasons mirror the engine's path rules (10 §4.3.2)", () => {
  it("names hidden, temporary and junk names, and nothing else", () => {
    expect(ignoreReason(".DS_Store")).toBe("hidden");
    expect(ignoreReason(".git")).toBe("hidden");
    expect(ignoreReason(".~lock.report.odt#")).toBe("hidden");
    for (const name of ["~$deck.pptx", "a.part", "b.TMP", "c.swp", "d.crdownload"]) expect(ignoreReason(name), name).toBe("temporary");
    for (const name of ["node_modules", "__pycache__", "Thumbs.db", "DESKTOP.INI"]) expect(ignoreReason(name), name).toBe("junk");
    for (const name of ["a.md", "node_modules.md", "my_node_modules", "report.partial.pdf", "tmp", "lockfile.md"]) expect(ignoreReason(name), name).toBeNull();
  });
});

describe("walking a mixed drop", () => {
  it("keeps each folder's structure, copies loose files beside them and reports every skip by reason", async () => {
    await put(path.join(source, "ProjectA", "readme.md"));
    await put(path.join(source, "ProjectA", "notes", "deep.txt"));
    await put(path.join(source, "ProjectA", ".DS_Store"));
    await put(path.join(source, "ProjectA", "node_modules", "dep", "index.md"));
    await put(path.join(source, "ProjectA", "~$lock.docx"));
    await put(path.join(source, "ProjectA", "dl.crdownload"));
    await put(path.join(source, "ProjectB", "plan.md"));
    await put(path.join(source, "loose.md"));
    const started = await make().start({ driveId: DRIVE_ID, folder: "in", paths: [path.join(source, "ProjectA"), path.join(source, "loose.md"), path.join(source, "ProjectB")] });
    expect(started.skipped).toEqual({ hidden: 1, links: 0, junk: 1, temporary: 2, special: 0, gitignore: 0, ooignore: 0 });
    expect(started.skippedSamples.map((entry) => entry.reason).sort()).toEqual(["hidden", "junk", "temporary", "temporary"]);
    expect(started.roots.map(({ name, kind, files }) => [name, kind, files])).toEqual([["ProjectA", "directory", 2], ["loose.md", "file", 1], ["ProjectB", "directory", 1]]);
    await settled(started);
    expect(await tree(path.join(root, "in"))).toEqual(["ProjectA/notes/deep.txt", "ProjectA/readme.md", "ProjectB/plan.md", "loose.md"]);
    expect(requests.filter(([name]) => name === "driveSync")).toHaveLength(1);
  });

  it("keeps an empty folder as a folder and reports how many", async () => {
    await mkdir(path.join(source, "Shell", "empty"), { recursive: true });
    await put(path.join(source, "Shell", "a.md"));
    const started = await make().start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "Shell")] });
    expect(started.emptyFolders).toBe(1);
    await settled(started);
    expect(await tree(root)).toEqual(["Shell/a.md", "Shell/empty/"]);
  });

  it("a drop of only an empty folder copies nothing and still keeps the folder", async () => {
    await mkdir(path.join(source, "Void"));
    const started = await make().start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "Void")] });
    expect(started).toMatchObject({ items: [], emptyFolders: 1, roots: [{ name: "Void", kind: "directory", files: 0 }] });
    expect((await stat(path.join(root, "Void"))).isDirectory()).toBe(true);
  });

  it("counts a path dropped twice, or inside a dropped folder, once", async () => {
    await put(path.join(source, "P", "a.md"));
    await put(path.join(source, "P", "sub", "b.md"));
    const started = await make().start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "P"), path.join(source, "P", "a.md"), path.join(source, "P"), path.join(source, "P", "sub", "b.md")] });
    expect(started.items.map((item) => item.relative)).toEqual(["P/a.md", "P/sub/b.md"]);
    expect(started.roots).toHaveLength(1);
    await settled(started);
  });
});

describe("links never lead outside the dropped folder", () => {
  it("skips a link to a file or a folder outside, copies nothing through it", async () => {
    const outside = path.join(work, "outside");
    await put(path.join(outside, "secret.md"), "secret");
    await put(path.join(source, "Tree", "ok.md"));
    await symlink(path.join(outside, "secret.md"), path.join(source, "Tree", "file-link.md"));
    await symlink(outside, path.join(source, "Tree", "dir-link"));
    await symlink(path.join(source, "Tree", "ok.md"), path.join(source, "Tree", "inside-link.md"));
    const started = await make().start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "Tree")] });
    expect(started.skipped.links).toBe(3);
    await settled(started);
    expect(await tree(root)).toEqual(["Tree/ok.md"]);
  });

  it("reads a dropped link itself once (the person chose it): a link to a folder is walked, a broken one is unreadable", async () => {
    await put(path.join(source, "Real", "a.md"));
    await symlink(path.join(source, "Real"), path.join(source, "Alias"));
    await symlink(path.join(source, "nowhere"), path.join(source, "Broken"));
    const started = await make().start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "Alias"), path.join(source, "Broken")] });
    expect(started.items.map((item) => item.relative)).toEqual(["Alias/a.md"]);
    expect(started.unreadable).toBe(1);
    await settled(started);
  });
});

describe("what cannot be read", () => {
  it("carries on past an unreadable folder and a missing path, counting both", async () => {
    await put(path.join(source, "Mix", "fine.md"));
    await put(path.join(source, "Mix", "locked", "hidden-inside.md"));
    await chmod(path.join(source, "Mix", "locked"), 0o000);
    const started = await make().start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "Mix"), path.join(source, "gone.md"), "relative.md"] });
    await chmod(path.join(source, "Mix", "locked"), 0o700);
    expect(started.items.map((item) => item.relative)).toEqual(["Mix/fine.md"]);
    expect(started.unreadable).toBe(3);
    await settled(started);
  });
});

describe("the same drop twice", () => {
  it("copies nothing again when the bytes are already there, and numbers a different file of the same name", async () => {
    await put(path.join(source, "P", "a.md"), "same");
    await put(path.join(source, "P", "b.md"), "first");
    const service = make();
    const first = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "P")] });
    await settled(first);
    await put(path.join(source, "P", "b.md"), "changed");
    pushes = [];
    const second = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "P")] });
    const last = await settled(second);
    const bySource = Object.fromEntries(second.items.map((item) => [item.name, last.items.find((entry) => entry.itemId === item.itemId)]));
    expect(bySource["a.md"]).toMatchObject({ state: "skipped", skipReason: "duplicate", drivePath: "P/a.md" });
    expect(bySource["b.md"]).toMatchObject({ state: "done", drivePath: "P/b (2).md" });
    expect(await tree(root)).toEqual(["P/a.md", "P/b (2).md", "P/b.md"]);
    expect(await readFile(path.join(root, "P", "b.md"), "utf8")).toBe("first");
  });

  it("asks for no sync when every file was already there", async () => {
    await put(path.join(source, "a.md"), "same");
    const service = make();
    await settled(await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "a.md")] }));
    requests.length = 0;
    await settled(await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "a.md")] }));
    expect(requests.filter(([name]) => name === "driveSync")).toHaveLength(0);
  });
});

describe("very large drops", () => {
  it("copies 1,200 files with pushes of what changed: the whole list goes out when the rows are released and once more at the end", async () => {
    await Promise.all(Array.from({ length: 1200 }, (_, index) => put(path.join(source, "Big", `d${index % 12}`, `f${index}.md`), `n${index}`)));
    const started = await make().start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "Big")] });
    expect(started.items).toHaveLength(1200);
    const last = await settled(started);
    expect(last.items.every((item) => item.state === "done")).toBe(true);
    const all = progresses();
    const sizes = all.map((entry) => entry.items.length);
    const released = all.findIndex((entry) => (entry.added ?? []).length > 0);
    expect(sizes.filter((size) => size >= 1200)).toHaveLength(2);
    expect(Math.max(...sizes.slice(released + 1, -1))).toBeLessThan(1200);
    expect((await tree(root)).length).toBe(1200);
  }, 60_000);
});

describe("keeping a dropped folder in sync instead of the copy", () => {
  it("takes back the copies this drop made of that folder, and nothing else", async () => {
    await put(path.join(source, "Keep", "a.md"), "aaa");
    await put(path.join(source, "Keep", "deep", "b.md"), "bbb");
    await put(path.join(source, "other.md"), "ooo");
    await put(path.join(root, "Keep", "already.md"), "was there before");
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "Keep"), path.join(source, "other.md")] });
    await settled(started);
    requests.length = 0;
    expect(await service.discard({ batchId: started.batchId, folderPath: path.join(source, "Keep") })).toEqual({ removed: 2 });
    expect(await tree(root)).toEqual(["Keep/already.md", "other.md"]);
    expect(requests.filter(([name]) => name === "driveSync")).toHaveLength(1);
  });

  it("removes the folders the drop made once they are empty, but never the destination", async () => {
    await put(path.join(source, "P", "sub", "x.md"), "x");
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "in/here", paths: [path.join(source, "P")] });
    await settled(started);
    expect((await service.discard({ batchId: started.batchId, folderPath: path.join(source, "P") })).removed).toBe(1);
    expect(await tree(root)).toEqual(["in/here/"]);
  });

  it("leaves a copy alone that changed since, and a file that was already there", async () => {
    await put(path.join(source, "P", "a.md"), "first");
    await put(path.join(source, "P", "b.md"), "same");
    await put(path.join(root, "P", "b.md"), "same");
    const service = make();
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [path.join(source, "P")] });
    await settled(started);
    await writeFile(path.join(root, "P", "a.md"), "edited meanwhile, longer");
    expect((await service.discard({ batchId: started.batchId, folderPath: path.join(source, "P") })).removed).toBe(0);
    expect(await tree(root)).toEqual(["P/a.md", "P/b.md"]);
  });

  it("refuses an unknown batch and a relative folder", async () => {
    const service = make();
    await expect(service.discard({ batchId: "00000000-0000-4000-8000-0000000000ff", folderPath: "/x" })).rejects.toMatchObject({ code: "SO_INVALID_REQUEST" });
    const started = await service.start({ driveId: DRIVE_ID, folder: "", paths: [] });
    await expect(service.discard({ batchId: started.batchId, folderPath: "relative" })).rejects.toMatchObject({ code: "SO_INVALID_REQUEST" });
  });
});
