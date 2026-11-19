import { mkdtemp, mkdir, rm, symlink, writeFile, chmod } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createGrantStore } from "@so-main/grants.mjs";
import { createNativePathPicker } from "@so-main/path-picker.mjs";

describe("native path picker", () => {
  let root;
  beforeEach(async () => { root = await mkdtemp(path.join(os.tmpdir(), "so-picker-")); });
  afterEach(async () => { await rm(root, { recursive: true, force: true }); });

  const translate = (key) => `t:${key}`;

  it("titles the dialog per purpose and returns the canonical path", async () => {
    const folder = path.join(root, "real");
    await mkdir(folder);
    const link = path.join(root, "link");
    await symlink(folder, link);
    const dialog = { showOpenDialog: vi.fn(async () => ({ canceled: false, filePaths: [link] })) };
    const pick = createNativePathPicker({ dialog, translate });
    expect(await pick({ sender: {}, purpose: "drive-root" })).toBe(await (await import("node:fs/promises")).realpath(folder));
    expect(dialog.showOpenDialog).toHaveBeenCalledWith({ title: "t:pick.driveRoot", properties: ["openDirectory"] });
    await pick({ sender: {}, purpose: "mount-target" });
    expect(dialog.showOpenDialog.mock.calls[1][0].title).toBe("t:pick.mountTarget");
  });

  it("parents the dialog to the owner window when there is one", async () => {
    const owner = { isDestroyed: () => false };
    const dialog = { showOpenDialog: vi.fn(async () => ({ canceled: true, filePaths: [] })) };
    const pick = createNativePathPicker({ dialog, translate, getOwnerWindow: () => owner });
    expect(await pick({ sender: {}, purpose: "oo-executable" })).toBeNull();
    expect(dialog.showOpenDialog.mock.calls[0][0]).toBe(owner);
  });

  it("accepts only an executable file for oo-executable and only a directory otherwise", async () => {
    const script = path.join(root, "oo");
    await writeFile(script, "#!/bin/sh\n");
    const dialogFor = (target) => ({ showOpenDialog: async () => ({ canceled: false, filePaths: [target] }) });
    await expect(createNativePathPicker({ dialog: dialogFor(script), translate })({ sender: {}, purpose: "oo-executable" })).rejects.toThrow();
    await chmod(script, 0o755);
    expect(await createNativePathPicker({ dialog: dialogFor(script), translate })({ sender: {}, purpose: "oo-executable" })).toContain("oo");
    expect(await createNativePathPicker({ dialog: dialogFor(root), translate })({ sender: {}, purpose: "oo-executable" })).toBeNull();
    expect(await createNativePathPicker({ dialog: dialogFor(script), translate })({ sender: {}, purpose: "drive-root" })).toBeNull();
  });
});

describe("grant store", () => {
  it("replaces an earlier grant of the same purpose and keeps mount-target reusable", () => {
    const ids = ["g1111111", "g2222222", "g3333333"];
    const grants = createGrantStore({ createId: () => ids.shift() });
    const sender = {};
    const first = grants.issue(sender, "drive-root", "/a");
    const second = grants.issue(sender, "drive-root", "/b");
    expect(() => grants.resolve(sender, first.grantId, "drive-root")).toThrow(expect.objectContaining({ code: "SO_GRANT_REQUIRED" }));
    expect(grants.resolve(sender, second.grantId, "drive-root")).toBe("/b");
    expect(() => grants.resolve(sender, second.grantId, "drive-root")).toThrow(expect.objectContaining({ code: "SO_GRANT_REQUIRED" }));
    const mount = grants.issue(sender, "mount-target", "/m");
    expect(grants.resolve(sender, mount.grantId, "mount-target")).toBe("/m");
    expect(grants.resolve(sender, mount.grantId, "mount-target")).toBe("/m");
    expect(grants.revoke(sender, mount.grantId)).toBe(true);
    expect(() => grants.resolve(sender, mount.grantId, "mount-target")).toThrow();
  });

  it("rejects anything that is not a window object", () => {
    expect(() => createGrantStore().issue(null, "drive-root", "/a")).toThrow(expect.objectContaining({ code: "SO_GRANT_REQUIRED" }));
  });
});
