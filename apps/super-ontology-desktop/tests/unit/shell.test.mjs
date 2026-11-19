import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { createAppProtocolHandler } from "../../electron/main/app-protocol.mjs";
import { createIpcRegistry } from "../../electron/main/ipc-registry.mjs";
import { bundledExecutablePath } from "../../electron/main/oo-bootstrap.mjs";
import { createSettingsStore, sanitizeSettings } from "../../electron/main/settings-store.mjs";
import { resolveLanguage } from "../../electron/main/strings.mjs";
import { isExternalUrl } from "../../electron/main/window.mjs";
import { compareVersions, createEngineLocator } from "../../electron/main/super-ontology/engine-locator.mjs";
import { normalizeRemoteUrl } from "../../electron/main/super-ontology/remote-engine.mjs";
import { SUPER_ONTOLOGY_CHANNELS, SUPER_ONTOLOGY_PUSH_CHANNELS } from "../../shared/super-ontology/requests.mjs";

const appRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const tmp = () => fs.mkdtemp(path.join(os.tmpdir(), "so-shell-"));

describe("preload channel list", () => {
  it("repeats the shared request channels exactly", async () => {
    const source = await fs.readFile(path.join(appRoot, "electron", "preload.cjs"), "utf8");
    const listed = [...source.slice(source.indexOf("const CHANNELS"), source.indexOf("const PUSH_CHANNELS")).matchAll(/"([a-z-]+)"/gu)].map((m) => m[1]);
    expect([...listed].sort()).toEqual([...SUPER_ONTOLOGY_CHANNELS].sort());
    const push = [...source.slice(source.indexOf("const PUSH_CHANNELS"), source.indexOf("const NOTICE_KINDS")).matchAll(/"([a-z-]+)"/gu)].map((m) => m[1]);
    expect(push).toEqual([...SUPER_ONTOLOGY_PUSH_CHANNELS]);
  });
});

describe("settings store", () => {
  it("falls back to defaults, persists atomically with 0600 and keeps main-owned keys out of bad input", async () => {
    const dir = await tmp();
    const file = path.join(dir, "s", "settings.json");
    const store = createSettingsStore({ file });
    expect(await store.read()).toMatchObject({ language: "system", runtime: "auto", remote: null, ooExecutable: null });
    await store.update({ language: "zh-Hans", ooExecutable: "relative/oo", runtime: "bogus" });
    const saved = JSON.parse(await fs.readFile(file, "utf8"));
    expect(saved).toMatchObject({ language: "zh-Hans", ooExecutable: null, runtime: "auto" });
    expect((await fs.stat(file)).mode & 0o777).toBe(0o600);
    expect(sanitizeSettings({ remote: { url: "http://h:1", tokenFile: "rel" } }).remote).toBeNull();
  });
  it("resolves the UI language", () => {
    expect(resolveLanguage("system", "zh-CN")).toBe("zh-Hans");
    expect(resolveLanguage("system", "en-US")).toBe("en");
    expect(resolveLanguage("en", "zh-CN")).toBe("en");
  });
});

describe("ipc registry", () => {
  it("refuses untrusted senders and double registration", async () => {
    const handlers = new Map();
    const ipcMain = { handle: (channel, handler) => handlers.set(channel, handler) };
    const registry = createIpcRegistry({ ipcMain, isTrustedSender: (event) => event.trusted === true, logger: { warn() {} } });
    registry.handle("x:one", async () => "ran");
    expect(() => registry.handle("x:one", async () => null)).toThrow();
    expect(await handlers.get("x:one")({ trusted: true })).toBe("ran");
    expect(await handlers.get("x:one")({ trusted: false })).toMatchObject({ ok: false });
  });
});

describe("app protocol", () => {
  it("serves inside the root only", async () => {
    const dir = await tmp();
    await fs.mkdir(path.join(dir, "root"));
    await fs.writeFile(path.join(dir, "root", "index.html"), "<p>hi</p>");
    await fs.writeFile(path.join(dir, "secret.txt"), "no");
    await fs.symlink(path.join(dir, "secret.txt"), path.join(dir, "root", "link.txt"));
    const handle = createAppProtocolHandler({ root: path.join(dir, "root") });
    const ok = await handle({ url: "app://renderer/", method: "GET" });
    expect(ok.status).toBe(200);
    expect(ok.headers.get("content-security-policy")).toContain("default-src 'self'");
    expect((await handle({ url: "app://renderer/%2e%2e/secret.txt", method: "GET" })).status).toBe(404);
    expect((await handle({ url: "app://renderer/link.txt", method: "GET" })).status).toBe(404);
    expect((await handle({ url: "app://other/index.html", method: "GET" })).status).toBe(404);
  });
});

