import fs from "node:fs";
import path from "node:path";
import { isIgnoredName } from "./upload-service.mjs";
import { isDrive, isPage, ensure } from "./response-guards.mjs";

/** Quiet time after the last change before paths are sent (D46). */
export const WATCH_DEBOUNCE_MS = 300;
/** A steady stream of changes (a big copy) still reaches the engine at least this often. */
export const WATCH_MAX_WAIT_MS = 3_000;
/** `SyncRequest.paths` takes at most 500; more paths than that, or a lost event, becomes one full sync. */
export const MAX_SYNC_PATHS = 500;
const SYNC_RUNNING_RETRY_MS = 1_000;
const SYNC_RUNNING_RETRIES = 5;
const RESTART_DELAY_MS = 5_000;
const DRIVE_PAGE_LIMIT = 200;
const DRIVE_PAGE_CAP = 50;
/** Whole directories the engine never reads (10 §4.3.2). */
const IGNORED_DIRECTORIES = new Set(["node_modules", "__pycache__"]);
const LOCK_FILE = /^\.~lock\..*#$/u;
const DRIVE_PATH = /^(?!\/)(?!.*\/\/)(?!.*(^|\/)\.\.?(\/|$)).+$/su;
const CONTROL_CHARACTERS = /[\u0000-\u001f\u007f]/u;

/** The same rule table the engine and the upload service use: a path is ignored when any of its names is. */
export function isIgnoredPath(relativePath) {
  return relativePath.split("/").some((name) => name === "" || isIgnoredName(name) || IGNORED_DIRECTORIES.has(name) || LOCK_FILE.test(name));
}

/** A DrivePath from what `fs.watch` reports, or `null` when the change is not for the engine (ignored, outside the root, not a valid path). */
export function toDrivePath(filename) {
  if (typeof filename !== "string" || filename === "") return null;
  const relative = filename.split(path.sep).join("/");
  if (relative.length > 1024 || CONTROL_CHARACTERS.test(relative) || !DRIVE_PATH.test(relative) || isIgnoredPath(relative)) return null;
  return relative;
}

/**
 * File-system watching of registered fs Drives (D46, 20-frontend F-S3-07). The main process, not the container, watches,
 * because a macOS bind mount does not deliver change events reliably. Changed paths are merged for 300 ms and sent as a
 * targeted sync (D47); a watcher error or an event without a path falls back to one full sync. The engine's own 30 s poll stays
 * as the safety net, so nothing here has to be perfect, only cheap: idle Drives cost no wake-ups.
 */
