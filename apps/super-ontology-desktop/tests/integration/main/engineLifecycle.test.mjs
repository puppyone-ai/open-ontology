import { spawn } from "node:child_process";
import fsSync from "node:fs";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { createSettingsStore } from "../../../electron/main/settings-store.mjs";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createEnginePreflight } from "@so-main/docker-preflight.mjs";
import { createEngineLocator } from "@so-main/engine-locator.mjs";
import { createSuperOntologyService } from "@so-main/service.mjs";
import { createFakeToolbox } from "../../support/fake-engine-tools.mjs";

const quietLogger = { debug() {}, warn() {}, info() {} };

/** Saves a runtime choice in `root/settings.json` unless the file exists, and returns a settings store over it. */
function settingsWith(root, runtime) {
  const file = `${root}/settings.json`;
  if (runtime && !fsSync.existsSync(file)) fsSync.writeFileSync(file, JSON.stringify({ runtime }));
  return createSettingsStore({ file, logger: quietLogger });
}

describe("engine preflight", () => {
  let tools;
  beforeEach(async () => { tools = await createFakeToolbox(); });
  afterEach(async () => { await tools.dispose(); });

  // These cases cover the Docker runtime's checks, so the Docker runtime is the saved choice (the default `auto` only needs `oo`).
  function createService({ platform = "linux", environment = tools.environment(), settings = settingsWith(tools.root, "docker"), ...rest } = {}) {
    const pushes = [];
    const service = createSuperOntologyService({
      settings,
      logger: quietLogger,
      platformHost: { platform },
      paths: { stateDir: tools.root },
      broadcast: (channel, payload) => pushes.push({ channel, payload }),
      readEnvironment: async () => ({ environment }),
      ...rest,
    });
    return { service, pushes };
  }

  const preflight = (service) => service.handlers["engine-preflight"]({}, { sender: {} });

  it("preflight: reports unsupported_platform on Windows without probing anything", async () => {
    const runProbe = vi.fn();
    const { service } = createService({ platform: "windows", runProbe });
    const snapshot = await preflight(service);
    expect(snapshot).toMatchObject({ status: "unsupported_platform", code: "SO_PLATFORM_UNSUPPORTED" });
    expect(snapshot.preflight.platform).toBe("unsupported");
    expect(runProbe).not.toHaveBeenCalled();
  });

  it("preflight: reports oo_missing when neither PATH nor a chosen location has oo", async () => {
    await tools.install("docker", "docker");
    const { service } = createService({ environment: tools.environment({ FAKE_DOCKER_RUNNING: "1" }) });
    const snapshot = await preflight(service);
    expect(snapshot).toMatchObject({ status: "oo_missing", code: "SO_OO_MISSING" });
    expect(snapshot.preflight).toMatchObject({ oo: { found: false, version: null, source: null }, docker: { installed: true, running: true } });
  });

  it("preflight: reports docker_missing when oo exists but docker does not", async () => {
    await tools.install("oo", "oo");
    const { service } = createService();
    const snapshot = await preflight(service);
    expect(snapshot).toMatchObject({ status: "docker_missing", code: "SO_DOCKER_MISSING" });
    expect(snapshot.preflight.oo).toEqual({ found: true, version: "0.3.0", source: "path" });
  });

  it("preflight: reports docker_stopped when the daemon does not answer", async () => {
    await tools.install("oo", "oo");
    await tools.install("docker", "docker");
    const { service } = createService({ environment: tools.environment({ FAKE_DOCKER_RUNNING: "0" }) });
    const snapshot = await preflight(service);
    expect(snapshot).toMatchObject({ status: "docker_stopped", code: "SO_DOCKER_STOPPED" });
    expect(snapshot.preflight.docker).toMatchObject({ installed: true, running: false });
  });

  it("preflight: passes to stopped with versions and free disk, and pushes revisioned snapshots", async () => {
    await tools.install("oo", "oo");
    await tools.install("docker", "docker");
    const { service, pushes } = createService({ environment: tools.environment({ FAKE_DOCKER_RUNNING: "1" }) });
    const snapshot = await preflight(service);
    expect(snapshot).toMatchObject({ status: "stopped", code: null });
    expect(snapshot.preflight).toMatchObject({
      platform: "ok",
      oo: { found: true, version: "0.3.0", source: "path" },
      docker: { installed: true, running: true, version: "27.3.1" },
    });
    expect(snapshot.preflight.diskFreeBytes).toBeGreaterThan(0);
    expect(pushes.map((push) => push.payload.status)).toEqual(["idle", "probing", "stopped"]);
    expect(pushes.every((push) => push.channel === "engine-changed")).toBe(true);
    expect(pushes[1].payload.revision).toBeGreaterThan(pushes[0].payload.revision);
  });

  it("preflight: probes are bounded; a hanging docker is not running and a hanging oo is invalid", async () => {
    await tools.install("oo", "oo-hanging");
    await tools.install("docker", "docker-hanging");
    const locator = createEngineLocator({
      platform: "linux", readEnvironment: async () => ({ environment: tools.environment() }), probeTimeoutMs: 300,
    });
    const run = createEnginePreflight({ platform: "linux", locator, timeoutMs: 300, getRuntimeChoice: () => "docker" });
    const started = Date.now();
    const result = await run.run({ stateDir: tools.root });
    expect(Date.now() - started).toBeLessThan(5_000);
    expect(result).toMatchObject({ status: "oo_missing", code: "SO_OO_INVALID" });
    await tools.install("oo", "oo");
    const second = await run.run({ stateDir: tools.root });
    expect(second).toMatchObject({ status: "docker_stopped" });
  });

  it("preflight: concurrent calls share one pass", async () => {
    await tools.install("oo", "oo");
    await tools.install("docker", "docker");
    const readEnvironment = vi.fn(async () => ({ environment: tools.environment({ FAKE_DOCKER_RUNNING: "1" }) }));
    const { service } = createService({ readEnvironment });
    const [a, b] = await Promise.all([preflight(service), preflight(service)]);
    expect(a.revision).toBe(b.revision);
    expect(readEnvironment).toHaveBeenCalledTimes(1);
  });

  it("preflight: engine-read never probes", async () => {
    const readEnvironment = vi.fn(async () => ({ environment: tools.environment() }));
    const { service } = createService({ readEnvironment });
    expect((await service.handlers["engine-read"]({}, {})).status).toBe("idle");
    expect(readEnvironment).not.toHaveBeenCalled();
  });

  describe("preflight: choosing the oo location", () => {
    const sender = {};
    function withPicker(selected) {
      return createService({ pickPath: async ({ purpose }) => (purpose === "oo-executable" ? selected : null) });
    }

    it("preflight: pick-path hands out a grant and a display path, never more", async () => {
      const chosen = await tools.installElsewhere("oo", "oo");
      const { service } = withPicker(chosen);
      const picked = await service.handlers["pick-path"]({ purpose: "oo-executable" }, { sender });
      expect(Object.keys(picked).sort()).toEqual(["displayPath", "grantId"]);
      expect(picked.displayPath).toBe(chosen);
    });

    it("preflight: a cancelled dialog yields cancelled and issues no grant", async () => {
      const { service } = withPicker(null);
      expect(await service.handlers["pick-path"]({ purpose: "oo-executable" }, { sender })).toEqual({ cancelled: true });
    });

    it("preflight: engine-set-executable accepts a working oo once and re-runs the checks", async () => {
      await tools.install("docker", "docker");
      const chosen = await tools.installElsewhere("oo", "oo");
      const { service } = createService({
        environment: tools.environment({ FAKE_DOCKER_RUNNING: "1" }),
        pickPath: async () => chosen,
      });
      const { grantId } = await service.handlers["pick-path"]({ purpose: "oo-executable" }, { sender });
      const snapshot = await service.handlers["engine-set-executable"]({ grantId }, { sender });
      expect(snapshot).toMatchObject({ status: "stopped" });
      expect(snapshot.preflight.oo).toEqual({ found: true, version: "0.3.0", source: "user" });
      await expect(service.handlers["engine-set-executable"]({ grantId }, { sender })).rejects.toMatchObject({ code: "SO_GRANT_REQUIRED" });
    });

    it("preflight: an executable that cannot report a version is rejected as SO_OO_INVALID", async () => {
      const broken = await tools.installElsewhere("oo", "oo-broken");
      const { service } = withPicker(broken);
      const { grantId } = await service.handlers["pick-path"]({ purpose: "oo-executable" }, { sender });
      await expect(service.handlers["engine-set-executable"]({ grantId }, { sender })).rejects.toMatchObject({ code: "SO_OO_INVALID" });
    });

    it("preflight: a grant is bound to its window and its purpose", async () => {
      const chosen = await tools.installElsewhere("oo", "oo");
      const { service } = withPicker(chosen);
      const { grantId } = await service.handlers["pick-path"]({ purpose: "oo-executable" }, { sender });
      await expect(service.handlers["engine-set-executable"]({ grantId }, { sender: {} })).rejects.toMatchObject({ code: "SO_GRANT_REQUIRED" });
      await expect(service.handlers["engine-set-executable"]({ grantId: "00000000-0000-4000-8000-000000000000" }, { sender })).rejects.toMatchObject({ code: "SO_GRANT_REQUIRED" });
    });

    it("preflight: the chosen location survives a restart through the settings file", async () => {
      await tools.install("docker", "docker");
      const chosen = await tools.installElsewhere("oo", "oo");
      const first = createService({ environment: tools.environment({ FAKE_DOCKER_RUNNING: "1" }), pickPath: async () => chosen, settings: settingsWith(tools.root, "docker") });
      const { grantId } = await first.service.handlers["pick-path"]({ purpose: "oo-executable" }, { sender });
      await first.service.handlers["engine-set-executable"]({ grantId }, { sender });
      const second = createService({ environment: tools.environment({ FAKE_DOCKER_RUNNING: "1" }), settings: settingsWith(tools.root, "docker") });
      const snapshot = await second.service.handlers["engine-preflight"]({}, { sender });
      expect(snapshot.preflight.oo.source).toBe("user");
    });
  });
});