describe("oo bootstrap helpers", () => {
  it("compares versions", () => {
    expect(compareVersions("0.2.1", "0.2.0")).toBe(1);
    expect(compareVersions("0.1.9", "0.2.0")).toBe(-1);
    expect(compareVersions("0.2.0-rc.1", "0.2.0")).toBe(0);
  });
  it("prefers the user path, then PATH when new enough, then the bundled binary", async () => {
    const dir = await tmp();
    const make = async (name, version) => {
      const file = path.join(dir, name);
      await fs.writeFile(file, `#!/bin/sh\nif [ "$1" = "serve" ]; then echo "oo serve is not running"; exit 1; fi\necho '{"name":"oo","version":"${version}"}'\n`, { mode: 0o755 });
      return file;
    };
    const old = await make("path-oo", "0.1.0");
    await fs.rename(old, path.join(dir, "oo"));
    const bundled = await make("bundled-oo", "0.2.1");
    const user = await make("user-oo", "0.3.0");
    const readEnvironment = async () => ({ environment: { PATH: dir } });
    const locator = createEngineLocator({ platform: "macos", readEnvironment, bundledExecutable: () => bundled });
    expect((await locator.locate({})).oo).toMatchObject({ source: "bundled", version: "0.2.1" });
    expect((await locator.locate({ userExecutable: user })).oo).toMatchObject({ source: "user", version: "0.3.0" });
    await fs.writeFile(path.join(dir, "oo"), '#!/bin/sh\nif [ "$1" = "serve" ]; then echo "oo serve is not running"; exit 1; fi\necho \'{"name":"oo","version":"0.2.5"}\'\n', { mode: 0o755 });
    expect((await locator.locate({})).oo).toMatchObject({ source: "path", version: "0.2.5" });
    // An `oo` without `serve` (the npm distribution) is skipped in favor of the bundled one.
    await fs.writeFile(path.join(dir, "oo"), '#!/bin/sh\nif [ "$1" = "serve" ]; then echo \'{"class":"unsupported","message":"command is not implemented"}\'; exit 2; fi\necho \'{"name":"oo","version":"0.2.9"}\'\n', { mode: 0o755 });
    expect((await locator.locate({})).oo).toMatchObject({ source: "bundled", version: "0.2.1" });
    expect((await createEngineLocator({ platform: "macos", readEnvironment: async () => ({ environment: { PATH: "/nonexistent" } }) }).locate({})).oo).toBeNull();
  });
  it("resolves the bundled path", () => {
    expect(bundledExecutablePath({ resourcesPath: "/R", env: {}, appRoot: "/A" })).toBe("/R/oo/oo");
    expect(bundledExecutablePath({ resourcesPath: null, env: {}, appRoot: "/A" })).toBe("/A/build/oo-bundle/oo");
    expect(bundledExecutablePath({ resourcesPath: "/R", env: { SUPER_ONTOLOGY_OO_BINARY: "/x/oo" }, appRoot: "/A" })).toBe("/x/oo");
  });
});

describe("remote engine and external links", () => {
  it("accepts only a bare http(s) origin", () => {
    expect(normalizeRemoteUrl("https://engine.example:8443/")).toBe("https://engine.example:8443");
    for (const bad of ["ftp://h", "http://u:p@h", "http://h/path", "http://h?x=1", "nope", ""]) expect(() => normalizeRemoteUrl(bad)).toThrow();
  });
  it("opens only http(s) links", () => {
    expect(isExternalUrl("https://example.org/a")).toBe(true);
    expect(isExternalUrl("file:///etc/passwd")).toBe(false);
    expect(isExternalUrl("https://u:p@example.org")).toBe(false);
  });
});

describe("module graph", () => {
  it("imports every main-process module (a missing export fails here, not at launch)", async () => {
    const files = [];
    const walk = async (directory) => {
      for (const entry of await fs.readdir(directory, { withFileTypes: true })) {
        const full = path.join(directory, entry.name);
        if (entry.isDirectory()) await walk(full); else if (entry.name.endsWith(".mjs") && entry.name !== "index.mjs") files.push(full);
      }
    };
    await walk(path.join(appRoot, "electron", "main"));
    expect(files.length).toBeGreaterThan(30);
    for (const file of files) await import(file);
  });
});
