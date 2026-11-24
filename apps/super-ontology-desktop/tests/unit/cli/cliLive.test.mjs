import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createCliHandlers } from "../../../electron/main/super-ontology/cli-handlers.mjs";

const FAKE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../support/fake-oo-cli.mjs");
let root; let log;
const calls = async () => (await fs.readFile(log, "utf8").catch(() => "")).split("\n").filter(Boolean).map((l) => JSON.parse(l));

beforeEach(async () => {
  root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), "so-live-")));
  log = path.join(root, "calls.jsonl");
  process.env.FAKE_OO_LOG = log;
  await fs.mkdir(path.join(root, "state"), { recursive: true });
});
afterEach(async () => { delete process.env.FAKE_OO_LOG; await fs.rm(root, { recursive: true, force: true }); });

const make = (extra = {}) => {
  const liveHooks = { onEvent: null };
  const handlers = createCliHandlers({
    locator: null, grants: {}, stateDir: path.join(root, "state"), dataDir: path.join(root, "app"), stateRegistryDir: path.join(root, "state"), configDir: path.join(root, "cfg"),
    liveHooks, client: { request: async () => ({ value: {} }) }, environment: { SO_CLI_OO: FAKE, SO_CLI_HOME: path.join(root, "home"), FAKE_OO_LOG: log }, homedir: () => root, ...extra,
  });
  return { handlers, liveHooks };
};

describe("a database added with the CLI shows up in the app", () => {
  it("lists the sources of the stack the CLI used last (stacks.json), not only the app's own stack", async () => {
    const project = path.join(root, "project");
    await fs.mkdir(project, { recursive: true });
    await fs.writeFile(path.join(project, "stack.yaml"), "version: oo.stack/v1\n");
    await fs.writeFile(path.join(root, "state", "stacks.json"), JSON.stringify({ protocol: "oo.stacks/v1", stacks: [{ path: path.join(root, "gone", "stack.yaml"), last_used: "2026-11-21T00:00:00Z" }, { path: path.join(project, "stack.yaml"), last_used: "2026-11-20T00:00:00Z" }] }), { mode: 0o600 });
    const { handlers } = make();
    const listed = await handlers["cli-db-list"]({});
    expect(listed.ok).toBe(true);
    const call = (await calls()).find((c) => c.args[0] === "source" && c.args[1] === "ls");
    expect(call.args).toContain(path.join(project, "stack.yaml"));
  });

  it("falls back to the app's own stack when nothing is registered", async () => {
    await fs.mkdir(path.join(root, "app"), { recursive: true });
    await fs.writeFile(path.join(root, "app", "stack.yaml"), "version: oo.stack/v1\n");
    const { handlers } = make();
    await handlers["cli-db-list"]({});
    expect((await calls()).find((c) => c.args[1] === "ls").args).toContain(path.join(root, "app", "stack.yaml"));
  });

  it("ignores a malformed or foreign registry", async () => {
    await fs.writeFile(path.join(root, "state", "stacks.json"), JSON.stringify({ protocol: "other", stacks: [{ path: "/etc/passwd" }] }));
    const { handlers } = make();
    const kinds = await handlers["cli-db-kinds"]({});
    expect(kinds.stackFile).toBe(path.join(root, "app", "stack.yaml"));
  });
});

describe("the Gateway follows the CLI's active profile", () => {
  async function profile(name) {
    await fs.mkdir(path.join(root, "cfg", "profiles"), { recursive: true });
    await fs.writeFile(path.join(root, "cfg", "active-profile.json"), JSON.stringify({ profile: name }));
    await fs.writeFile(path.join(root, "cfg", "profiles", `${name}.json`), JSON.stringify({ gateway: "http://127.0.0.1:8443/x", token_file: path.join(root, "tok") }));
  }
  it("reads it when the app saved none", async () => {
    await profile("shop");
    const { handlers } = make();
    expect(await handlers["cli-gateway-read"]({})).toMatchObject({ configured: true, url: "http://127.0.0.1:8443" });
  });
  it("adopts the profile of a published build announced by an event, and ignores a failed one", async () => {
    await profile("shop");
    await fs.mkdir(path.join(root, "cfg", "profiles"), { recursive: true });
    await fs.writeFile(path.join(root, "cfg", "profiles", "second.json"), JSON.stringify({ gateway: "http://127.0.0.1:9443/x", token_file: path.join(root, "tok2") }));
    const { handlers, liveHooks } = make();
    liveHooks.onEvent({ type: "db.pipeline_changed", data: { command: "build", ok: false, published: false, profile: "second" } });
    await new Promise((resolve) => { setTimeout(resolve, 50); });
    expect((await handlers["cli-gateway-read"]({})).url).toBe("http://127.0.0.1:8443");
    liveHooks.onEvent({ type: "db.pipeline_changed", data: { command: "build", ok: true, published: true, profile: "second" } });
    await new Promise((resolve) => { setTimeout(resolve, 100); });
    expect((await handlers["cli-gateway-read"]({})).url).toBe("http://127.0.0.1:9443");
  });
});
