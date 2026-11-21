import { createHash, randomUUID } from "node:crypto";
import fs from "node:fs/promises";
import { createReadStream, createWriteStream } from "node:fs";
import path from "node:path";
import { pipeline } from "node:stream/promises";
import { SuperOntologyError } from "../../../src/shared/super-ontology/errors.mjs";

/** At most this many files in one drop (the engine's own per-Drive limit); a larger tree is refused instead of silently truncated. */
export const MAX_UPLOAD_ITEMS = 10_000;
const COPY_CONCURRENCY = 2;
const PROGRESS_INTERVAL_MS = 120;
const BATCH_SETTLE_RETAIN = 20;
/** How many skipped names a drop reports back, so a drop of thousands of files stays a small answer. */
const SKIP_SAMPLE_LIMIT = 12;
const TEMPORARY_NAME = /^~\$|\.(part|tmp|swp|crdownload)$/iu;
const JUNK_NAME = /^(thumbs\.db|desktop\.ini|node_modules|__pycache__)$/iu;

/**
 * Why the engine would ignore a name (10 §4.3.2), or null: "hidden" (a leading dot: .git, .DS_Store, `.~lock.*#`), "temporary" (Office lock files,
 * partial downloads, editor swap files) or "junk" (OS metadata, node_modules, __pycache__). Copying such files only to have the engine skip them
 * wastes the drop, so they are counted and named in the answer instead.
 */
export function ignoreReason(name) {
  if (name.startsWith(".")) return "hidden";
  if (TEMPORARY_NAME.test(name)) return "temporary";
  if (JUNK_NAME.test(name)) return "junk";
  return null;
}

export const isIgnoredName = (name) => ignoreReason(name) !== null;

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

/**
 * Copies files the user dropped into an fs Drive's folder (20-frontend §3.2.6): a temporary name the engine ignores,
 * then an atomic claim of the final name (a numbered name on a clash), then one `:sync`. Only the main process
 * touches the disk; the renderer sends paths from the OS drop and receives progress pushes.
 *
 * @param {object} options
 * @param {{ request: Function }} options.client control API client
 * @param {(channel: string, payload: unknown) => void} options.broadcast
 */
