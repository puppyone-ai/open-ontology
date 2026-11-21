import { createHash, randomUUID } from "node:crypto";
import fs from "node:fs/promises";
import { createReadStream, createWriteStream } from "node:fs";
import path from "node:path";
import { pipeline } from "node:stream/promises";
import { SuperOntologyError } from "../../../src/shared/super-ontology/errors.mjs";
import { createIgnoreStack, ignoreReason, isIgnoredName, loadDirRules } from "./ignore-rules.mjs";

export { ignoreReason, isIgnoredName };

/**
 * A dropped folder with more files than this (after the ignore rules), or more bytes than {@link LARGE_FOLDER_BYTES}, is not copied unasked:
 * the person chooses between linking it (no copy, stays in sync) and copying it (streamed). There is no cap on how much can be dropped.
 */
export const LARGE_FOLDER_FILES = 2_000;
export const LARGE_FOLDER_BYTES = 1024 ** 3;
const COPY_CONCURRENCY = 6;
/** Files of one folder are stat-ed this many at a time (a code project is thousands of tiny files; one round trip each, in a row, is the slow way). */
const STAT_CONCURRENCY = 64;
const PROGRESS_INTERVAL_MS = 100;
/** The copy queue is bounded: the walk waits above the high mark and goes on below the low one, so a million files never sit in memory. */
const QUEUE_HIGH = 1_000;
const QUEUE_LOW = 400;
/** Finished items kept per batch (for the keep-in-sync takeback and retries); the counts stay exact beyond it. */
const SETTLED_RETAIN = 5_000;
const BATCH_SETTLE_RETAIN = 20;
/** Free space kept back from a copy; a drop that would leave less is refused with a clear message instead of failing halfway. */
export const SPACE_RESERVE_BYTES = 512 * 1024 ** 2;
const SYNC_FIRST_MS = 400;
const SYNC_EVERY_MS = 2_500;
const SYNC_RETRY_MS = 1_000;
/** How many skipped names a drop reports back, so a drop of thousands of files stays a small answer. */
const SKIP_SAMPLE_LIMIT = 12;

const emptySkipped = () => ({ hidden: 0, links: 0, junk: 0, temporary: 0, special: 0, gitignore: 0, ooignore: 0 });

/** "report.pdf" -> "report (2).pdf"; the extension stays last, a leading dot is not an extension. */
export function numberedName(name, attempt) {
  const dot = name.lastIndexOf(".");
  const hasExtension = dot > 0;
  return hasExtension ? `${name.slice(0, dot)} (${attempt})${name.slice(dot)}` : `${name} (${attempt})`;
}

function classifyError(error) {
  switch (error?.code) {
    case "ENOSPC": case "EDQUOT": return "SO_UPLOAD_DISK_FULL";
    case "EACCES": case "EPERM": case "EROFS": return "SO_UPLOAD_PERMISSION_DENIED";
    case "ENOENT": case "ENOTDIR": return "SO_UPLOAD_SOURCE_MISSING";
    default: return "SO_UPLOAD_FAILED";
  }
}

const isInside = (parent, child) => {
  const relative = path.relative(parent, child);
  return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
};
const byName = (a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0);

/**
 * Copies, or lets the person link, what was dropped into an fs Drive's folder (20-frontend §3.2.6). `start` answers at once; the main process
 * then walks the dropped paths in the background and the renderer follows `upload-progress` pushes: the files found so far, the rows as they are
 * released, the copy per item. Only the main process touches the disk.
 *
 *  - The walk applies the engine's ignore rules (hidden, temporary, junk, code project folders, `.gitignore`, `.ooignore`) and counts what it skipped.
 *  - A dropped folder over {@link LARGE_FOLDER_FILES} files or {@link LARGE_FOLDER_BYTES} bytes pauses for a choice (`decide`); nothing of it is
 *    held in memory or copied meanwhile. "copy" walks it again as a stream with a bounded queue; "skip" leaves it (the person links it instead).
 *  - Copies: a temporary name the engine ignores, then an atomic claim of the final name (a numbered name on a clash). Duplicates are found by size
 *    and modification time first (a copy keeps the modification time), and by hash only when those differ.
 *  - `driveSync` is asked for while the copy runs (first after a moment, then every few seconds), so the engine lists files as they arrive.
 *
 * @param {object} options
 * @param {{ request: Function }} options.client control API client
 * @param {(channel: string, payload: unknown) => void} options.broadcast
 */
