import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createSuperOntologyService } from "@so-main/service.mjs";

// This app has no hosted service: the channels stay for the bridge's shape and always answer "local, not configured".
let root;
beforeEach(() => { root = fs.mkdtempSync(path.join(os.tmpdir(), "so-cloud-mode-")); });
afterEach(() => { fs.rmSync(root, { recursive: true, force: true }); });

const make = () => createSuperOntologyService({ logger: { debug() {}, warn() {} }, paths: { stateDir: path.join(root, "state"), settingsFile: path.join(root, "settings.json") } });

describe("cloud mode channels", () => {
  it("always read local and not configured, with no host", async () => {
    expect(await make().handlers["cloud-mode-read"]({})).toEqual({ mode: "local", configured: false, host: null, remoteUrl: null });
  });
  it("refuses cloud mode and keeps local", async () => {
    const service = make();
    await expect(service.handlers["cloud-mode-set"]({ mode: "cloud" })).rejects.toMatchObject({ code: "SO_CLOUD_NOT_CONFIGURED" });
    expect(await service.handlers["cloud-mode-set"]({ mode: "local" })).toMatchObject({ mode: "local", configured: false });
  });
  it("ignores a saved cloud choice from a settings file", async () => {
    fs.writeFileSync(path.join(root, "settings.json"), JSON.stringify({ cloudMode: "cloud" }));
    expect(await make().handlers["cloud-mode-read"]({})).toMatchObject({ mode: "local", configured: false });
  });
});
