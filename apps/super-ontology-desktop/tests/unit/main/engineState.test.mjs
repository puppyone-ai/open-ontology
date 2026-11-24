import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { cliConfigDirectory, cliStateDirectory, readActiveProfile, resolveEngineState } from "../../../electron/main/super-ontology/engine-state.mjs";

let root;
beforeEach(async () => { root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), "so-state-"))); });
afterEach(async () => { await fs.rm(root, { recursive: true, force: true }); });

describe("where the shared engine keeps its state", () => {
  it("follows the CLI: OO_DOCS_STATE_DIR, else $HOME/.local/state/open-ontology/docs", () => {
    expect(cliStateDirectory({ env: {}, homedir: () => "/h" })).toBe("/h/.local/state/open-ontology/docs");
    expect(cliStateDirectory({ env: { OO_DOCS_STATE_DIR: "/s/docs" }, homedir: () => "/h" })).toBe("/s/docs");
    expect(cliStateDirectory({ env: { OO_DOCS_STATE_DIR: "relative" }, homedir: () => "/h" })).toBe("/h/.local/state/open-ontology/docs");
    expect(cliConfigDirectory({ env: {}, homedir: () => "/h" })).toBe("/h/.config/open-ontology");
    expect(cliConfigDirectory({ env: { OO_CONFIG_DIR: "/c" }, homedir: () => "/h" })).toBe("/c");
  });

  it("a new install uses the CLI's directory", async () => {
    const resolved = await resolveEngineState({ legacyDir: path.join(root, "app", "state"), env: {}, homedir: () => root });
    expect(resolved).toMatchObject({ mode: "cli", decided: false, stateDir: path.join(root, ".local/state/open-ontology/docs"), legacyHasData: false });
  });

  it("an install that holds data of its own keeps it until the person chooses, and nothing is touched", async () => {
    const legacy = path.join(root, "app", "state");
    await fs.mkdir(legacy, { recursive: true });
    await fs.writeFile(path.join(legacy, "serve.json"), "{}");
    const kept = await resolveEngineState({ legacyDir: legacy, env: {}, homedir: () => root });
    expect(kept).toMatchObject({ mode: "app", decided: false, stateDir: legacy, legacyHasData: true });
    const chosen = await resolveEngineState({ legacyDir: legacy, choice: "cli", env: {}, homedir: () => root });
    expect(chosen).toMatchObject({ mode: "cli", decided: true, stateDir: path.join(root, ".local/state/open-ontology/docs") });
    expect(await fs.readdir(legacy)).toEqual(["serve.json"]);
    const back = await resolveEngineState({ legacyDir: legacy, choice: "app", env: {}, homedir: () => root });
    expect(back.mode).toBe("app");
  });

  it("SUPER_ONTOLOGY_ENGINE_DATA overrides the saved choice", async () => {
    const legacy = path.join(root, "app", "state");
    expect((await resolveEngineState({ legacyDir: legacy, choice: "cli", env: { SUPER_ONTOLOGY_ENGINE_DATA: "app" }, homedir: () => root })).stateDir).toBe(legacy);
  });
});

describe("the active oo profile", () => {
  it("reads the name and the Gateway host, never a token path", async () => {
    await fs.mkdir(path.join(root, "profiles"), { recursive: true });
    await fs.writeFile(path.join(root, "active-profile.json"), JSON.stringify({ profile: "shop" }));
    await fs.writeFile(path.join(root, "profiles", "shop.json"), JSON.stringify({ gateway: "https://gw.example.test:8443/x", token_file: "/secret/token" }));
    const profile = await readActiveProfile({ configDir: root });
    expect(profile).toEqual({ name: "shop", gatewayHost: "gw.example.test:8443", saved: true });
    expect(JSON.stringify(profile)).not.toContain("secret");
  });
  it("is null without an active profile and rejects odd names", async () => {
    expect(await readActiveProfile({ configDir: root })).toBeNull();
    await fs.writeFile(path.join(root, "active-profile.json"), JSON.stringify({ profile: "../x" }));
    expect(await readActiveProfile({ configDir: root })).toBeNull();
  });
});
