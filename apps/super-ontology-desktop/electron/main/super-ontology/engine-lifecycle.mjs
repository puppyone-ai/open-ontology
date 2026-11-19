import { spawn as nodeSpawn } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import { SuperOntologyError } from "../../../src/shared/super-ontology/errors.mjs";
import { runBoundedProcessProbe } from "../platform/common/bounded-process-probe.mjs";
import { classifyHealth, engineBaseUrl, fetchEngineHealth } from "./engine-health.mjs";
import { readServeCredentials } from "./serve-credentials.mjs";

const RUNTIME_ARGUMENT = Object.freeze({ auto: "auto", docker: "compose", lite: "embedded" });
const SERVE_PROTOCOL = "oo.docs.serve/v1";
const ACTIVE_STATUSES = new Set(["starting", "healthy", "degraded", "restarting", "stopping"]);

/** Defaults follow 20-frontend §1.4.1; tests shrink them. */
export const LIFECYCLE_DEFAULTS = Object.freeze({
  backoffMs: Object.freeze([5_000, 15_000, 45_000]),
  restartWindowMs: 10 * 60_000,
  maxRestarts: 3,
  startTimeoutMs: 150_000,
  pollIntervalMs: 500,
  healthIntervalMs: 10_000,
  stopGraceMs: 12_000,
  statusProbeTimeoutMs: 4_000,
  unreachableLimit: 3,
  driveChangeTimeoutMs: 120_000,
});

/** `oo serve` exit codes (10 §8.6.9) -> terminal state. 1, signals and unknown codes mean "restart". */
const EXIT_OUTCOMES = Object.freeze({
  // A usage error for our fixed arguments: this `oo` does not know `serve` (an older build), so it is not compatible.
  2: { status: "incompatible", code: "SO_ENGINE_INCOMPATIBLE" },
  3: { status: "docker_stopped", code: "DOCS_ENGINE_DOCKER_UNAVAILABLE" },
  4: { status: "crashed", code: "SO_SERVE_IMAGE_INVALID" },
  5: { status: "crashed", code: "SO_SERVE_STATE_DIR_INVALID" },
  7: { status: "crashed", code: "SO_SERVE_START_TIMEOUT" },
});

const PREFLIGHT_ERROR_BY_STATUS = Object.freeze({
  unsupported_platform: "SO_PLATFORM_UNSUPPORTED", oo_missing: "SO_OO_MISSING",
  docker_missing: "SO_DOCKER_MISSING", docker_stopped: "SO_DOCKER_STOPPED", embedded_missing: "SO_EMBEDDED_RUNTIME_MISSING",
});

/**
 * The engine state machine of 20-frontend §1.4.1. It owns the `oo serve`
 * child, reads credentials after every (re)start, patrols health, and is the
 * only writer of the engine part of the snapshot. The control token stays
 * inside this closure (`credentials()` is for the in-process HTTP client).
 */