// ---- engine lifecycle (F-S2-02) ------------------------------------------

describe("engine lifecycle", () => {
  let tools;
  let service;
  let env;
  let pushes;
  let warnings;
  const stateDirOf = () => `${tools.root}/state`;
  const sender = {};

  beforeEach(async () => {
    tools = await createFakeToolbox();
    await tools.install("docker", "docker");
    await tools.install("oo", "fake-oo");
    env = tools.environment({ FAKE_DOCKER_RUNNING: "1" });
    pushes = [];
    warnings = [];
    service = undefined;
  });
  afterEach(async () => {
    await service?.shutdown();
    await killLeftoverDaemon(stateDirOf());
    await tools.dispose();
  });

  async function killLeftoverDaemon(stateDir) {
    try {
      const { pid } = JSON.parse(await readFile(`${stateDir}/serve.json`, "utf8"));
      process.kill(pid, "SIGKILL");
    } catch { /* Nothing left. */ }
  }

  function create(extra = {}) {
    service = createSuperOntologyService({
      settings: settingsWith(tools.root, "docker"),
      logger: { debug() {}, info() {}, warn: (...entry) => warnings.push(entry) },
      platformHost: { platform: "linux" },
      paths: { stateDir: stateDirOf(), logsDir: `${tools.root}/logs` },
      broadcast: (channel, payload) => pushes.push({ channel, payload }),
      readEnvironment: async () => ({ environment: env }),
      lifecycleTuning: { pollIntervalMs: 40, healthIntervalMs: 60, stopGraceMs: 1_500, startTimeoutMs: 8_000, backoffMs: [60, 60, 60] },
      ...extra,
    });
    return service;
  }

  const call = (channel, request = {}) => service.handlers[channel](request, { sender });
  const current = () => service.handlers["engine-read"]();
  const statuses = () => pushes.map((push) => push.payload.status).filter((status, index, all) => status !== all[index - 1]);
  const invocations = async () => (await readFile(`${stateDirOf()}/fake-oo-invocations.jsonl`, "utf8")).trim().split("\n").map((line) => JSON.parse(line));

  async function waitFor(predicate, { timeout = 10_000, label = "condition" } = {}) {
    const deadline = Date.now() + timeout;
    for (;;) {
      const snapshot = await current();
      if (predicate(snapshot)) return snapshot;
      if (Date.now() > deadline) throw new Error(`Timed out waiting for ${label}; last status ${snapshot.status} ${snapshot.code} ${snapshot.reason}`);
      await new Promise((resolve) => { setTimeout(resolve, 25); });
    }
  }
  const untilStatus = (status, options) => waitFor((snapshot) => snapshot.status === status, { label: status, ...options });

  it("starts the daemon, waits for credentials and reaches healthy without leaking the token", async () => {
    create();
    const started = await call("engine-start");
    expect(started.status).toBe("starting");
    const healthy = await untilStatus("healthy");
    expect(healthy).toMatchObject({ ownedByDesktop: true, code: null, reason: null });
    expect(healthy.health).toMatchObject({ status: "ok", api_version: "1.0" });
    expect(healthy.startedAt).toEqual(expect.any(String));
    expect(statuses()).toEqual(expect.arrayContaining(["probing", "stopped", "starting", "healthy"]));
    const [serveCall] = await invocations();
    expect(serveCall).toEqual(["serve", "--state-dir", stateDirOf(), "--port", "0", "--json", "--runtime", "compose"]);

    const token = (await readFile(`${stateDirOf()}/serve.token`, "utf8")).trim();
    expect(JSON.stringify(pushes)).not.toContain(token);
    expect(JSON.stringify(pushes)).not.toContain("oodt_");
    expect(JSON.stringify(warnings)).not.toContain(token);
    expect(JSON.stringify(healthy)).not.toContain("serve.token");
    expect(service.internals.lifecycle.credentials()).toMatchObject({ baseUrl: expect.stringMatching(/^http:\/\/127\.0\.0\.1:\d+$/u), token });
  });

  it("passes the exchange directory to the engine on macOS only (host media decoding, F-S10-05)", async () => {
    create({ platformHost: { platform: "macos" } });
    await call("engine-start");
    await untilStatus("healthy");
    const lines = (await readFile(`${stateDirOf()}/fake-oo-exchange.jsonl`, "utf8")).trim().split("\n").map((line) => JSON.parse(line));
    expect(lines.at(-1)).toBe(`${stateDirOf()}/exchange`);
  });

  it("does not pass an exchange directory on Linux", async () => {
    create();
    await call("engine-start");
    await untilStatus("healthy");
    const lines = (await readFile(`${stateDirOf()}/fake-oo-exchange.jsonl`, "utf8")).trim().split("\n").map((line) => JSON.parse(line));
    expect(lines.at(-1)).toBeNull();
  });

  it("passes --keep-engine when asked and persists the choice", async () => {
    create();
    await call("engine-start", { keepEngineOnQuit: true });
    await untilStatus("healthy");
    expect((await invocations()).at(-1)).toContain("--keep-engine");
    expect((await current()).keepEngineOnQuit).toBe(true);
  });

  it("reuses a live instance without starting another and refuses to stop it", async () => {
    await mkdir(stateDirOf(), { recursive: true, mode: 0o700 });
    const external = spawn(`${tools.bin}/oo`, ["serve", "--state-dir", stateDirOf(), "--port", "0", "--json"], { stdio: "ignore", env });
    await vi.waitFor(async () => { await readFile(`${stateDirOf()}/serve.json`, "utf8"); }, { timeout: 5_000 });
    try {
      create();
      await call("engine-start");
      const healthy = await untilStatus("healthy");
      expect(healthy.ownedByDesktop).toBe(false);
      const serveStarts = (await invocations()).filter((entry) => entry[0] === "serve" && !entry.includes("--status"));
      expect(serveStarts).toHaveLength(1);
      await expect(call("engine-stop")).rejects.toMatchObject({ code: "SO_ENGINE_NOT_OWNED" });
      await service.shutdown();
      expect(() => process.kill(external.pid, 0)).not.toThrow();
    } finally {
      external.kill("SIGKILL");
    }
  });

  it.each([
    ["2", "incompatible", "SO_ENGINE_INCOMPATIBLE"],
    ["3", "docker_stopped", "DOCS_ENGINE_DOCKER_UNAVAILABLE"],
    ["4", "crashed", "SO_SERVE_IMAGE_INVALID"],
    ["5", "crashed", "SO_SERVE_STATE_DIR_INVALID"],
    ["7", "crashed", "SO_SERVE_START_TIMEOUT"],
  ])("maps oo serve exit code %s to %s (%s)", async (exitCode, status, code) => {
    env.FAKE_OO_MODE = `exit-${exitCode}`;
    create();
    await call("engine-start");
    const snapshot = await untilStatus(status);
    expect(snapshot).toMatchObject({ code, ownedByDesktop: false });
    // No automatic restart: the state stays put.
    await new Promise((resolve) => { setTimeout(resolve, 300); });
    expect((await current()).status).toBe(status);
  });

  it("retries once with another port after exit code 6, then gives up", async () => {
    env.FAKE_OO_MODE = "exit-6-once";
    create();
    await call("engine-start");
    await untilStatus("healthy");
    const calls = await invocations();
    expect(calls).toHaveLength(2);
    expect(calls[0][calls[0].indexOf("--port") + 1]).toBe("0");
    expect(calls[1][calls[1].indexOf("--port") + 1]).not.toBe("0");

    await service.shutdown();
    await rm(stateDirOf(), { recursive: true, force: true });
    env.FAKE_OO_MODE = "exit-6";
    create();
    await call("engine-start");
    expect(await untilStatus("crashed")).toMatchObject({ code: "SO_SERVE_PORT_IN_USE" });
    expect(await invocations()).toHaveLength(2);
  });

  it("rejects credentials with loose permissions as SO_SERVE_CREDENTIALS_INVALID and stops the daemon", async () => {
    env.FAKE_OO_BAD_PERMS = "1";
    create();
    await call("engine-start");
    const snapshot = await untilStatus("crashed");
    expect(snapshot.code).toBe("SO_SERVE_CREDENTIALS_INVALID");
    await vi.waitFor(async () => { await readFile(`${stateDirOf()}/fake-oo-terminated`, "utf8"); }, { timeout: 5_000 });
  });

  it("times out when serve.json never appears and kills the process", async () => {
    env.FAKE_OO_START_DELAY_MS = "60000";
    create({ lifecycleTuning: { pollIntervalMs: 30, startTimeoutMs: 400, stopGraceMs: 500 } });
    await call("engine-start");
    expect(await untilStatus("crashed")).toMatchObject({ code: "SO_SERVE_START_TIMEOUT" });
  });

  it("removes a stale serve.json left by a killed daemon before starting", async () => {
    await mkdir(stateDirOf(), { recursive: true, mode: 0o700 });
    await writeFile(`${stateDirOf()}/serve.json`, JSON.stringify({ protocol: "oo.docs.serve/v1", pid: 2147483000, port: 9, api_version: "1.0", token_file: `${stateDirOf()}/serve.token` }), { mode: 0o600 });
    create();
    await call("engine-start");
    const healthy = await untilStatus("healthy");
    expect(healthy.ownedByDesktop).toBe(true);
  });

  it("refuses to start when the preflight fails, with the precise error code", async () => {
    await rm(`${tools.bin}/docker`);
    create();
    await expect(call("engine-start")).rejects.toMatchObject({ code: "SO_DOCKER_MISSING" });
    expect((await current()).status).toBe("docker_missing");
  });

  describe("health patrol", () => {
    const setHealth = (control) => writeFile(`${stateDirOf()}/fake-health.json`, JSON.stringify(control));
    const okBody = { api_version: "1.0", engine_version: "0.3.0", status: "ok", components: {}, queue: {}, events_head: 0 };

    async function startHealthy() {
      create();
      await call("engine-start");
      await untilStatus("healthy");
    }

    it("shows degraded and recovers", async () => {
      await startHealthy();
      await setHealth({ body: { ...okBody, status: "degraded", components: { parser: { status: "unavailable" } } } });
      const degraded = await untilStatus("degraded");
      expect(degraded.health.components.parser.status).toBe("unavailable");
      await rm(`${stateDirOf()}/fake-health.json`);
      await untilStatus("healthy");
    });

    it("treats an unreachable engine container as restarting(engine_container) and recovers", async () => {
      await startHealthy();
      await setHealth({ http: 503, body: { code: "DOCS_ENGINE_UNAVAILABLE", message: "x", retryable: true, details: {} } });
      expect(await untilStatus("restarting")).toMatchObject({ reason: "engine_container", code: "DOCS_ENGINE_UNAVAILABLE" });
      await rm(`${stateDirOf()}/fake-health.json`);
      expect(await untilStatus("healthy")).toMatchObject({ reason: null, code: null });
    });

    it("shows applying-drive-changes while the container is rebuilt, then returns to healthy", async () => {
      await startHealthy();
      service.internals.lifecycle.beginDriveChange();
      expect(await current()).toMatchObject({ status: "restarting", reason: "drive_change", code: null });
      await setHealth({ http: 503, body: { code: "DOCS_ENGINE_UNAVAILABLE", message: "x", retryable: true, details: {} } });
      await new Promise((resolve) => { setTimeout(resolve, 300); });
      expect(await current()).toMatchObject({ status: "restarting", reason: "drive_change" });
      await rm(`${stateDirOf()}/fake-health.json`);
      expect(await untilStatus("healthy")).toMatchObject({ reason: null, code: null });
    });

    it("maps DOCS_ENGINE_STARTING to starting", async () => {
      await startHealthy();
      await setHealth({ http: 503, body: { code: "DOCS_ENGINE_STARTING", message: "x", retryable: true, details: {} } });
      await untilStatus("starting");
    });

    it.each([
      ["schema mismatch", { http: 503, body: { code: "DOCS_ENGINE_SCHEMA_MISMATCH", message: "x", retryable: false, details: {} } }, "DOCS_ENGINE_SCHEMA_MISMATCH"],
      ["major version header 2", { http: 200, headers: { "OO-Docs-Api": "2.0" }, body: okBody }, "SO_ENGINE_INCOMPATIBLE"],
    ])("maps %s to incompatible", async (_name, control, code) => {
      await startHealthy();
      await setHealth(control);
      expect(await untilStatus("incompatible")).toMatchObject({ code });
    });
  });

  describe("crashes and backoff", () => {
    it("restarts with backoff after unexpected exits and ends healthy", async () => {
      env.FAKE_OO_CRASH_FIRST = "2";
      create();
      await call("engine-start");
      const restarting = await untilStatus("restarting");
      expect(restarting).toMatchObject({ reason: "process_exit", restarts: { count: 1, nextAttemptAt: expect.any(String) } });
      const healthy = await waitFor((snapshot) => snapshot.status === "healthy" && snapshot.restarts.count === 2, { label: "healthy after 2 restarts" });
      expect(healthy.restarts.nextAttemptAt).toBeNull();
      expect((await invocations()).filter((entry) => !entry.includes("--status"))).toHaveLength(3);
    });

    it("gives up after 3 restarts inside the window and needs a manual retry", async () => {
      env.FAKE_OO_CRASH_ALWAYS = "1";
      create();
      await call("engine-start");
      const crashed = await untilStatus("crashed", { timeout: 15_000 });
      expect(crashed).toMatchObject({ code: "SO_ENGINE_UNREACHABLE", reason: "restart_limit", restarts: { count: 3 } });
      expect((await invocations()).filter((entry) => !entry.includes("--status"))).toHaveLength(4);
      delete env.FAKE_OO_CRASH_ALWAYS;
      await call("engine-start");
      expect(await untilStatus("healthy")).toMatchObject({ restarts: { count: 0 } });
    });

    it("forgets old failures once they leave the 10 minute window", async () => {
      env.FAKE_OO_CRASH_FIRST = "5";
      create({ lifecycleTuning: { pollIntervalMs: 30, healthIntervalMs: 60, backoffMs: [60, 60, 60], restartWindowMs: 20, stopGraceMs: 1_500 } });
      await call("engine-start");
      await waitFor((snapshot) => snapshot.status === "healthy" && snapshot.restarts.count === 5, { timeout: 15_000, label: "5 restarts survived" });
    });
  });

  describe("stop, restart and quit", () => {
    it("stops the child with SIGTERM and reports stopped", async () => {
      create();
      await call("engine-start");
      await untilStatus("healthy");
      const stopped = await call("engine-stop");
      expect(stopped).toMatchObject({ status: "stopped", ownedByDesktop: false, health: null });
      await readFile(`${stateDirOf()}/fake-oo-terminated`, "utf8");
      expect(statuses()).toContain("stopping");
      expect(service.internals.lifecycle.credentials()).toBeNull();
    });

    it("escalates to SIGKILL when the daemon ignores SIGTERM", async () => {
      env.FAKE_OO_IGNORE_TERM = "1";
      create({ lifecycleTuning: { pollIntervalMs: 30, healthIntervalMs: 60, stopGraceMs: 300 } });
      await call("engine-start");
      await untilStatus("healthy");
      const { pid } = JSON.parse(await readFile(`${stateDirOf()}/serve.json`, "utf8"));
      const started = Date.now();
      await call("engine-stop");
      expect(Date.now() - started).toBeLessThan(5_000);
      await vi.waitFor(() => { expect(() => process.kill(pid, 0)).toThrow(); }, { timeout: 3_000 });
    });

    it("restart stops and starts a new instance", async () => {
      create();
      await call("engine-start");
      await untilStatus("healthy");
      const first = JSON.parse(await readFile(`${stateDirOf()}/serve.json`, "utf8")).pid;
      const restarted = await call("engine-restart");
      expect(restarted.status).toBe("starting");
      await untilStatus("healthy");
      expect(JSON.parse(await readFile(`${stateDirOf()}/serve.json`, "utf8")).pid).not.toBe(first);
    });

    it("sends SIGTERM synchronously on shutdown", async () => {
      create();
      await call("engine-start");
      await untilStatus("healthy");
      const { pid } = JSON.parse(await readFile(`${stateDirOf()}/serve.json`, "utf8"));
      const done = service.shutdown();
      // The signal is out before the first await resolves.
      await vi.waitFor(() => { expect(() => process.kill(pid, 0)).toThrow(); }, { timeout: 3_000 });
      await done;
      await readFile(`${stateDirOf()}/fake-oo-terminated`, "utf8");
    });

    it("keeps the engine running on quit when the option is on", async () => {
      create();
      await call("engine-start", { keepEngineOnQuit: true });
      await untilStatus("healthy");
      const { pid } = JSON.parse(await readFile(`${stateDirOf()}/serve.json`, "utf8"));
      await service.shutdown();
      expect(() => process.kill(pid, 0)).not.toThrow();
      await expect(readFile(`${stateDirOf()}/fake-oo-terminated`, "utf8")).rejects.toThrow();
      process.kill(pid, "SIGKILL");
    });
  });

  it("reveals the log file and the state directory through the platform shell", async () => {
    const revealed = [];
    create({ revealPath: (target) => revealed.push(target) });
    expect(await call("engine-reveal-logs")).toEqual({ revealed: true });
    expect(await call("engine-reveal-logs", { target: "state" })).toEqual({ revealed: true });
    expect(revealed).toEqual([`${tools.root}/logs/super-ontology-serve.log`, stateDirOf()]);
  });
});

