import { mkdir, mkdtemp, realpath, rm, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createSuperOntologyService } from "@so-main/service.mjs";
import { parseSuperOntologyRequest } from "@so-shared/requests.mjs";

// A folder dropped on the window becomes a one-use `drive-root` grant (the same thing the native picker issues), so it can be kept in sync as a source.
const quiet = { debug() {}, warn() {}, info() {} };
let work; let service;
beforeEach(async () => {
  work = await realpath(await mkdtemp(path.join(os.tmpdir(), "so-grant-")));
  service = createSuperOntologyService({ logger: quiet, platformHost: { platform: "darwin" }, paths: { stateDir: work }, broadcast: () => {} });
});
afterEach(async () => { await rm(work, { recursive: true, force: true }); });

const grant = (dropped, sender = {}) => service.handlers["grant-dropped-folder"]({ path: dropped }, { sender });

describe("grant-dropped-folder", () => {
  it("issues a grant with the real path of a folder, never more", async () => {
    await mkdir(path.join(work, "Project"));
    const issued = await grant(path.join(work, "Project"));
    expect(Object.keys(issued).sort()).toEqual(["displayPath", "grantId"]);
    expect(issued.displayPath).toBe(path.join(work, "Project"));
  });
  it("resolves a link to a folder to the folder itself", async () => {
    await mkdir(path.join(work, "Real"));
    await symlink(path.join(work, "Real"), path.join(work, "Alias"));
    expect((await grant(path.join(work, "Alias"))).displayPath).toBe(path.join(work, "Real"));
  });
  it("refuses a file, a missing path and a relative path", async () => {
    await writeFile(path.join(work, "n.md"), "x");
    await expect(grant(path.join(work, "n.md"))).rejects.toMatchObject({ code: "SO_UPLOAD_SOURCE_MISSING" });
    await expect(grant(path.join(work, "gone"))).rejects.toMatchObject({ code: "SO_UPLOAD_SOURCE_MISSING" });
    await expect(grant("relative/dir")).rejects.toMatchObject({ code: "SO_INVALID_REQUEST" });
  });
  it("is closed: nothing but a path is accepted", () => {
    expect(parseSuperOntologyRequest("grant-dropped-folder", { path: "/a/b" })).toEqual({ path: "/a/b" });
    for (const body of [{}, { path: "" }, { path: "/a", extra: 1 }, { path: 4 }]) expect(() => parseSuperOntologyRequest("grant-dropped-folder", body)).toThrow(expect.objectContaining({ code: "SO_INVALID_REQUEST" }));
  });
});