export function createEngineLifecycle({
  snapshots,
  enginePreflight,
  getUserExecutable = () => null,
  stateDir,
  serveLog,
  spawn = nodeSpawn,
  runProbe = runBoundedProcessProbe,
  fetchHealth = fetchEngineHealth,
  readCredentials = readServeCredentials,
  fsPromises = fs,
  logger = console,
  timers = { setTimeout, clearTimeout, now: () => Date.now() },
  tuning = {},
  randomPort = () => 49_152 + Math.floor(Math.random() * 16_000),
  /** Where the engine finds host decode output (`OO_DOCS_EXCHANGE`, mounted read-only); `null` where no helper exists. */
  exchangeDirectory = null,
} = {}) {
  const t = { ...LIFECYCLE_DEFAULTS, ...tuning };
  let generation = 0;
  let child = null;
  let childExited = null;
  let credentials = null;
  let launchContext = null;
  let keepEngine = false;
  let owned = false;
  let restartTimes = [];
  let totalRestarts = 0;
  let pollTimer = null;
  let restartTimer = null;
  let startDeadline = 0;
  let relaunching = false;
  let portRetried = false;
  let unreachableCount = 0;
  let lastHealthJson = "";
  let preflightInFlight = null;
  let shutdownPromise = null;
  let driveChangeUntil = 0;

  const stale = (gen) => gen !== generation;
  const iso = (ms) => new Date(ms).toISOString();

  function clearTimers() {
    if (pollTimer) timers.clearTimeout(pollTimer);
    if (restartTimer) timers.clearTimeout(restartTimer);
    pollTimer = null;
    restartTimer = null;
  }

  function update(patch) {
    return snapshots.update({ keepEngineOnQuit: keepEngine, ...patch });
  }

  function setTerminal(status, code, extra = {}) {
    clearTimers();
    credentials = null;
    relaunching = false;
    update({
      status, code, reason: null, health: null, ownedByDesktop: false, startedAt: null,
      restarts: { count: totalRestarts, nextAttemptAt: null }, ...extra,
    });
  }

  // ---- preflight --------------------------------------------------------

  async function runPreflight() {
    const previous = snapshots.peek();
    update({ status: "probing", reason: null, code: null });
    try {
      const result = await enginePreflight.run({ userExecutable: getUserExecutable(), stateDir });
      launchContext = { ooPath: result.ooPath ?? null, environment: result.environment ?? null, runtime: result.runtime ?? "docker" };
      update({ status: result.status, code: result.code, reason: null, preflight: result.preflight });
      return snapshots.get();
    } catch (error) {
      logger.warn?.("Super Drive preflight failed.", { name: error?.name });
      update({ status: previous.status, reason: previous.reason, code: previous.code });
      throw new SuperOntologyError("SO_INTERNAL");
    }
  }

  /** Probes only while no engine is active; concurrent callers share one pass. */
  function preflight() {
    if (ACTIVE_STATUSES.has(snapshots.peek().status)) return Promise.resolve(snapshots.get());
    preflightInFlight ??= runPreflight().finally(() => { preflightInFlight = null; });
    return preflightInFlight;
  }

  // ---- start ------------------------------------------------------------

  async function start({ keepEngineOnQuit } = {}) {
    if (shutdownPromise) throw new SuperOntologyError("SO_ENGINE_NOT_RUNNING");
    if (ACTIVE_STATUSES.has(snapshots.peek().status)) return snapshots.get();
    if (typeof keepEngineOnQuit === "boolean") keepEngine = keepEngineOnQuit;
    const checked = await preflight();
    if (checked.status !== "stopped") {
      throw new SuperOntologyError(checked.code ?? PREFLIGHT_ERROR_BY_STATUS[checked.status] ?? "SO_INTERNAL", { retryable: true });
    }
    totalRestarts = 0;
    restartTimes = [];
    portRetried = false;
    launch({ fresh: true });
    return snapshots.get();
  }

  function launch({ fresh }) {
    generation += 1;
    const gen = generation;
    clearTimers();
    unreachableCount = 0;
    lastHealthJson = "";
    startDeadline = timers.now() + t.startTimeoutMs;
    relaunching = !fresh;
    if (fresh) update({ status: "starting", reason: null, code: null, health: null, ownedByDesktop: false, startedAt: null });
    runStart(gen).catch((error) => {
      if (stale(gen)) return;
      logger.warn?.("Super Drive engine start failed.", { name: error?.name, code: error?.code });
      const code = error instanceof SuperOntologyError ? error.code : "SO_INTERNAL";
      setTerminal("crashed", code);
    });
  }

  async function runStart(gen) {
    if (!launchContext?.ooPath) throw new SuperOntologyError("SO_OO_MISSING");
    if (await attachExisting(gen) || stale(gen)) return;
    await spawnEngine(gen, portRetried ? randomPort() : 0);
  }

  /** Reuse a live instance (`ownedByDesktop=false`): `oo serve --status --json`. */
  async function attachExisting(gen) {
    let printed;
    try {
      const result = await runProbe(launchContext.ooPath, ["serve", "--status", "--json", "--state-dir", stateDir], {
        env: launchContext.environment, timeoutMs: t.statusProbeTimeoutMs, maxOutputBytes: 16 * 1024, label: "oo serve status",
      });
      if (result.code !== 0) return false;
      printed = JSON.parse(result.stdout);
    } catch {
      return false;
    }
    if (stale(gen) || printed?.protocol !== SERVE_PROTOCOL) return false;
    const found = await readCredentials({ stateDir, fsPromises });
    if (stale(gen)) return true;
    if (!found) return false;
    if (found.info.apiMajor !== 1) {
      setTerminal("incompatible", "SO_ENGINE_INCOMPATIBLE");
      return true;
    }
    credentials = found;
    owned = false;
    update({ ownedByDesktop: false, startedAt: found.info.startedAt });
    schedulePoll(gen, 0);
    return true;
  }

  async function removeStaleServeFiles() {
    // `--status` found no live instance, so these belong to a killed daemon (10 §8.6.8).
    await fsPromises.rm(path.join(stateDir, "serve.json"), { force: true }).catch(() => {});
  }

  async function spawnEngine(gen, port) {
    await fsPromises.mkdir(stateDir, { recursive: true, mode: 0o700 });
    if (exchangeDirectory) await fsPromises.mkdir(exchangeDirectory, { recursive: true, mode: 0o700 });
    await removeStaleServeFiles();
    // The Docker-free runtime (F-S14-01): the engine runs as a child of `oo serve`, with no compose project and no container.
    const args = ["serve", "--state-dir", stateDir, "--port", String(port), "--json", "--runtime", RUNTIME_ARGUMENT[launchContext.runtime] ?? "auto", ...(keepEngine ? ["--keep-engine"] : [])];
    const handle = await serveLog.open();
    let process;
    try {
      process = spawn(launchContext.ooPath, args, {
        env: exchangeDirectory ? { ...launchContext.environment, OO_DOCS_EXCHANGE: exchangeDirectory } : launchContext.environment, stdio: ["ignore", handle.fd, handle.fd], windowsHide: true,
      });
    } catch {
      throw new SuperOntologyError("SO_OO_INVALID");
    } finally {
      await handle.close().catch(() => {});
    }
    child = process;
    owned = true;
    let resolveExit;
    childExited = new Promise((resolve) => { resolveExit = resolve; });
    let exitSeen = false;
    const onExit = (code, signal) => {
      if (exitSeen) return;
      exitSeen = true;
      if (child === process) { child = null; }
      resolveExit({ code, signal });
      handleExit(gen, code, signal);
    };
    process.once("exit", onExit);
    process.once("error", () => onExit(1, null));
    update({ ownedByDesktop: true });
    await waitForCredentials(gen, process);
  }

  async function waitForCredentials(gen, process) {
    while (!stale(gen) && child === process) {
      let found;
      try {
        found = await readCredentials({ stateDir, fsPromises });
      } catch (error) {
        if (stale(gen)) return;
        terminate(process).catch(() => {});
        setTerminal("crashed", error instanceof SuperOntologyError ? error.code : "SO_SERVE_CREDENTIALS_INVALID", { ownedByDesktop: true });
        generation += 1;
        return;
      }
      if (found) {
        credentials = found;
        update({ startedAt: found.info.startedAt });
        schedulePoll(gen, 0);
        return;
      }
      if (timers.now() > startDeadline) {
        terminate(process).catch(() => {});
        generation += 1;
        setTerminal("crashed", "SO_SERVE_START_TIMEOUT");
        return;
      }
      await new Promise((resolve) => { timers.setTimeout(resolve, t.pollIntervalMs); });
    }
  }

  // ---- health patrol ----------------------------------------------------

  function schedulePoll(gen, delay) {
    if (pollTimer) timers.clearTimeout(pollTimer);
    pollTimer = timers.setTimeout(() => { pollTimer = null; void pollHealth(gen); }, delay);
    pollTimer.unref?.();
  }

  async function pollHealth(gen) {
    if (stale(gen) || !credentials) return;
    let outcome;
    try {
      outcome = classifyHealth(await fetchHealth({ port: credentials.info.port, baseUrl: credentials.baseUrl ?? null, token: credentials.token }));
    } catch {
      outcome = { kind: "unreachable" };
    }
    if (stale(gen)) return;
    if (outcome.kind === "unauthenticated") {
      // The token rotates on every daemon start: re-read once, then give up for this round.
      if (credentials.baseUrl) { generation += 1; setTerminal("crashed", "SO_SERVE_CREDENTIALS_INVALID", { mode: "remote" }); return; }
      const before = credentials.token;
      credentials = await readCredentials({ stateDir, fsPromises }).catch(() => null) ?? credentials;
      if (stale(gen)) return;
      if (credentials.token !== before) { schedulePoll(gen, 0); return; }
    }
    applyOutcome(gen, outcome);
    if (!stale(gen) && pollTimer === null) {
      void serveLog.rotateIfNeeded().catch(() => {});
      schedulePoll(gen, ["starting", "restarting"].includes(snapshots.peek().status) ? t.pollIntervalMs : t.healthIntervalMs);
    }
  }

  function setRunning(status, health) {
    driveChangeUntil = 0;
    unreachableCount = 0;
    relaunching = false;
    const json = JSON.stringify(health ?? null);
    const current = snapshots.peek();
    if (current.status === status && json === lastHealthJson && current.code === null) return;
    lastHealthJson = json;
    update({ status, reason: null, code: null, health: health ?? null, ownedByDesktop: owned,
      restarts: { count: totalRestarts, nextAttemptAt: null } });
  }

  function applyOutcome(gen, outcome) {
    const current = snapshots.peek();
    // After a local Drive change the daemon rebuilds the engine container: stay on
    // "applying Drive changes" until health answers again (20-frontend §1.4.1).
    if (timers.now() < driveChangeUntil) {
      if (["starting", "container", "unreachable", "unauthenticated"].includes(outcome.kind)) return;
    }
    switch (outcome.kind) {
      case "ok": setRunning("healthy", outcome.health); return;
      case "degraded": setRunning("degraded", outcome.health); return;
      case "starting":
        if (current.status === "starting" || relaunching) {
          if (timers.now() > startDeadline) startTimedOut(gen);
          return;
        }
        update({ status: "starting", reason: null, code: null });
        return;
      case "container":
        update({ status: "restarting", reason: "engine_container", code: outcome.code, health: current.health });
        return;
      case "incompatible":
        setTerminal("incompatible", outcome.code, { ownedByDesktop: owned });
        generation += 1;
        return;
      case "unauthenticated":
        update({ status: "restarting", reason: "credentials", code: "DOCS_ENGINE_UNAUTHENTICATED" });
        return;
      default:
        unreachableCount += 1;
        if (current.status === "starting" || relaunching) {
          if (timers.now() > startDeadline) startTimedOut(gen);
          return;
        }
        if (unreachableCount < t.unreachableLimit) return;
        if (owned && child) {
          update({ status: "restarting", reason: "engine_container", code: "SO_ENGINE_UNREACHABLE", health: current.health });
        } else {
          generation += 1;
          setTerminal("stopped", "SO_ENGINE_UNREACHABLE");
        }
    }
  }

  function startTimedOut(gen) {
    const running = child;
    generation += 1;
    if (running) terminate(running).catch(() => {});
    setTerminal("crashed", "SO_SERVE_START_TIMEOUT");
    logger.warn?.("Super Drive engine start timed out.", { gen });
  }

  // ---- exits and restarts -----------------------------------------------

  function handleExit(gen, code, signal) {
    if (stale(gen)) return;
    clearTimers();
    credentials = null;
    owned = false;
    const outcome = EXIT_OUTCOMES[code];
    if (outcome && !signal) {
      generation += 1;
      setTerminal(outcome.status, outcome.code);
      return;
    }
    if (code === 6 && !portRetried) {
      portRetried = true;
      launch({ fresh: false });
      return;
    }
    if (code === 6) {
      generation += 1;
      setTerminal("crashed", "SO_SERVE_PORT_IN_USE");
      return;
    }
    scheduleRestart(gen);
  }

  /** Unexpected exit: back off 5 s, 15 s, 45 s; a fourth failure inside 10 minutes needs a manual retry. */
  function scheduleRestart(gen) {
    const now = timers.now();
    restartTimes = restartTimes.filter((at) => now - at < t.restartWindowMs);
    if (restartTimes.length >= t.maxRestarts) {
      generation += 1;
      setTerminal("crashed", "SO_ENGINE_UNREACHABLE", { reason: "restart_limit" });
      return;
    }
    const delay = t.backoffMs[Math.min(restartTimes.length, t.backoffMs.length - 1)];
    restartTimes.push(now);
    totalRestarts += 1;
    relaunching = true;
    update({
      status: "restarting", reason: "process_exit", code: null, health: null, ownedByDesktop: false,
      restarts: { count: totalRestarts, nextAttemptAt: iso(now + delay) },
    });
    restartTimer = timers.setTimeout(() => {
      restartTimer = null;
      if (!stale(gen)) launch({ fresh: false });
    }, delay);
    restartTimer.unref?.();
  }

  // ---- stop, restart, shutdown --------------------------------------------

  /** SIGTERM first; SIGKILL when the process outlives the grace period. */
  async function terminate(running) {
    const exited = childExited;
    try { running.kill("SIGTERM"); } catch { /* Already gone. */ }
    const timedOut = Symbol("timeout");
    let timer;
    const race = await Promise.race([
      exited,
      new Promise((resolve) => { timer = timers.setTimeout(() => resolve(timedOut), t.stopGraceMs); }),
    ]);
    timers.clearTimeout(timer);
    if (race === timedOut) {
      try { running.kill("SIGKILL"); } catch { /* Already gone. */ }
      await Promise.race([exited, new Promise((resolve) => { timers.setTimeout(resolve, 2_000).unref?.(); })]);
    }
  }

  async function stop() {
    const current = snapshots.peek();
    if (!ACTIVE_STATUSES.has(current.status) && !child) return snapshots.get();
    if (credentials?.baseUrl) return disconnectRemote();
    if (!owned && !child && credentials && !credentials.baseUrl) throw new SuperOntologyError("SO_ENGINE_NOT_OWNED");
    const running = child;
    generation += 1;
    clearTimers();
    if (running) {
      update({ status: "stopping", reason: null, code: null });
      await terminate(running);
    }
    credentials = null;
    owned = false;
    relaunching = false;
    update({
      status: "stopped", reason: null, code: null, health: null, ownedByDesktop: false, startedAt: null,
      restarts: { count: totalRestarts, nextAttemptAt: null },
    });
    return snapshots.get();
  }

  /** Remote mode: attach to an `oo serve` somewhere else with a token the person supplied. Never owned, never stopped by the app. */
  async function connectRemote({ baseUrl, token }) {
    if (shutdownPromise) throw new SuperOntologyError("SO_ENGINE_NOT_RUNNING");
    if (child || owned || ACTIVE_STATUSES.has(snapshots.peek().status)) await stop();
    generation += 1;
    const gen = generation;
    clearTimers();
    unreachableCount = 0;
    lastHealthJson = "";
    relaunching = false;
    owned = false;
    startDeadline = timers.now() + t.startTimeoutMs;
    credentials = { info: { pid: null, port: 0, apiMajor: 1, apiVersion: "1", startedAt: null }, token, baseUrl };
    update({ status: "starting", reason: null, code: null, health: null, ownedByDesktop: false, startedAt: null, mode: "remote", remote: { host: new URL(baseUrl).host } });
    schedulePoll(gen, 0);
    return snapshots.get();
  }

  async function disconnectRemote() {
    generation += 1;
    clearTimers();
    credentials = null;
    owned = false;
    update({ status: "stopped", reason: null, code: null, health: null, ownedByDesktop: false, startedAt: null, mode: "local", remote: null,
      restarts: { count: totalRestarts, nextAttemptAt: null } });
    return snapshots.get();
  }

  async function restart() {
    const keep = keepEngine;
    if (!owned && !child && credentials) throw new SuperOntologyError("SO_ENGINE_NOT_OWNED");
    if (ACTIVE_STATUSES.has(snapshots.peek().status) || child) await stop();
    return start({ keepEngineOnQuit: keep });
  }

  /**
   * Application quit. The SIGTERM goes out synchronously, before the first
   * await, so it is delivered even when Electron exits right after calling
   * this; the daemon finishes its own 10 s drain on its own. With "keep the
   * engine running" nothing is signalled and the child is detached.
   */
  function shutdown() {
    if (shutdownPromise) return shutdownPromise;
    clearTimers();
    generation += 1;
    const running = child;
    if (running && owned && !keepEngine) {
      shutdownPromise = terminate(running);
    } else {
      running?.unref?.();
      shutdownPromise = Promise.resolve();
    }
    credentials = null;
    return shutdownPromise;
  }

  return Object.freeze({
    preflight,
    start,
    stop,
    restart,
    connectRemote,
    disconnectRemote,
    shutdown,
    /** An `engine.health_changed` event arrived: do not wait for the next poll. */
    pokeHealth() {
      if (credentials && !stale(generation) && ACTIVE_STATUSES.has(snapshots.peek().status)) schedulePoll(generation, 0);
    },
    /** A local Drive was registered or removed: the daemon rebuilds the engine container. */
    beginDriveChange() {
      if (!credentials) return;
      driveChangeUntil = timers.now() + t.driveChangeTimeoutMs;
      const current = snapshots.peek();
      update({ status: "restarting", reason: "drive_change", code: null, health: current.health });
      if (pollTimer) schedulePoll(generation, t.pollIntervalMs);
    },
    /** The control client could not authenticate even after re-reading the token. */
    reportAuthFailure() {
      if (!credentials || !ACTIVE_STATUSES.has(snapshots.peek().status)) return;
      update({ status: "restarting", reason: "credentials", code: "DOCS_ENGINE_UNAUTHENTICATED" });
    },
    setKeepEngine(value) {
      keepEngine = value === true;
      update({});
      return snapshots.get();
    },
    /** In-process only: base URL and control token for the HTTP client. Never serialized. */
    credentials() {
      return credentials ? { baseUrl: credentials.baseUrl ?? engineBaseUrl(credentials.info.port), token: credentials.token } : null;
    },
    /** Re-reads serve.json and the token file (a rotated token after an engine restart). */
    async reloadCredentials() {
      if (credentials?.baseUrl) return false;
      const found = await readCredentials({ stateDir, fsPromises }).catch(() => null);
      if (found) credentials = found;
      return found !== null;
    },
    /** Identity of the running instance; a change means event sequence numbers may have reset. */
    instanceId() {
      return credentials ? `${credentials.info.pid ?? 0}:${credentials.info.startedAt ?? ""}` : null;
    },
  });
}
