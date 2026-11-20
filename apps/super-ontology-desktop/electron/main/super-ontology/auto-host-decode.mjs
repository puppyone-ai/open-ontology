import { isDrive, isPage, ensure } from "./response-guards.mjs";

/** How often the engine is asked for files that wait for a host decode. A tick with nothing to do is one cheap list call per Drive. */
export const AUTO_DECODE_INTERVAL_MS = 3_000;
const PAGE_LIMIT = 200;
const CODEC_UNAVAILABLE = "DOCS_PARSE_CODEC_UNAVAILABLE";

/**
 * Host decode without a button (D65, F-S10-05). The engine's own FFmpeg has no AAC or H.264 decoder by design, so an `.m4a` or `.mp4`
 * a person dropped ends `unsupported` with DOCS_PARSE_CODEC_UNAVAILABLE. On a Mac that is a file the system codecs can read: this
 * decodes it into the exchange directory through `decode` (the same function the `media-decode` channel runs) and asks the engine to
 * retry it, so the person never has to know. Each file version is tried once; a failed decode leaves the file as it was, with its
 * reason, and is not repeated until the file changes.
 */
export function createAutoHostDecode({ client, decode, logger = console, timers = { setTimeout, clearTimeout }, intervalMs = AUTO_DECODE_INTERVAL_MS, createId = () => globalThis.crypto.randomUUID() } = {}) {
  /** fileId:b3 of the versions already handled, so one failure is not retried every tick. */
  const tried = new Set();
  let timer = null;
  let running = false;
  let stopped = true;

  async function listDrives() {
    const drives = [];
    let cursor;
    for (let page = 0; page < 50; page += 1) {
      const { value } = await client.request("drivesList", { query: { limit: PAGE_LIMIT, cursor } });
      ensure(value, isPage);
      for (const item of value.items) drives.push(ensure(item, isDrive));
      cursor = value.next_cursor ?? value.nextCursor ?? null;
      if (!cursor) break;
    }
    return drives;
  }

  async function waiting(driveId) {
    const found = [];
    let cursor;
    for (let page = 0; page < 50; page += 1) {
      const { value } = await client.request("filesList", { params: { id: driveId }, query: { recursive: true, state: ["unsupported"], limit: PAGE_LIMIT, cursor } });
      ensure(value, isPage);
      for (const file of value.items) if (file?.state_code === CODEC_UNAVAILABLE && typeof file.id === "string") found.push(file);
      cursor = value.next_cursor ?? value.nextCursor ?? null;
      if (!cursor) break;
    }
    return found;
  }

  /** One pass; returns how many files were decoded and sent back. */
  async function tick() {
    if (running || stopped) return 0;
    running = true;
    let done = 0;
    try {
      for (const drive of await listDrives()) {
        if (drive.backend !== "fs") continue;
        for (const file of await waiting(drive.id)) {
          const key = `${file.id}:${file.b3 ?? file.revision ?? ""}`;
          if (tried.has(key) || stopped) continue;
          tried.add(key);
          try {
            await decode(file.id);
            await client.request("filesRetry", { body: { file_ids: [file.id] }, operationId: `host-decode-${createId()}` });
            done += 1;
          } catch (error) {
            logger.warn?.("Host decode of a media file failed.", { code: error?.code ?? "SO_MEDIA_DECODE_FAILED" });
          }
        }
      }
    } catch (error) {
      logger.debug?.("Host decode pass failed.", { code: error?.code });
    } finally {
      running = false;
    }
    return done;
  }

  function schedule() {
    if (stopped) return;
    timer = timers.setTimeout(async () => { timer = null; await tick(); schedule(); }, intervalMs);
  }

  return Object.freeze({
    tick,
    start() { if (!stopped) return; stopped = false; schedule(); },
    stop() { stopped = true; if (timer !== null) { timers.clearTimeout(timer); timer = null; } },
    /** For tests: the versions handled so far. */
    handled: () => tried.size,
  });
}