export function createDriveWatcher({
  client, logger = console, watch = fs.watch, readDrives, createId = () => `w${Math.random().toString(16).slice(2)}${Date.now().toString(16)}`.padEnd(16, "0"),
  timers = { setTimeout, clearTimeout, now: () => Date.now() }, debounceMs = WATCH_DEBOUNCE_MS, maxWaitMs = WATCH_MAX_WAIT_MS,
} = {}) {
  /** driveId -> { root, handle, pending: Set, overflow, timer, firstAt, retries, restartTimer } */
  const watched = new Map();
  let disposed = false;
  let refreshing = null;
  let refreshAgain = false;
  let sequence = 0;

  async function listDrives() {
    if (readDrives) return readDrives();
    const drives = [];
    let cursor;
    for (let page = 0; page < DRIVE_PAGE_CAP; page += 1) {
      const { value } = await client.request("drivesList", { query: { limit: DRIVE_PAGE_LIMIT, cursor } });
      ensure(value, isPage);
      for (const item of value.items) drives.push(ensure(item, isDrive));
      if (!value.next_cursor) return drives;
      cursor = value.next_cursor;
    }
    return drives;
  }

  function clearTimer(entry) {
    if (entry.timer) timers.clearTimeout(entry.timer);
    entry.timer = null;
  }

  async function flush(driveId) {
    const entry = watched.get(driveId);
    if (!entry || disposed) return;
    clearTimer(entry);
    entry.firstAt = null;
    const full = entry.overflow || entry.pending.size > MAX_SYNC_PATHS;
    const paths = [...entry.pending];
    entry.pending.clear();
    entry.overflow = false;
    if (!full && paths.length === 0) return;
    sequence += 1;
    try {
      await client.request("driveSync", { params: { id: driveId }, body: full ? {} : { paths }, operationId: `watch-${createId()}-${sequence}` });
      entry.retries = 0;
    } catch (error) {
      if (error?.code === "DOCS_SOURCE_SYNC_RUNNING" && entry.retries < SYNC_RUNNING_RETRIES && watched.get(driveId) === entry) {
        // The running sync may have listed before these changes: ask again shortly.
        entry.retries += 1;
        if (full) entry.overflow = true; else for (const item of paths) entry.pending.add(item);
        entry.timer = timers.setTimeout(() => { void flush(driveId); }, SYNC_RUNNING_RETRY_MS);
        return;
      }
      // Anything else (engine restarting, drive gone): the engine's poll finds the files.
      logger.debug?.("Super Drive watcher sync request failed.", { code: error?.code });
    }
  }

  function schedule(driveId, entry) {
    const now = timers.now();
    entry.firstAt ??= now;
    clearTimer(entry);
    const wait = Math.max(0, Math.min(debounceMs, entry.firstAt + maxWaitMs - now));
    entry.timer = timers.setTimeout(() => { void flush(driveId); }, wait);
  }

  function onChange(driveId, entry, filename) {
    if (disposed || watched.get(driveId) !== entry) return;
    const relative = filename === null || filename === undefined ? null : toDrivePath(String(filename));
    if (filename === null || filename === undefined || String(filename) === "") entry.overflow = true; // No path: events were lost.
    else if (relative === null) return; // Hidden or junk: not the engine's business.
    else entry.pending.add(relative);
    schedule(driveId, entry);
  }

  function start(driveId, root) {
    const entry = { root, handle: null, pending: new Set(), overflow: false, timer: null, firstAt: null, retries: 0, restartTimer: null };
    try {
      entry.handle = watch(root, { recursive: true, persistent: false }, (_type, filename) => onChange(driveId, entry, filename));
    } catch (error) {
      logger.debug?.("Super Drive watcher could not start.", { code: error?.code });
      return false;
    }
    entry.handle.on?.("error", (error) => {
      logger.debug?.("Super Drive watcher failed.", { code: error?.code });
      if (watched.get(driveId) !== entry || disposed) return;
      try { entry.handle.close(); } catch { /* Already closed. */ }
      entry.handle = null;
      entry.overflow = true;
      schedule(driveId, entry);
      entry.restartTimer = timers.setTimeout(() => {
        if (watched.get(driveId) !== entry || disposed) return;
        watched.delete(driveId);
        clearTimer(entry);
        start(driveId, root);
      }, RESTART_DELAY_MS);
    });
    watched.set(driveId, entry);
    return true;
  }

  function stop(driveId) {
    const entry = watched.get(driveId);
    if (!entry) return;
    watched.delete(driveId);
    clearTimer(entry);
    if (entry.restartTimer) timers.clearTimeout(entry.restartTimer);
    try { entry.handle?.close(); } catch { /* Already closed. */ }
  }

  /** Starts watching the active folder Drives that are not watched yet and releases the ones that went away or moved. */
  function reconcile(drives) {
    const wanted = new Map();
    for (const drive of drives) {
      const root = drive.config?.root;
      if (drive.backend === "fs" && drive.status === "active" && typeof root === "string" && path.isAbsolute(root)) wanted.set(drive.id, root);
    }
    for (const [driveId, entry] of [...watched]) if (wanted.get(driveId) !== entry.root) stop(driveId);
    for (const [driveId, root] of wanted) if (!watched.has(driveId)) start(driveId, root);
  }

  return Object.freeze({
    /** Reads the Drive list from the engine and reconciles; calls overlap safely (one more pass runs after the current one). */
    refresh() {
      if (disposed) return Promise.resolve();
      if (refreshing) { refreshAgain = true; return refreshing; }
      refreshing = (async () => {
        try {
          do {
            refreshAgain = false;
            reconcile(await listDrives());
          } while (refreshAgain && !disposed);
        } catch (error) {
          logger.debug?.("Super Drive watcher could not read the Drive list.", { code: error?.code });
        } finally {
          refreshing = null;
        }
      })();
      return refreshing;
    },
    reconcile,
    /** The engine is not usable: release everything (events would only be lost); `refresh` starts again. */
    stopAll() { for (const driveId of [...watched.keys()]) stop(driveId); },
    watching: () => [...watched.keys()],
    dispose() {
      disposed = true;
      for (const driveId of [...watched.keys()]) stop(driveId);
    },
  });
}
