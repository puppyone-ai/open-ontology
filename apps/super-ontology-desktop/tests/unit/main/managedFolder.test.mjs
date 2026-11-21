import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { createManagedFolder, MERGED_NAME } from "../../../electron/main/super-ontology/managed-folder.mjs";

let root; let calls;
beforeEach(async () => { root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), "so-managed-"))); calls = []; });
afterEach(async () => { await fs.rm(root, { recursive: true, force: true }); });

const drive = (id, name, dir, extra = {}) => ({ id, name, backend: "fs", status: "active", revision: 1, config: { root: dir }, ...extra });
const client = (items) => ({ request: async (name, options) => { calls.push({ name, options }); return { value: name === "drivesList" ? { items } : {} }; } });

describe("one managed folder", () => {
  it("renames the CLI's earlier `default` Drive to My files when there is no My files", async () => {
    const legacy = drive("d1", "default", path.join(root, "state", "drives", "default"), { revision: 4 });
    expect(await createManagedFolder({ client: client([legacy]) }).merge()).toEqual({ action: "renamed", driveId: "d1" });
    expect(calls.at(-1)).toMatchObject({ name: "driveUpdate", options: { params: { id: "d1" }, body: { name: "My files" }, revision: 4 } });
  });

  it("when both exist: copies the files without overwriting, keeps the originals, syncs My files and retires the earlier Drive", async () => {
    const from = path.join(root, "state", "drives", "default"); const to = path.join(root, "My files");
    await fs.mkdir(path.join(from, "sub"), { recursive: true }); await fs.mkdir(to, { recursive: true });
    await fs.writeFile(path.join(from, "a.md"), "from the CLI"); await fs.writeFile(path.join(from, "sub", "b.md"), "b"); await fs.writeFile(path.join(from, ".hidden"), "x");
    await fs.writeFile(path.join(to, "a.md"), "a different file");
    const result = await createManagedFolder({ client: client([drive("d1", "default", from), drive("d2", "My files", to)]) }).merge();
    expect(result).toMatchObject({ action: "merged", driveId: "d2", copied: 2 });
    expect(await fs.readFile(path.join(to, "a.md"), "utf8")).toBe("a different file");
    expect(await fs.readFile(path.join(to, "a (2).md"), "utf8")).toBe("from the CLI");
    expect(await fs.readFile(path.join(to, "sub", "b.md"), "utf8")).toBe("b");
    await expect(fs.stat(path.join(to, ".hidden"))).rejects.toThrow();
    expect(await fs.readFile(path.join(from, "a.md"), "utf8")).toBe("from the CLI");
    expect(calls.map((c) => c.name)).toEqual(["drivesList", "driveSync", "driveUpdate"]);
    expect(calls.at(-1).options.body).toEqual({ name: MERGED_NAME, status: "paused" });
  });

  it("does nothing when there is no earlier folder, or after it was merged", async () => {
    const mine = drive("d2", "My files", path.join(root, "My files"));
    expect((await createManagedFolder({ client: client([mine]) }).merge()).action).toBe("none");
    expect((await createManagedFolder({ client: client([mine, drive("d1", MERGED_NAME, path.join(root, "state", "drives", "default"))]) }).merge()).action).toBe("none");
    expect(calls.every((c) => c.name === "drivesList")).toBe(true);
  });

  it("leaves a folder named default that the CLI did not make alone", async () => {
    expect((await createManagedFolder({ client: client([drive("d1", "default", path.join(root, "projects", "default"))]) }).merge()).action).toBe("none");
  });
});