export function createUploadService({ client, broadcast, fsPromises = fs, createId = randomUUID, now = () => Date.now(), logger = {} }) {
  const batches = new Map();

  const phaseOf = (batch) => {
    if (batch.cancelled) return "cancelled";
    if (batch.scanning) return "scanning";
    if (batch.large.some((root) => root.state === "pending")) return "choice";
    return batch.finished ? "done" : "copying";
  };

  function snapshot(batch) {
    return {
      phase: phaseOf(batch), found: batch.found, bytes: batch.bytes, queued: batch.queued,
      counts: { ...batch.counts }, skipped: { ...batch.skipped }, skippedSamples: batch.samples.slice(),
      roots: batch.roots.map((root) => ({ ...root })), large: batch.large.map(({ path: where, name, files, bytes, state }) => ({ path: where, name, files, bytes, state })),
      emptyFolders: batch.emptyFolders, unreadable: batch.unreadable,
      ...(batch.error ? { error: batch.error } : {}),
    };
  }

  /** A change to an item is queued for the next push; a push carries only what changed (and the rows released since the last one). */
  function touch(batch, item) { batch.dirty.add(item); }

  function publish(batch, { force = false, full = false } = {}) {
    const time = now();
    if (!force && !full && time - batch.lastPush < PROGRESS_INTERVAL_MS) return;
    const changed = full ? [...batch.items.values()] : [...batch.dirty];
    batch.dirty.clear();
    const added = batch.added;
    batch.added = [];
    batch.lastPush = time;
    broadcast("upload-progress", {
      batchId: batch.id,
      items: changed.map((item) => ({
        itemId: item.id, state: item.state, bytesDone: item.bytesDone, bytesTotal: item.size,
        ...(item.code ? { code: item.code } : {}), ...(item.drivePath ? { drivePath: item.drivePath } : {}), ...(item.skipReason ? { skipReason: item.skipReason } : {}),
      })),
      ...(added.length > 0 ? { added } : {}),
      batch: snapshot(batch),
    });
  }

  async function readDrive(driveId) {
    const { value } = await client.request("driveGet", { params: { id: driveId } });
    if (value?.backend !== "fs") throw new SuperOntologyError("SO_UPLOAD_UNSUPPORTED_BACKEND");
    const root = value?.config?.root;
    if (typeof root !== "string" || !path.isAbsolute(root)) throw new SuperOntologyError("SO_UPLOAD_UNSUPPORTED_BACKEND");
    return root;
  }

  const skip = (batch, reason, name) => {
    batch.skipped[reason] += 1;
    if (batch.samples.length < SKIP_SAMPLE_LIMIT) batch.samples.push({ name, reason });
  };

  /**
   * Walks one dropped folder depth first, in name order, calling `visit.file({ source, relative, size, mtimeMs })` for every file the rules keep and
   * `visit.empty(relative)` for an empty folder. Links below the folder are never followed (one could point outside what the person chose).
   * `report.skip(reason, name)` and `report.unreadable()` count what was left out. `visit.file` may be async: the walk waits for it (backpressure).
   */
  async function walkTree(batch, top, name, visit, report) {
    async function flush(pending, relative) {
      for (let from = 0; from < pending.length; from += STAT_CONCURRENCY) {
        const chunk = pending.slice(from, from + STAT_CONCURRENCY);
        const stats = await Promise.all(chunk.map((entry) => fsPromises.lstat(entry.full).catch(() => null)));
        for (let index = 0; index < chunk.length; index += 1) {
          if (batch.cancelled) return;
          const stat = stats[index];
          if (!stat) { report.unreadable(); continue; }
          await visit.file({ source: chunk[index].full, relative: path.posix.join(relative, chunk[index].name), size: stat.size, mtimeMs: stat.mtimeMs });
        }
      }
      pending.length = 0;
    }

    async function walk(directory, relative, parts, stack) {
      if (batch.cancelled) return;
      let entries;
      try { entries = await fsPromises.readdir(directory, { withFileTypes: true }); } catch { report.unreadable(); return; }
      if (entries.length === 0) { visit.empty?.(relative); return; }
      entries.sort(byName);
      let rules = stack;
      if (entries.some((entry) => (entry.name === ".gitignore" || entry.name === ".ooignore") && entry.isFile())) rules = stack.with(parts.length, await loadDirRules(directory, fsPromises));
      let pending = [];
      for (const entry of entries) {
        const isDirectory = entry.isDirectory();
        const reason = ignoreReason(entry.name, isDirectory) ?? (rules.isEmpty ? null : rules.ignored([...parts, entry.name], isDirectory));
        if (reason) { report.skip(reason, entry.name); continue; }
        const full = path.join(directory, entry.name);
        if (entry.isSymbolicLink()) { report.skip("links", entry.name); continue; }
        if (isDirectory) {
          await flush(pending, relative);
          pending = [];
          await walk(full, path.posix.join(relative, entry.name), [...parts, entry.name], rules);
          if (batch.cancelled) return;
          continue;
        }
        if (!entry.isFile()) { report.skip("special", entry.name); continue; }
        pending.push({ full, name: entry.name });
      }
      await flush(pending, relative);
    }
    await walk(top, name, [], createIgnoreStack());
  }

  const countingReport = (batch) => ({ skip: (reason, name) => skip(batch, reason, name), unreadable: () => { batch.unreadable += 1; } });
  const quietReport = { skip: () => {}, unreadable: () => {} };

  async function freeBytes(realRoot) {
    try {
      const stats = await fsPromises.statfs(realRoot);
      return Number(stats.bavail) * Number(stats.bsize);
    } catch { return null; }
  }

  /** A real limit: the drop needs more room than the disk has (minus a reserve). Null when it fits or the free space cannot be read. */
  async function spaceProblem(batch, extraBytes) {
    const free = await freeBytes(batch.realRoot);
    const needed = extraBytes + batch.pendingBytes;
    if (free === null || needed + SPACE_RESERVE_BYTES <= free) return null;
    return { needed, free };
  }

  function wakeRoom(batch) {
    const waiters = batch.roomWaiters;
    batch.roomWaiters = [];
    for (const resolve of waiters) resolve();
  }
  async function waitForRoom(batch) {
    while (batch.queue.length >= QUEUE_HIGH && !batch.cancelled) await new Promise((resolve) => { batch.roomWaiters.push(resolve); });
  }

  /** Makes an item for a walked file and hands it to the copy workers; the row is announced in the next push. */
  function enqueue(batch, file, problem) {
    const item = { id: createId(), ...file, state: "waiting", bytesDone: 0, code: null, drivePath: null, skipReason: null, abort: null };
    batch.items.set(item.id, item);
    batch.queued += 1;
    batch.pendingBytes += item.size;
    batch.added.push({ itemId: item.id, name: path.posix.basename(item.relative), relative: item.relative, size: item.size, source: item.source });
    if (problem) {
      item.state = "failed";
      item.code = "SO_UPLOAD_NOT_ENOUGH_SPACE";
      batch.pendingBytes -= item.size;
      settle(batch, item);
    } else batch.queue.push(item);
    touch(batch, item);
  }

  function settle(batch, item) {
    if (item.state === "done") batch.counts.done += 1;
    else if (item.state === "failed") batch.counts.failed += 1;
    else if (item.state === "skipped") batch.counts.skipped += 1;
    else if (item.state === "cancelled") batch.counts.cancelled += 1;
    else return;
    batch.settled.push(item.id);
    while (batch.settled.length > SETTLED_RETAIN) {
      const old = batch.items.get(batch.settled.shift());
      // A failed item stays (the person can retry it); a finished one beyond the retained window is forgotten, the counts keep it.
      if (old && old.state !== "failed" && !batch.dirty.has(old)) batch.items.delete(old.id);
    }
  }

  async function createEmptyDirectories(batch, relatives) {
    for (const relative of relatives) {
      try {
        const directory = path.join(batch.destination, relative);
        await fsPromises.mkdir(directory, { recursive: true });
        if (!isInside(batch.realRoot, await fsPromises.realpath(directory))) await fsPromises.rmdir(directory).catch(() => {});
      } catch { /* A folder that cannot be made is not worth failing the drop. */ }
    }
  }

  /** The files of one small root (or a single dropped file) go to the copy workers. */
  async function release(batch, files, empties) {
    if (batch.cancelled) return;
    const bytes = files.reduce((sum, file) => sum + file.size, 0);
    const problem = files.length > 0 ? await spaceProblem(batch, bytes) : null;
    if (problem) batch.error = { code: "SO_UPLOAD_NOT_ENOUGH_SPACE", needed: problem.needed, free: problem.free };
    await createEmptyDirectories(batch, empties);
    batch.emptyFolders += empties.length;
    for (const file of files) enqueue(batch, file, problem);
    publish(batch, { force: true });
    ensureWorkers(batch);
  }

  async function scanDirectory(batch, source, name) {
    const buffer = [];
    const empties = [];
    let files = 0;
    let bytes = 0;
    let large = false;
    await walkTree(batch, source, name, {
      file(file) {
        files += 1; bytes += file.size; batch.found += 1; batch.bytes += file.size;
        if (!large) {
          buffer.push(file);
          if (files > LARGE_FOLDER_FILES || bytes > LARGE_FOLDER_BYTES) { large = true; buffer.length = 0; empties.length = 0; }
        }
        publish(batch);
      },
      empty(relative) { if (!large) empties.push(relative); },
    }, countingReport(batch));
    if (batch.cancelled) return;
    batch.roots.push({ path: source, name, kind: "directory", files });
    if (large) batch.large.push({ path: source, name, files, bytes, state: "pending" });
    else await release(batch, buffer, empties);
  }

  async function scan(batch, requested) {
    try {
      for (const source of requested) {
        if (batch.cancelled) break;
        const name = path.basename(source);
        let stat;
        try {
          stat = await fsPromises.lstat(source);
          if (stat.isSymbolicLink()) stat = await fsPromises.stat(source);
        } catch { batch.unreadable += 1; continue; }
        const isDirectory = stat.isDirectory();
        // The person chose this item, so a folder called "build" or "dist" is not skipped for its name; hidden, lock and junk names still are.
        const reason = ignoreReason(name, false);
        if (reason) { skip(batch, reason, name); continue; }
        if (isDirectory) await scanDirectory(batch, source, name);
        else if (stat.isFile()) {
          batch.found += 1; batch.bytes += stat.size;
          batch.roots.push({ path: source, name, kind: "file", files: 1 });
          await release(batch, [{ source, relative: name, size: stat.size, mtimeMs: stat.mtimeMs }], []);
        } else skip(batch, "special", name);
      }
    } catch (error) {
      logger.debug?.("Super Drive drop scan failed.", { code: error?.code });
      batch.error = { code: classifyError(error) };
    } finally {
      batch.scanning = false;
      publish(batch, { force: true });
      batch.resolveScanned();
      maybeFinish(batch);
    }
  }

  /** "Copy into My files" for a big folder: the walk runs again as a stream, the queue bounded, rows announced in chunks. */
  async function copyLarge(batch, root) {
    const problem = await spaceProblem(batch, root.bytes);
    if (problem) {
      batch.error = { code: "SO_UPLOAD_NOT_ENOUGH_SPACE", needed: problem.needed, free: problem.free };
      root.state = "failed";
      publish(batch, { force: true });
      return;
    }
    root.state = "copying";
    const empties = [];
    await walkTree(batch, root.path, root.name, {
      async file(file) {
        enqueue(batch, file, null);
        publish(batch, { force: batch.queued === 1 });
        ensureWorkers(batch);
        await waitForRoom(batch);
      },
      empty(relative) { empties.push(relative); },
    }, quietReport);
    await createEmptyDirectories(batch, empties);
    batch.emptyFolders += empties.length;
    root.state = "copied";
    publish(batch, { force: true });
  }

  const digest = (file) => new Promise((resolve, reject) => {
    const hash = createHash("sha256");
    const stream = createReadStream(file);
    stream.on("data", (chunk) => hash.update(chunk));
    stream.on("error", reject);
    stream.on("end", () => resolve(hash.digest("hex")));
  });

  /**
   * The same bytes already sit under the final name (the same drop twice): copying again would only make "name (2)" and double the ontology.
   * Same size and modification time (a copy keeps the source's) is taken as the same file; the bytes are hashed only when those differ.
   */
  async function alreadyThere(item, finalPath) {
    const existing = await fsPromises.lstat(finalPath).catch(() => null);
    if (!existing?.isFile() || existing.size !== item.size) return false;
    if (Math.abs(existing.mtimeMs - item.mtimeMs) < 1) return true;
    try { return (await digest(item.source)) === (await digest(finalPath)); } catch { return false; }
  }

  /** Claims `name` in `directory` for the finished temporary file without ever overwriting: link, then drop the temp name. */
  async function claimName(directory, name, temporary) {
    for (let attempt = 1; attempt < 1000; attempt += 1) {
      const candidate = attempt === 1 ? name : numberedName(name, attempt);
      try {
        await fsPromises.link(temporary, path.join(directory, candidate));
        await fsPromises.unlink(temporary);
        return candidate;
      } catch (error) {
        if (error?.code === "EEXIST") continue;
        if (["EPERM", "ENOTSUP", "EXDEV", "EOPNOTSUPP"].includes(error?.code)) {
          // No hard links on this volume: check, then rename (a race here can only lose to a file that appeared a moment ago).
          const exists = await fsPromises.access(path.join(directory, candidate)).then(() => true, () => false);
          if (exists) continue;
          await fsPromises.rename(temporary, path.join(directory, candidate));
          return candidate;
        }
        throw error;
      }
    }
    throw new SuperOntologyError("SO_UPLOAD_CONFLICT");
  }

  async function copyItem(batch, item) {
    if (item.state === "cancelled" || batch.cancelled) { item.state = "cancelled"; batch.pendingBytes -= item.size; settle(batch, item); touch(batch, item); return; }
    item.state = "copying";
    touch(batch, item);
    publish(batch);
    const directory = path.dirname(path.join(batch.destination, item.relative));
    let temporary = null;
    try {
      await fsPromises.mkdir(directory, { recursive: true });
      const realDirectory = await fsPromises.realpath(directory);
      if (!isInside(batch.realRoot, realDirectory)) throw new SuperOntologyError("SO_INVALID_REQUEST");
      const finalPath = path.join(realDirectory, path.basename(item.relative));
      const posixOf = (file) => path.posix.join(...path.relative(batch.realRoot, file).split(path.sep));
      if (await alreadyThere(item, finalPath)) {
        item.bytesDone = item.size;
        item.drivePath = posixOf(finalPath);
        item.skipReason = "duplicate";
        item.state = "skipped";
        return;
      }
      temporary = path.join(realDirectory, `.oo-upload-${item.id}.part`);
      const controller = new AbortController();
      item.abort = () => controller.abort();
      const source = createReadStream(item.source, { signal: controller.signal });
      source.on("data", (chunk) => { item.bytesDone += chunk.length; touch(batch, item); publish(batch); });
      await pipeline(source, createWriteStream(temporary, { flags: "wx", mode: 0o644 }), { signal: controller.signal });
      if (Number.isFinite(item.mtimeMs)) await fsPromises.utimes(temporary, new Date(), new Date(item.mtimeMs)).catch(() => {});
      const finalName = await claimName(realDirectory, path.basename(item.relative), temporary);
      temporary = null;
      item.drivePath = posixOf(path.join(realDirectory, finalName));
      item.state = "done";
    } catch (error) {
      if (temporary) await fsPromises.rm(temporary, { force: true }).catch(() => {});
      if (item.state === "cancelled" || batch.cancelled || error?.name === "AbortError") item.state = "cancelled";
      else {
        item.state = "failed";
        item.code = error instanceof SuperOntologyError ? error.code : classifyError(error);
        logger.debug?.("Super Drive upload item failed.", { code: item.code });
      }
    } finally {
      item.abort = null;
      batch.pendingBytes -= item.size;
      settle(batch, item);
      touch(batch, item);
      publish(batch, { force: item.state === "failed" });
      maybeSync(batch);
    }
  }

  /** The engine is asked to list what has arrived while the copy still runs: first soon, then every few seconds. */
  function maybeSync(batch) {
    if (batch.syncing || batch.counts.done === batch.syncedDone) return;
    if (now() - batch.lastSync < (batch.syncedDone === 0 ? SYNC_FIRST_MS : SYNC_EVERY_MS)) return;
    void requestSync(batch, 0);
  }
  async function requestSync(batch, retries) {
    if (batch.syncing) return;
    batch.syncing = true;
    batch.lastSync = now();
    batch.syncedDone = batch.counts.done;
    try {
      for (let attempt = 0; ; attempt += 1) {
        try {
          await client.request("driveSync", { params: { id: batch.driveId }, body: {}, operationId: `sync-${createId().replaceAll("-", "")}` });
          return;
        } catch (error) {
          // A sync already running may have listed before the last files arrived: ask again shortly (only at the end). Anything else is found by the next poll.
          if (error?.code !== "DOCS_SOURCE_SYNC_RUNNING" || attempt >= retries) { logger.debug?.("Super Drive upload sync request failed.", { code: error?.code }); return; }
          await new Promise((resolve) => { setTimeout(resolve, SYNC_RETRY_MS); });
        }
      }
    } finally { batch.syncing = false; }
  }

  function ensureWorkers(batch) {
    while (batch.workers < COPY_CONCURRENCY && batch.queue.length > 0) {
      batch.workers += 1;
      void (async () => {
        try {
          for (let item = batch.queue.shift(); item; item = batch.queue.shift()) {
            if (batch.queue.length < QUEUE_LOW) wakeRoom(batch);
            await copyItem(batch, item);
          }
        } finally {
          batch.workers -= 1;
          wakeRoom(batch);
          maybeFinish(batch);
        }
      })();
    }
  }

  /** Everything walked, decided, copied: one last sync for what is still unlisted, one complete snapshot that heals a window that missed a push. */
  function maybeFinish(batch) {
    if (batch.finished || batch.scanning || batch.producers > 0 || batch.workers > 0 || batch.queue.length > 0) return;
    if (batch.large.some((root) => root.state === "pending")) return;
    batch.finished = true;
    void (async () => {
      if (batch.counts.done > 0) { while (batch.syncing) await new Promise((resolve) => { setTimeout(resolve, 20); }); await requestSync(batch, 5); }
      publish(batch, { full: true });
      while (batches.size > BATCH_SETTLE_RETAIN) batches.delete(batches.keys().next().value);
    })();
  }

  function requestedPaths(sources) {
    const seen = new Set();
    const requested = [];
    let unreadable = 0;
    for (const source of sources) {
      if (!path.isAbsolute(source)) { unreadable += 1; continue; }
      const normal = path.resolve(source);
      if (!seen.has(normal)) { seen.add(normal); requested.push(normal); }
    }
    // A path inside another dropped folder is already part of that folder.
    return { requested: requested.filter((source) => !requested.some((other) => other !== source && isInside(other, source))), unreadable };
  }

  const ownItems = (batch, folderPath) => {
    const prefix = folderPath.endsWith(path.sep) ? folderPath : `${folderPath}${path.sep}`;
    return [...batch.items.values()].filter((item) => item.source.startsWith(prefix));
  };

  return Object.freeze({
    /**
     * Answers at once with the batch id; the walk and the copy run in the background and report through `upload-progress` pushes.
     * @returns {Promise<{ batchId: string, streaming: true }>}
     */
    async start({ driveId, folder, paths }) {
      const root = await readDrive(driveId);
      const realRoot = await fsPromises.realpath(root).catch(() => { throw new SuperOntologyError("SO_UPLOAD_SOURCE_MISSING"); });
      const destination = path.join(realRoot, ...folder.split("/").filter(Boolean));
      if (!isInside(realRoot, destination)) throw new SuperOntologyError("SO_INVALID_REQUEST");
      const { requested, unreadable } = requestedPaths(paths);
      let resolveScanned;
      const scanned = new Promise((resolve) => { resolveScanned = resolve; });
      const batch = {
        id: createId(), driveId, destination, realRoot, cancelled: false, finished: false, scanning: true, lastPush: 0, dirty: new Set(), added: [],
        items: new Map(), settled: [], queue: [], roomWaiters: [], workers: 0, producers: 0, queued: 0, found: 0, bytes: 0, pendingBytes: 0,
        counts: { done: 0, failed: 0, skipped: 0, cancelled: 0 }, skipped: emptySkipped(), samples: [], unreadable, emptyFolders: 0, roots: [], large: [], error: null,
        syncing: false, syncedDone: 0, lastSync: now(), startedAt: now(), resolveScanned: () => resolveScanned(), scanned,
      };
      batches.set(batch.id, batch);
      publish(batch, { force: true });
      void scan(batch, requested);
      return { batchId: batch.id, streaming: true };
    },

    /**
     * Resolves when the walk of a batch has finished: the plan as the old synchronous start answered it. For tests and tools; the app follows pushes.
     */
    async scanned(batchId) {
      const batch = batches.get(batchId);
      if (!batch) throw new SuperOntologyError("SO_INVALID_REQUEST");
      await batch.scanned;
      return {
        items: [...batch.items.values()].map((item) => ({ itemId: item.id, name: path.posix.basename(item.relative), relative: item.relative, size: item.size, source: item.source })),
        roots: batch.roots.map((root) => ({ ...root })), skipped: { ...batch.skipped }, skippedSamples: batch.samples.slice(),
        unreadable: batch.unreadable, emptyFolders: batch.emptyFolders, large: batch.large.map((root) => ({ ...root })),
      };
    },

    /**
     * The person's answer for the big folders a drop found: "copy" streams them into the Drive, "skip" leaves them (they are linked as sources
     * instead). `paths` limits the answer to those folders; without it every folder still waiting gets it.
     */
    decide({ batchId, choice, paths }) {
      const batch = batches.get(batchId);
      if (!batch) throw new SuperOntologyError("SO_INVALID_REQUEST");
      let copying = 0;
      let skipped = 0;
      for (const root of batch.large) {
        if (root.state !== "pending" || (paths && !paths.includes(root.path))) continue;
        if (choice === "copy") {
          copying += 1;
          root.state = "copying";
          batch.producers += 1;
          void copyLarge(batch, root).catch((error) => {
            logger.debug?.("Super Drive streamed copy failed.", { code: error?.code });
            root.state = "failed";
            batch.error = { code: classifyError(error) };
          }).finally(() => { batch.producers -= 1; publish(batch, { force: true }); maybeFinish(batch); });
        } else { skipped += 1; root.state = "skipped"; }
      }
      publish(batch, { force: true });
      maybeFinish(batch);
      return { copying, skipped };
    },

    /** Cancels one item or the rest of a batch; copies in flight stop and their temporary files are removed. */
    cancel({ batchId, itemId }) {
      const batch = batches.get(batchId);
      if (!batch) throw new SuperOntologyError("SO_INVALID_REQUEST");
      for (const item of batch.items.values()) {
        if (itemId && item.id !== itemId) continue;
        if (item.state === "waiting") { item.state = "cancelled"; touch(batch, item); }
        else if (item.state === "copying") { item.state = "cancelled"; touch(batch, item); item.abort?.(); }
      }
      if (!itemId) { batch.cancelled = true; wakeRoom(batch); for (const root of batch.large) if (root.state === "pending") root.state = "skipped"; }
      publish(batch, { force: true });
      if (!itemId) { batch.scanning = false; maybeFinish(batch); }
      return { cancelled: true };
    },

    /**
     * Takes back the copies this batch made of a dropped folder, because the person chose to keep that folder in sync as a source instead: what
     * is still waiting or copying is cancelled, a finished copy is deleted only while it still has the size it was copied with, and the folders
     * this drop made are removed when they are empty. Files that were already in the Drive before the drop are never touched.
     */
    async discard({ batchId, folderPath }) {
      const batch = batches.get(batchId);
      if (!batch || !path.isAbsolute(folderPath)) throw new SuperOntologyError("SO_INVALID_REQUEST");
      const own = ownItems(batch, folderPath);
      for (const item of own) {
        if (item.state === "waiting") { item.state = "cancelled"; touch(batch, item); }
        else if (item.state === "copying") { item.state = "cancelled"; touch(batch, item); item.abort?.(); }
      }
      let removed = 0;
      const directories = new Set();
      for (const item of own) {
        if (item.state !== "done" || !item.drivePath) continue;
        const target = path.join(batch.realRoot, ...item.drivePath.split("/"));
        if (!isInside(batch.realRoot, target)) continue;
        const stat = await fsPromises.lstat(target).catch(() => null);
        if (!stat?.isFile() || stat.size !== item.size) continue;
        await fsPromises.rm(target, { force: true });
        removed += 1;
        directories.add(path.dirname(target));
      }
      for (const start of [...directories].sort((a, b) => b.length - a.length)) {
        for (let directory = start; directory.startsWith(`${batch.destination}${path.sep}`); directory = path.dirname(directory)) {
          try { await fsPromises.rmdir(directory); } catch { break; }
        }
      }
      publish(batch, { force: true });
      if (removed > 0) {
        try { await client.request("driveSync", { params: { id: batch.driveId }, body: {}, operationId: `sync-${createId().replaceAll("-", "")}` }); } catch (error) { logger.debug?.("Super Drive discard sync request failed.", { code: error?.code }); }
      }
      return { removed };
    },

    /** Stops everything (app shutdown). */
    dispose() {
      for (const batch of batches.values()) {
        batch.cancelled = true;
        wakeRoom(batch);
        for (const item of batch.items.values()) item.abort?.();
      }
    },
  });
}
