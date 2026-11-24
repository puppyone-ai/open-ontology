import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { withHostDriveRoots } from "@so-main/drive-host-roots.mjs";

let stateDir;
beforeEach(async () => { stateDir = await fs.mkdtemp(path.join(os.tmpdir(), "drive-roots-")); });
afterEach(async () => { await fs.rm(stateDir, { recursive: true, force: true }); });

const drive = (root) => ({ id: "d1", backend: "fs", config: { root, exclude: ["tmp"] } });
const clientAnswering = (answers) => ({ marker: "kept", request: async (name) => ({ value: answers[name], replayed: false }) });

describe("Docker runtime Drive roots", () => {
  it("replaces the container folder with the host folder `oo serve` recorded, in a Drive and in a list", async () => {
    await fs.writeFile(path.join(stateDir, "drive-mounts.json"), JSON.stringify({ mounts: { "4df3aedde1d6382a": "/Users/example/Drive" } }));
    const client = withHostDriveRoots(clientAnswering({ driveGet: drive("/drives/4df3aedde1d6382a"), drivesList: { items: [drive("/drives/4df3aedde1d6382a"), drive("/Users/example-other/Folder")] } }), { stateDir, fsPromises: fs });
    expect((await client.request("driveGet")).value.config).toEqual({ root: "/Users/example/Drive", exclude: ["tmp"] });
    expect((await client.request("drivesList")).value.items.map((item) => item.config.root)).toEqual(["/Users/example/Drive", "/Users/example-other/Folder"]);
    expect(client.marker).toBe("kept");
  });

  it("leaves a root alone that is not a container mount, or that the table does not know, and other answers untouched", async () => {
    const client = withHostDriveRoots(clientAnswering({ driveGet: drive("/drives/4df3aedde1d6382a"), filesList: { items: [1] } }), { stateDir, fsPromises: fs });
    expect((await client.request("driveGet")).value.config.root).toBe("/drives/4df3aedde1d6382a");
    expect((await client.request("filesList")).value).toEqual({ items: [1] });
  });

  it("reads the older mounts.json when drive-mounts.json is absent", async () => {
    await fs.writeFile(path.join(stateDir, "mounts.json"), JSON.stringify({ mounts: { "0123456789abcdef": "/Users/example/Old" } }));
    const client = withHostDriveRoots(clientAnswering({ driveGet: drive("/drives/0123456789abcdef") }), { stateDir, fsPromises: fs });
    expect((await client.request("driveGet")).value.config.root).toBe("/Users/example/Old");
  });

  it("is the client itself without a state directory", () => {
    const client = clientAnswering({});
    expect(withHostDriveRoots(client, { stateDir: null, fsPromises: fs })).toBe(client);
  });
});
