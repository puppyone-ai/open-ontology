import fs from "node:fs/promises";
import path from "node:path";
import { SuperOntologyError } from "../../shared/super-ontology/errors.mjs";
import { runBoundedProcessProbe } from "./bounded-process-probe.mjs";
import { createEnginePreflight } from "./docker-preflight.mjs";
import { createLiteRuntimeProbe } from "./lite-runtime.mjs";
import { RUNTIME_CHOICES } from "../../shared/super-ontology/engine-contract.mjs";
import { createEngineLifecycle } from "./engine-lifecycle.mjs";
import { createEngineLocator } from "./engine-locator.mjs";
import { createControlClient } from "./control-client.mjs";
import { createControlHandlers } from "./control-handlers.mjs";
import { createEnterpriseHandlers } from "./enterprise-handlers.mjs";
import { createEventBridge } from "./event-bridge.mjs";
import { createDriveFileHandler, createDriveFileTokens } from "./drive-file-protocol.mjs";
import { createFileHandlers } from "./file-handlers.mjs";
import { createGrantStore } from "./grants.mjs";
import { fileURLToPath } from "node:url";
import { createHostMount } from "./host-mount.mjs";
import { createMediaDecoder, resolveMediaDecoderPath } from "./media-decoder.mjs";
import { createOntologyHandlers } from "./ontology-handlers.mjs";
import { createDriveWatcher } from "./drive-watcher.mjs";
import { createSecretFiles } from "./secret-files.mjs";
import { createSuggestHandlers } from "./suggest-handlers.mjs";
import { createWriteback } from "./writeback.mjs";
import { createServeLog } from "./serve-log.mjs";
import { withHostDriveRoots } from "./drive-host-roots.mjs";
import { createSnapshotStore } from "./snapshot.mjs";
import { createUploadService } from "./upload-service.mjs";

/**
 * Composition root of the Super Ontology main-process service (20-frontend §1.4).
 * Idle until the first call, and even then nothing is probed or started
 * until a handler that needs it runs. Handlers receive `(request, { sender })`.
 *
 * @param {{
 *   logger?: any,
 *   platformHost?: { platform: string, executableDiscovery?: { captureEnvironment: Function } },
 *   paths?: { stateDir?: string, logsDir?: string, settingsFile?: string },
 *   broadcast?: (channel: string, payload: unknown) => void,
 *   pickPath?: (input: { sender: object, purpose: string }) => Promise<string | null>,
 *   revealPath?: (absolutePath: string) => void,
 *   readEnvironment?: (input?: object) => Promise<{ environment: Record<string, string> }>,
 *   runProbe?: Function,
 *   spawn?: Function,
 *   fetchHealth?: Function,
 *   fetchImpl?: typeof fetch,
 *   statfs?: Function,
 *   fsPromises?: any,
 *   timers?: { setTimeout: Function, clearTimeout: Function, now: () => number },
 *   lifecycleTuning?: Record<string, number>,
 *   bridgeTuning?: Record<string, number>,
 *   driveWatcherOptions?: object,
 *   onFileWritten?: (written: { driveId: string, root: string, relativePath: string, suggestionId: string }) => Promise<void>,
 * }} [options] `broadcast` pushes to every window; `pickPath` is the native picker; `revealPath` shows a file in the OS file manager.
 */