// F-S14-01: the Docker-free runtime. `oo` is the fake one; the embedded runtime is a folder with the layout of lite-runtime.mjs.
describe("lite runtime (Docker-free, F-S14-01)", () => {
  let tools;
  let service;
  let liteDirectory;
  let probes;
  const stateDirOf = () => `${tools.root}/state`;
  const sender = {};

  async function makeLiteRuntime({ postgres = true, sidecar = true, manifest = true } = {}) {
    liteDirectory = `${tools.root}/lite`;
    await mkdir(`${liteDirectory}/postgres/bin`, { recursive: true });
    if (postgres) for (const name of ["postgres", "initdb", "pg_ctl"]) await writeFile(`${liteDirectory}/postgres/bin/${name}`, "#!/bin/sh\nexit 0\n", { mode: 0o755 });
    if (sidecar) await mkdir(`${liteDirectory}/sidecar`, { recursive: true });
    if (manifest) await writeFile(`${liteDirectory}/runtime.json`, JSON.stringify({ protocol: "oo.lite-runtime/v1", postgres: "17.4" }));
  }

  beforeEach(async () => { tools = await createFakeToolbox(); await tools.install("oo", "fake-oo"); probes = []; service = undefined; });
  afterEach(async () => {
    await service?.shutdown();
    try { process.kill(JSON.parse(await readFile(`${stateDirOf()}/serve.json`, "utf8")).pid, "SIGKILL"); } catch { /* Nothing left. */ }
    await tools.dispose();
  });

  function create({ choice = "auto", dockerRunning = false, ...extra } = {}) {
    service = createSuperOntologyService({
      settings: settingsWith(tools.root, choice === "auto" ? null : choice),
      logger: { debug() {}, info() {}, warn() {} },
      // Linux has no fallback directories for the command search, so a Docker on this machine cannot leak into the test.
      platformHost: { platform: "linux" },
      paths: { stateDir: stateDirOf(), logsDir: `${tools.root}/logs`, liteRuntimeDir: liteDirectory },
      readEnvironment: async () => ({ environment: tools.environment(dockerRunning ? { FAKE_DOCKER_RUNNING: "1" } : {}) }),
      runProbe: async (...call) => { probes.push(call); return (await import("../../../electron/main/platform/common/bounded-process-probe.mjs")).runBoundedProcessProbe(...call); },
      lifecycleTuning: { pollIntervalMs: 40, healthIntervalMs: 60, stopGraceMs: 1_500, startTimeoutMs: 8_000, backoffMs: [60, 60, 60] },
      ...extra,
    });
    return service;
  }
  const call = (channel, request = {}) => service.handlers[channel](request, { sender });
  const dockerProbed = () => probes.some(([executable]) => /docker$/u.test(String(executable)));
  async function untilStatus(status) {
    for (let attempt = 0; attempt < 400; attempt += 1) {
      const snapshot = await call("engine-read");
      if (snapshot.status === status) return snapshot;
      await new Promise((resolve) => { setTimeout(resolve, 25); });
    }
    throw new Error(`Timed out waiting for ${status}`);
  }

  it("lite: the embedded runtime is ready to start with no Docker row and no word about Docker", async () => {
    await makeLiteRuntime();
    create({ choice: "lite" });
    const snapshot = await call("engine-preflight");
    expect(snapshot).toMatchObject({ status: "stopped", code: null });
    expect(snapshot.preflight).toMatchObject({
      runtime: "lite", runtimeChoice: "lite", docker: { installed: false, running: false, version: null },
      embedded: { found: true, postgres: { found: true, version: "17.4" }, sidecar: { found: true } },
    });
    expect(JSON.stringify(snapshot)).not.toMatch(/docker_(missing|stopped)/u);
    expect(dockerProbed()).toBe(false);
  });

  it("lite: starts `oo serve --runtime embedded` and reaches healthy without Docker, from install to engine with no terminal", async () => {
    await makeLiteRuntime();
    create({ choice: "lite" });
    await call("engine-start");
    const healthy = await untilStatus("healthy");
    expect(healthy.preflight.runtime).toBe("lite");
    const [serveCall] = (await readFile(`${stateDirOf()}/fake-oo-invocations.jsonl`, "utf8")).trim().split("\n").map((line) => JSON.parse(line));
    expect(serveCall).toEqual(["serve", "--state-dir", stateDirOf(), "--port", "0", "--json", "--runtime", "embedded"]);
  });

  it("auto: needs only oo, probes neither Docker nor the embedded runtime and lets `oo serve` pick", async () => {
    await makeLiteRuntime();
    await tools.install("docker", "docker");
    create({ dockerRunning: true });
    const snapshot = await call("engine-preflight");
    expect(snapshot).toMatchObject({ status: "stopped", code: null });
    expect(snapshot.preflight).toMatchObject({ runtime: "auto", runtimeChoice: "auto", embedded: null, docker: { installed: false, running: false } });
    expect(dockerProbed()).toBe(false);
    await call("engine-start");
    await untilStatus("healthy");
    const [serveCall] = (await readFile(`${stateDirOf()}/fake-oo-invocations.jsonl`, "utf8")).trim().split("\n").map((line) => JSON.parse(line));
    expect(serveCall).toEqual(["serve", "--state-dir", stateDirOf(), "--port", "0", "--json", "--runtime", "auto"]);
  });

  it("docker: choosing Docker without Docker keeps the Docker status", async () => {
    create({ choice: "docker" });
    const snapshot = await call("engine-preflight");
    expect(snapshot).toMatchObject({ status: "docker_missing", code: "SO_DOCKER_MISSING" });
    expect(snapshot.preflight.runtime).toBe("docker");
  });

  it("lite: choosing the embedded runtime never looks at Docker, and a build without it reports embedded_missing and refuses to start", async () => {
    await makeLiteRuntime({ postgres: false });
    await tools.install("docker", "docker");
    create({ dockerRunning: true });
    const set = await call("engine-set-runtime", { runtime: "lite" });
    expect(set).toMatchObject({ status: "embedded_missing", code: "SO_EMBEDDED_RUNTIME_MISSING" });
    expect(set.preflight).toMatchObject({ runtime: "lite", runtimeChoice: "lite", embedded: { found: false } });
    expect(dockerProbed()).toBe(false);
    await expect(call("engine-start")).rejects.toMatchObject({ code: "SO_EMBEDDED_RUNTIME_MISSING" });
  });

  it("lite: choosing Docker never looks at the embedded runtime; the choice is saved and read back after a restart of the app", async () => {
    await makeLiteRuntime();
    await tools.install("docker", "docker");
    create({ dockerRunning: true });
    const set = await call("engine-set-runtime", { runtime: "docker" });
    expect(set.preflight).toMatchObject({ runtime: "docker", runtimeChoice: "docker", embedded: null });
    expect(JSON.parse(await readFile(`${tools.root}/settings.json`, "utf8")).runtime).toBe("docker");
    await service.shutdown();
    create({ dockerRunning: true });
    expect((await call("engine-preflight")).preflight.runtimeChoice).toBe("docker");
  });

  it("lite: a folder that is not a runtime is not found, and a broken manifest only hides the version", async () => {
    const { createLiteRuntimeProbe } = await import("@so-main/lite-runtime.mjs");
    expect(await createLiteRuntimeProbe({ directory: null }).probe()).toMatchObject({ found: false });
    expect(await createLiteRuntimeProbe({ directory: "relative/dir" }).probe()).toMatchObject({ found: false });
    await makeLiteRuntime({ manifest: false });
    expect(await createLiteRuntimeProbe({ directory: liteDirectory }).probe()).toEqual({ found: true, postgres: { found: true, version: null }, sidecar: { found: true } });
    await writeFile(`${liteDirectory}/runtime.json`, "not json");
    expect((await createLiteRuntimeProbe({ directory: liteDirectory }).probe()).postgres.version).toBeNull();
    await rm(`${liteDirectory}/sidecar`, { recursive: true });
    expect((await createLiteRuntimeProbe({ directory: liteDirectory }).probe()).sidecar.found).toBe(false);
  });
});
