import { randomUUID } from "node:crypto";
import fs from "node:fs/promises";
import { constants as fsConstants, createReadStream, createWriteStream } from "node:fs";
import path from "node:path";
import { pipeline } from "node:stream/promises";
import { SuperOntologyError } from "../../shared/super-ontology/errors.mjs";

/** At most this many files in one drop; a larger tree is refused instead of silently truncated. */
export const MAX_UPLOAD_ITEMS = 10_000;
const COPY_CONCURRENCY = 2;
const PROGRESS_INTERVAL_MS = 120;
const BATCH_SETTLE_RETAIN = 20;
/** Names the engine ignores too (10 §4.3.2): hidden files, partial downloads, editor and OS litter. */
const IGNORED_NAME = /^(\.|~\$)|\.(part|tmp|swp|crdownload)$|^(thumbs\.db|desktop\.ini)$/iu;

export const isIgnoredName = (name) => IGNORED_NAME.test(name);

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

  function publish(batch, force = false) {
    const time = now();
    if (!force && time - batch.lastPush < PROGRESS_INTERVAL_MS) return;
    batch.lastPush = time;
    broadcast("upload-progress", {
      batchId: batch.id,
      items: batch.items.map((item) => ({
        itemId: item.id, state: item.state, bytesDone: item.bytesDone, bytesTotal: item.size,
        ...(item.code ? { code: item.code } : {}), ...(item.drivePath ? { drivePath: item.drivePath } : {}),
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

  /** Walks the dropped paths into a flat plan; hidden and temporary names and links are counted, not copied. */
  async function plan(sources) {
    const files = [];
    const skipped = { hidden: 0, links: 0 };
    const missing = [];
    async function walk(source, relative) {
      const stat = await fsPromises.lstat(source);
      if (stat.isSymbolicLink()) { skipped.links += 1; return; }
      if (stat.isDirectory()) {
        for (const entry of (await fsPromises.readdir(source)).sort()) {
          if (isIgnoredName(entry)) { skipped.hidden += 1; continue; }
          await walk(path.join(source, entry), path.posix.join(relative, entry));
        }
        return;
      }
      if (!stat.isFile()) return;
      if (files.length >= MAX_UPLOAD_ITEMS) throw new SuperOntologyError("SO_INVALID_REQUEST");
      files.push({ source, relative, size: stat.size });
    }
    for (const source of sources) {
      if (!path.isAbsolute(source)) { missing.push(source); continue; }
      const name = path.basename(source);
      if (isIgnoredName(name)) { skipped.hidden += 1; continue; }
      try { await walk(source, name); } catch (error) {
        if (error instanceof SuperOntologyError) throw error;
        missing.push(source);
      }
    }
    return { files, skipped, missingCount: missing.length };
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
    if (item.state === "cancelled" || batch.cancelled) { item.state = "cancelled"; return; }
    item.state = "copying";
    publish(batch, true);
    const directory = path.dirname(path.join(batch.destination, item.relative));
    let temporary = null;
    try {
      await fsPromises.mkdir(directory, { recursive: true });
      const realDirectory = await fsPromises.realpath(directory);
      if (!isInside(batch.realRoot, realDirectory)) throw new SuperOntologyError("SO_INVALID_REQUEST");
      temporary = path.join(realDirectory, `.so-upload-${item.id}.part`);
      const controller = new AbortController();
      item.abort = () => controller.abort();
      const source = createReadStream(item.source, { signal: controller.signal });
      source.on("data", (chunk) => { item.bytesDone += chunk.length; publish(batch); });
      await pipeline(source, createWriteStream(temporary, { flags: "wx", mode: 0o644 }), { signal: controller.signal });
      const finalName = await claimName(realDirectory, path.basename(item.relative), temporary);
      temporary = null;
      item.drivePath = path.posix.join(...path.relative(batch.realRoot, path.join(realDirectory, finalName)).split(path.sep));
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
      publish(batch, true);
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
    publish(batch, true);
    while (batches.size > BATCH_SETTLE_RETAIN) batches.delete(batches.keys().next().value);
  }

  return Object.freeze({
    /** @returns {Promise<{ batchId: string, items: object[], skipped: { hidden: number, links: number }, unreadable: number }>} */
    async start({ driveId, folder, paths }) {
      const root = await readDrive(driveId);
      const realRoot = await fsPromises.realpath(root).catch(() => { throw new SuperOntologyError("SO_UPLOAD_SOURCE_MISSING"); });
      const destination = path.join(realRoot, ...folder.split("/").filter(Boolean));
      if (!isInside(realRoot, destination)) throw new SuperOntologyError("SO_INVALID_REQUEST");
      const { files, skipped, missingCount } = await plan(paths);
      const batch = {
        id: createId(), driveId, destination, realRoot, cancelled: false, finished: false, lastPush: 0,
        items: files.map((file) => ({ id: createId(), ...file, state: "waiting", bytesDone: 0, code: null, drivePath: null, abort: null })),
      };
      batches.set(batch.id, batch);
      const summary = batch.items.map((item) => ({ itemId: item.id, name: path.posix.basename(item.relative), relative: item.relative, size: item.size, source: item.source }));
      if (batch.items.length > 0) void run(batch);
      else batch.finished = true;
      return { batchId: batch.id, items: summary, skipped, unreadable: missingCount };
    },

    /** Cancels one item or the rest of a batch; copies in flight stop and their temporary files are removed. */
    cancel({ batchId, itemId }) {
      const batch = batches.get(batchId);
      if (!batch) throw new SuperOntologyError("SO_INVALID_REQUEST");
      for (const item of batch.items) {
        if (itemId && item.id !== itemId) continue;
        if (item.state === "waiting") item.state = "cancelled";
        else if (item.state === "copying") { item.state = "cancelled"; item.abort?.(); }
      }
      if (!itemId) batch.cancelled = true;
      publish(batch, true);
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