export function createUploadService({ client, broadcast, fsPromises = fs, createId = randomUUID, now = () => Date.now(), logger = {} }) {
  const batches = new Map();

  /** A change to an item is queued for the next push; a push carries only what changed, a finished batch sends everything once. */
  function touch(batch, item) { batch.dirty.add(item); }

  function publish(batch, { force = false, full = false } = {}) {
    const time = now();
    if (!force && !full && time - batch.lastPush < PROGRESS_INTERVAL_MS) return;
    const changed = full ? batch.items : [...batch.dirty];
    batch.dirty.clear();
    if (changed.length === 0 && !full) return;
    batch.lastPush = time;
    broadcast("upload-progress", {
      batchId: batch.id,
      items: changed.map((item) => ({
        itemId: item.id, state: item.state, bytesDone: item.bytesDone, bytesTotal: item.size,
        ...(item.code ? { code: item.code } : {}), ...(item.drivePath ? { drivePath: item.drivePath } : {}), ...(item.skipReason ? { skipReason: item.skipReason } : {}),
      })),
    });
  }

  async function readDrive(driveId) {
    const { value } = await client.request("driveGet", { params: { id: driveId } });
    if (value?.backend !== "fs") throw new SuperOntologyError("SO_UPLOAD_UNSUPPORTED_BACKEND");
    const root = value?.config?.root;
    if (typeof root !== "string" || !path.isAbsolute(root)) throw new SuperOntologyError("SO_UPLOAD_UNSUPPORTED_BACKEND");
    return root;
  }

  /**
   * Walks the dropped paths into a flat plan (a folder keeps its name and structure, `ProjectA/sub/x.md`). Names the engine ignores, links and
   * special files are counted and sampled, not copied; a directory or file that cannot be read is counted and the rest of the tree goes on.
   * A link below a dropped folder is never followed (it could point outside the folder the person chose); a dropped link itself is read once.
   */
  async function plan(sources) {
    const files = [];
    const emptyDirectories = [];
    const roots = [];
    const skipped = { hidden: 0, links: 0, junk: 0, temporary: 0, special: 0 };
    const samples = [];
    let unreadable = 0;
    const skip = (reason, name) => {
      skipped[reason] += 1;
      if (samples.length < SKIP_SAMPLE_LIMIT) samples.push({ name, reason });
    };

    async function walk(directory, relative) {
      let entries;
      try { entries = await fsPromises.readdir(directory, { withFileTypes: true }); } catch { unreadable += 1; return; }
      if (entries.length === 0) { emptyDirectories.push(relative); return; }
      entries.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
      for (const entry of entries) {
        const reason = ignoreReason(entry.name);
        if (reason) { skip(reason, entry.name); continue; }
        const full = path.join(directory, entry.name);
        const child = path.posix.join(relative, entry.name);
        if (entry.isSymbolicLink()) { skip("links", entry.name); continue; }
        if (entry.isDirectory()) { await walk(full, child); continue; }
        if (!entry.isFile()) { skip("special", entry.name); continue; }
        if (files.length >= MAX_UPLOAD_ITEMS) throw new SuperOntologyError("SO_UPLOAD_TOO_MANY");
        try {
          const stat = await fsPromises.lstat(full);
          files.push({ source: full, relative: child, size: stat.size });
        } catch { unreadable += 1; }
      }
    }

    const seen = new Set();
    const requested = [];
    for (const source of sources) {
      if (!path.isAbsolute(source)) { unreadable += 1; continue; }
      const normal = path.resolve(source);
      if (!seen.has(normal)) { seen.add(normal); requested.push(normal); }
    }
    for (const source of requested) {
      // A path inside another dropped folder is already part of that folder.
      if (requested.some((other) => other !== source && isInside(other, source))) continue;
      const name = path.basename(source);
      const reason = ignoreReason(name);
      if (reason) { skip(reason, name); continue; }
      let stat;
      try {
        stat = await fsPromises.lstat(source);
        if (stat.isSymbolicLink()) stat = await fsPromises.stat(source);
      } catch { unreadable += 1; continue; }
      if (stat.isDirectory()) {
        const before = files.length;
        await walk(source, name);
        roots.push({ path: source, name, kind: "directory", files: files.length - before });
      } else if (stat.isFile()) {
        if (files.length >= MAX_UPLOAD_ITEMS) throw new SuperOntologyError("SO_UPLOAD_TOO_MANY");
        files.push({ source, relative: name, size: stat.size });
        roots.push({ path: source, name, kind: "file", files: 1 });
      } else skip("special", name);
    }
    return { files, emptyDirectories, roots, skipped, samples, unreadable };
  }

  const digest = (file) => new Promise((resolve, reject) => {
    const hash = createHash("sha256");
    const stream = createReadStream(file);
    stream.on("data", (chunk) => hash.update(chunk));
    stream.on("error", reject);
    stream.on("end", () => resolve(hash.digest("hex")));
  });

  /** The same bytes already sit under the final name (the same drop twice): copying again would only make "name (2)" and double the ontology. */
  async function alreadyThere(item, finalPath) {
    const existing = await fsPromises.lstat(finalPath).catch(() => null);
    if (!existing?.isFile() || existing.size !== item.size) return false;
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
    if (item.state === "cancelled" || batch.cancelled) { item.state = "cancelled"; touch(batch, item); return; }
    item.state = "copying";
    touch(batch, item);
    publish(batch, { force: true });
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
      touch(batch, item);
      publish(batch, { force: true });
    }
  }

  async function run(batch) {
    const queue = [...batch.items];
    const workers = Array.from({ length: Math.min(COPY_CONCURRENCY, queue.length) }, async () => {
      for (let item = queue.shift(); item; item = queue.shift()) await copyItem(batch, item);
    });
    await Promise.all(workers);
    batch.finished = true;
    if (batch.items.some((item) => item.state === "done")) {
      try {
        await client.request("driveSync", { params: { id: batch.driveId }, body: {}, operationId: `sync-${batch.id.replaceAll("-", "")}` });
      } catch (error) {
        // A sync already running will pick the files up; anything else is found by the next poll.
        logger.debug?.("Super Drive upload sync request failed.", { code: error?.code });
      }
    }
    // One complete snapshot at the end heals a window that missed an earlier, partial push.
    publish(batch, { full: true });
    while (batches.size > BATCH_SETTLE_RETAIN) batches.delete(batches.keys().next().value);
  }

  /** Empty folders in the drop are kept as folders (the engine lists files only, but the person's structure stays visible in a file manager). */
  async function createEmptyDirectories(batch, relatives) {
    for (const relative of relatives) {
      try {
        const directory = path.join(batch.destination, relative);
        await fsPromises.mkdir(directory, { recursive: true });
        if (!isInside(batch.realRoot, await fsPromises.realpath(directory))) await fsPromises.rmdir(directory).catch(() => {});
      } catch { /* A folder that cannot be made is not worth failing the drop. */ }
    }
  }

  return Object.freeze({
    /**
     * @returns {Promise<{ batchId: string, items: object[], roots: { path: string, name: string, kind: "file" | "directory", files: number }[],
     *   skipped: { hidden: number, links: number, junk: number, temporary: number, special: number }, skippedSamples: { name: string, reason: string }[],
     *   unreadable: number, emptyFolders: number }>}
     */
    async start({ driveId, folder, paths }) {
      const root = await readDrive(driveId);
      const realRoot = await fsPromises.realpath(root).catch(() => { throw new SuperOntologyError("SO_UPLOAD_SOURCE_MISSING"); });
      const destination = path.join(realRoot, ...folder.split("/").filter(Boolean));
      if (!isInside(realRoot, destination)) throw new SuperOntologyError("SO_INVALID_REQUEST");
      const { files, emptyDirectories, roots, skipped, samples, unreadable } = await plan(paths);
      const batch = {
        id: createId(), driveId, destination, realRoot, cancelled: false, finished: false, lastPush: 0, dirty: new Set(),
        items: files.map((file) => ({ id: createId(), ...file, state: "waiting", bytesDone: 0, code: null, drivePath: null, skipReason: null, abort: null })),
      };
      batches.set(batch.id, batch);
      await createEmptyDirectories(batch, emptyDirectories);
      const summary = batch.items.map((item) => ({ itemId: item.id, name: path.posix.basename(item.relative), relative: item.relative, size: item.size, source: item.source }));
      if (batch.items.length > 0) void run(batch);
      else batch.finished = true;
      return { batchId: batch.id, items: summary, roots, skipped, skippedSamples: samples, unreadable, emptyFolders: emptyDirectories.length };
    },

    /** Cancels one item or the rest of a batch; copies in flight stop and their temporary files are removed. */
    cancel({ batchId, itemId }) {
      const batch = batches.get(batchId);
      if (!batch) throw new SuperOntologyError("SO_INVALID_REQUEST");
      for (const item of batch.items) {
        if (itemId && item.id !== itemId) continue;
        if (item.state === "waiting") { item.state = "cancelled"; touch(batch, item); }
        else if (item.state === "copying") { item.state = "cancelled"; touch(batch, item); item.abort?.(); }
      }
      if (!itemId) batch.cancelled = true;
      publish(batch, { force: true });
      return { cancelled: true };
    },

    /** Stops everything (app shutdown). */
    dispose() {
      for (const batch of batches.values()) {
        batch.cancelled = true;
        for (const item of batch.items) item.abort?.();
      }
    },
  });
}
