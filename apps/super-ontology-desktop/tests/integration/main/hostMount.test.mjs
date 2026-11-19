import { createHash } from "node:crypto";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createHostMount } from "@so-main/host-mount.mjs";

const sha = (text) => createHash("sha256").update(text).digest("hex");
const CONTENT = { "BRIEF.md": "# brief\n", "manifest.json": "{}\n", "facts/a.md": "fact\n" };

function shareFor(id, role = "suggester", content = CONTENT) {
  return {
    id, pack_id: `pack-${id}`, role, mount_ref: `ref-${id}`, share_json: JSON.stringify({ share_id: id }),
    files: Object.entries(content).map(([file, text]) => ({ path: file, sha256: sha(text), bytes: Buffer.byteLength(text) })),
  };
}

describe("host mount (10 section 7.7)", () => {
  let root; let target; let state; let mounter; let served;
  beforeEach(async () => {
    root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), "sd-host-mount-")));
    target = path.join(root, "target");
    state = path.join(root, "state");
    await fs.mkdir(target); await fs.mkdir(state);
    served = { ...CONTENT };
    mounter = createHostMount({ stateDir: state, homeDir: path.join(root, "home"), fetchPackFile: async (_pack, file) => new TextEncoder().encode(served[file]) });
  });
  afterEach(async () => {
    const open = async (directory) => { await fs.chmod(directory, 0o700).catch(() => {}); for (const entry of await fs.readdir(directory).catch(() => [])) { const next = path.join(directory, entry); if ((await fs.lstat(next)).isDirectory()) await open(next); } };
    await open(root);
    await fs.rm(root, { recursive: true, force: true });
  });

  it("writes read-only files with SHARE.json, a writable suggest directory for a suggester, and records the mount", async () => {
    const share = shareFor("s1");
    const mounted = await mounter.install(share, await mounter.preflight(target));
    expect(mounted).toBe(path.join(target, ".context"));
    expect(await fs.readFile(path.join(mounted, "BRIEF.md"), "utf8")).toBe("# brief\n");
    expect(await fs.readFile(path.join(mounted, "SHARE.json"), "utf8")).toBe(share.share_json);
    expect((await fs.stat(path.join(mounted, "BRIEF.md"))).mode & 0o777).toBe(0o444);
    expect((await fs.stat(path.join(mounted, "facts"))).mode & 0o777).toBe(0o555);
    expect((await fs.stat(path.join(mounted, "suggest"))).mode & 0o777).toBe(0o755);
    const record = JSON.parse(await fs.readFile(path.join(state, "mounts.json"), "utf8"));
    expect(record["ref-s1"]).toMatchObject({ share_id: "s1", path: target, updates_seq: 0 });
    expect((await fs.stat(path.join(state, "mounts.json"))).mode & 0o777).toBe(0o600);
  });

  it("annotates a Share with the folder it was mounted into and whether it is still there", async () => {
    const share = shareFor("s1");
    await mounter.install(share, target);
    const [mounted] = await mounter.annotate([{ id: "s1", mount: { present: false } }]);
    expect(mounted.mount).toEqual({ present: true, target_dir: target });
    await mounter.remove("s1");
    await mounter.install(shareFor("s9"), target);
    const [moved] = await mounter.annotate([{ id: "s1", mount: { present: false } }]);
    expect(moved.mount.present).toBe(false);
    const [unknown] = await mounter.annotate([{ id: "zz", mount: { present: false } }]);
    expect(unknown.mount).toEqual({ present: false });
  });

  it("gives a viewer no suggest directory", async () => {
    const mounted = await mounter.install(shareFor("s1", "viewer"), target);
    await expect(fs.stat(path.join(mounted, "suggest"))).rejects.toThrow();
  });

  it("refuses an occupied target, a relative path, the state directory and the home directory", async () => {
    await mounter.install(shareFor("s1"), target);
    await expect(mounter.preflight(target)).rejects.toMatchObject({ code: "DOCS_SHARE_TARGET_OCCUPIED" });
    await expect(mounter.preflight("relative")).rejects.toMatchObject({ code: "DOCS_SHARE_TARGET_INVALID" });
    await expect(mounter.preflight(state)).rejects.toMatchObject({ code: "DOCS_SHARE_TARGET_INVALID" });
    await fs.mkdir(path.join(root, "home"));
    await expect(mounter.preflight(path.join(root, "home"))).rejects.toMatchObject({ code: "DOCS_SHARE_TARGET_INVALID" });
    await expect(mounter.preflight(path.join(root, "missing"))).rejects.toMatchObject({ code: "DOCS_SHARE_TARGET_INVALID" });
  });

  it("leaves nothing behind when a file does not match its manifest entry", async () => {
    served["BRIEF.md"] = "tampered\n";
    await expect(mounter.install(shareFor("s1"), target)).rejects.toMatchObject({ code: "DOCS_SHARE_TARGET_WRITE_FAILED" });
    expect(await fs.readdir(target)).toEqual([]);
  });

  it("refuses an unsafe pack path", async () => {
    const share = shareFor("s1");
    share.files[0].path = "../escape.md";
    await expect(mounter.install(share, target)).rejects.toMatchObject({ code: "DOCS_SHARE_TARGET_WRITE_FAILED" });
  });

  it("replaces the folder of the Share it refreshes and records the new Share", async () => {
    await mounter.install(shareFor("s1"), target);
    served = { ...CONTENT, "BRIEF.md": "# brief v2\n" };
    const next = shareFor("s2", "suggester", served);
    const mounted = await mounter.replace("s1", next);
    expect(await fs.readFile(path.join(mounted, "BRIEF.md"), "utf8")).toBe("# brief v2\n");
    expect(JSON.parse(await fs.readFile(path.join(mounted, "SHARE.json"), "utf8")).share_id).toBe("s2");
    expect((await fs.readdir(target)).sort()).toEqual([".context"]);
    expect(await mounter.directoryOf("s2")).toBe(target);
  });

  it("does nothing for a refresh of a Share with no recorded folder", async () => {
    expect(await mounter.replace("unknown", shareFor("s2"))).toBeNull();
  });

  it("removes only a folder whose SHARE.json is ours and drops its record", async () => {
    await mounter.install(shareFor("s1"), target);
    expect(await mounter.remove("s1")).toBe(true);
    expect(await fs.readdir(target)).toEqual([]);
    expect(await mounter.directoryOf("s1")).toBeNull();
    // A foreign .context is never touched.
    await mounter.install(shareFor("s3"), target);
    await expect(mounter.replace("s3", shareFor("s4"))).resolves.toBeTruthy();
    expect(await mounter.remove("s3")).toBe(false);
    expect(await fs.readdir(target)).toEqual([".context"]);
  });
});