export function createSuperOntologyService({
  logger = console,
  platformHost = { platform: "macos" },
  paths = {},
  broadcast = () => {},
  pickPath = async () => null,
  revealPath = () => {},
  readEnvironment,
  runProbe = runBoundedProcessProbe,
  spawn,
  fetchHealth,
  fetchImpl,
  statfs,
  fsPromises = fs,
  timers,
  lifecycleTuning,
  bridgeTuning,
  mediaDecoderPath = null,
  driveWatcherOptions,
  onFileWritten,
} = {}) {
  const stateDir = paths.stateDir;
  /** The engine reads host decode output from here (`OO_DOCS_EXCHANGE`); only macOS has a helper that writes it. */
  const exchangeRoot = platformHost.platform === "macos" && stateDir ? (paths.exchangeDir ?? path.join(stateDir, "exchange")) : null;
  const logsDir = paths.logsDir ?? (stateDir ? path.join(path.dirname(stateDir), "logs") : undefined);
  const snapshots = createSnapshotStore({ broadcast });
  const grants = createGrantStore();
  const locator = createEngineLocator({
    platform: platformHost.platform,
    readEnvironment: readEnvironment
      ?? ((input) => platformHost.executableDiscovery.captureEnvironment(input)),
    fsPromises,
    runProbe,
  });
  // The Docker-free runtime ships inside the app (`paths.liteRuntimeDir`); SUPER_ONTOLOGY_LITE_DIR points a development run at another one.
  const liteRuntime = createLiteRuntimeProbe({ directory: process.env.SUPER_ONTOLOGY_LITE_DIR ?? paths.liteRuntimeDir ?? null, fsPromises });
  let runtimeChoice = "auto";
  const enginePreflight = createEnginePreflight({ platform: platformHost.platform, locator, runProbe, liteRuntime, getRuntimeChoice: () => runtimeChoice, ...(statfs ? { statfs } : {}) });
  const serveLog = createServeLog({ directory: logsDir ?? "/nonexistent", fsPromises });

  let userExecutable = null;
  let settingsLoaded = false;

  async function loadSettings() {
    if (settingsLoaded) return;
    settingsLoaded = true;
    if (!paths.settingsFile) return;
    try {
      const parsed = JSON.parse(await fsPromises.readFile(paths.settingsFile, "utf8"));
      if (typeof parsed?.ooExecutable === "string" && path.isAbsolute(parsed.ooExecutable)) userExecutable = parsed.ooExecutable;
      if (typeof parsed?.keepEngineOnQuit === "boolean") lifecycle.setKeepEngine(parsed.keepEngineOnQuit);
      if (RUNTIME_CHOICES.includes(parsed?.runtime)) runtimeChoice = parsed.runtime;
    } catch { /* No settings yet, or unreadable: fall back to PATH. */ }
  }

  async function saveSettings() {
    if (!paths.settingsFile) return;
    try {
      await fsPromises.mkdir(path.dirname(paths.settingsFile), { recursive: true, mode: 0o700 });
      const temporary = `${paths.settingsFile}.${process.pid}.tmp`;
      await fsPromises.writeFile(temporary, JSON.stringify({
        ooExecutable: userExecutable, keepEngineOnQuit: snapshots.peek().keepEngineOnQuit, runtime: runtimeChoice,
      }), { mode: 0o600 });
      await fsPromises.rename(temporary, paths.settingsFile);
    } catch (error) {
      logger.warn?.("Super Ontology settings could not be saved.", { code: error?.code });
    }
  }

  const lifecycle = createEngineLifecycle({
    snapshots, enginePreflight, getUserExecutable: () => userExecutable, stateDir, serveLog,
    runProbe, logger, fsPromises, exchangeDirectory: exchangeRoot,
    ...(spawn ? { spawn } : {}), ...(fetchHealth ? { fetchHealth } : {}),
    ...(timers ? { timers } : {}), ...(lifecycleTuning ? { tuning: lifecycleTuning } : {}),
  });

  const controlClient = createControlClient({
    credentials: () => lifecycle.credentials(),
    reloadCredentials: () => lifecycle.reloadCredentials(),
    onAuthFailure: () => lifecycle.reportAuthFailure(),
    logger,
    ...(fetchImpl ? { fetchImpl } : {}),
  });
  // The Docker runtime's Drive documents name the container's folder; the app needs the one on this Mac.
  const client = withHostDriveRoots(controlClient, { stateDir, fsPromises });
  const hostMount = stateDir
    ? createHostMount({ stateDir, fs: fsPromises, fetchPackFile: async (packId, relativePath) => (await client.request("packFile", { params: { pid: packId, path: relativePath } })).value })
    : null;
  const secretFiles = createSecretFiles({ directory: stateDir ? path.join(path.dirname(stateDir), "secrets") : undefined, fsPromises });
  const nodePlatform = platformHost.platform === "macos" ? "darwin" : platformHost.platform;
  const mediaDecoder = createMediaDecoder({
    helperPath: mediaDecoderPath, exchangeRoot, platform: nodePlatform, logger, fsPromises,
    locateHelper: () => resolveMediaDecoderPath({ platform: nodePlatform, repositoryRoot: path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../.."), fsPromises }),
  });
  const uploads = createUploadService({ client, broadcast, logger });
  // D46: change hints for fs Drives come from here, not from the container. Off in tests that bring no real engine.
  const driveWatcher = createDriveWatcher({ client, logger, ...(driveWatcherOptions ?? {}) });
  let engineUsable = false;
  snapshots.subscribe((snapshot) => {
    const usable = snapshot.status === "healthy" || snapshot.status === "degraded";
    if (usable === engineUsable) return;
    engineUsable = usable;
    if (usable) void driveWatcher.refresh();
    else driveWatcher.stopAll();
  });
  const driveFileTokens = createDriveFileTokens();
  const driveFileHandler = createDriveFileHandler({ tokens: driveFileTokens });
  const eventBridge = createEventBridge({
    lifecycle, snapshots, broadcast, logger,
    ...(fetchImpl ? { fetchImpl } : {}), ...(timers ? { timers } : {}), ...(bridgeTuning ? { tuning: bridgeTuning } : {}),
  });

  const controlHandlers = createControlHandlers({ client, grants, lifecycle, secretFiles });
  // Write-back of accepted editor suggestions (F-S8-03). The journal records suggestion, Share, files and evidence for every write;
  // `onFileWritten` lets the host nudge the Git auto-commit flow (its watcher also sees the change).
  const writeback = createWriteback({
    client, logger, onWritten: onFileWritten ?? (async () => undefined),
    journalFile: stateDir ? path.join(path.dirname(stateDir), "writeback", "journal.jsonl") : null,
  });
  const suggestHandlers = createSuggestHandlers({ client, writeback });

  /** After a Drive was created, changed or deleted the set of watched folders may have changed. */
  const watchesDrives = (name, handler) => async (...args) => {
    const result = await handler(...args);
    void driveWatcher.refresh();
    return result;
  };

  /** Request handlers keyed by channel name; the IPC layer validates the request first. */
  const handlers = {
    ...controlHandlers,
    ...suggestHandlers,
    ...createEnterpriseHandlers({ client }),
    ...createFileHandlers({ client, tokens: driveFileTokens }),
    ...createOntologyHandlers({ client, grants, revealPath, secretsDirectory: stateDir ? path.join(path.dirname(stateDir), "secrets") : null, fsPromises, mediaDecoder, hostMount }),
    ...Object.fromEntries(["drive-create", "drive-update", "drive-delete"].map((name) => [name, watchesDrives(name, controlHandlers[name])])),
    "engine-read": async () => snapshots.get(),
    "engine-preflight": async () => { await loadSettings(); return lifecycle.preflight(); },
    "engine-start": async ({ keepEngineOnQuit } = {}) => {
      await loadSettings();
      const snapshot = await lifecycle.start({ keepEngineOnQuit });
      if (typeof keepEngineOnQuit === "boolean") await saveSettings();
      return snapshot;
    },
    "engine-stop": async () => lifecycle.stop(),
    /** Chooses the runtime for the next start and checks the setup again; while an engine runs, the choice waits for its next start. */
    "engine-set-runtime": async ({ runtime }) => {
      await loadSettings();
      runtimeChoice = runtime;
      await saveSettings();
      return lifecycle.preflight();
    },
    "engine-restart": async () => { await loadSettings(); return lifecycle.restart(); },
    "engine-set-keep": async ({ keepEngineOnQuit }) => {
      const snapshot = lifecycle.setKeepEngine(keepEngineOnQuit);
      await saveSettings();
      return snapshot;
    },
    "engine-reveal-logs": async ({ target = "logs" } = {}) => {
      const location = target === "state" ? stateDir : serveLog.file;
      if (!location) throw new SuperOntologyError("SO_ENGINE_NOT_RUNNING");
      revealPath(location);
      return { revealed: true };
    },
    "upload-start": async ({ driveId, folder, paths }) => uploads.start({ driveId, folder, paths }),
    "upload-cancel": async ({ batchId, itemId }) => uploads.cancel({ batchId, itemId }),
    "events-subscribe": async ({ sinceSeq } = {}, { sender } = {}) => eventBridge.subscribe(sender, { sinceSeq }),
    "events-since": async ({ instanceId, seq }) => eventBridge.since({ instanceId, seq }),
    "events-unsubscribe": async (_request, { sender } = {}) => eventBridge.unsubscribe(sender),
    "pick-path": async ({ purpose }, { sender } = {}) => {
      const selected = await pickPath({ sender, purpose });
      if (!selected) return { cancelled: true };
      return grants.issue(sender, purpose, selected);
    },
    "engine-set-executable": async ({ grantId }, { sender } = {}) => {
      const selected = grants.resolve(sender, grantId, "oo-executable");
      const environment = await locator.environment();
      const version = await locator.readOoVersion(selected, environment);
      if (!version) throw new SuperOntologyError("SO_OO_INVALID");
      await loadSettings();
      userExecutable = selected;
      await saveSettings();
      return lifecycle.preflight();
    },
  };

  let shutDown = false;
  return Object.freeze({
    handlers: Object.freeze(handlers),
    /** For main.mjs: `registerDriveFileProtocol({ protocol, handler: service.driveFileHandler })` once the app is ready. */
    driveFileHandler,
    /** Shared parts for later composition (client, event bridge) and for tests; not exposed over IPC. */
    internals: Object.freeze({ snapshots, grants, locator, enginePreflight, lifecycle, serveLog, client, secretFiles, eventBridge, getUserExecutable: () => userExecutable }),
    async shutdown() {
      if (shutDown) return;
      shutDown = true;
      driveFileTokens.revokeAll();
      uploads.dispose();
      driveWatcher.dispose();
      eventBridge.dispose();
      await mediaDecoder.shutdown();
      await lifecycle.shutdown();
      logger.debug?.("Super Ontology service shut down.");
    },
  });
}
