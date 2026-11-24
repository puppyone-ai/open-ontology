import { spawn as nodeSpawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";
import { SuperOntologyError } from "../../../src/shared/super-ontology/errors.mjs";

/** A long recording takes minutes to decode; the cap is a safety net, the user can cancel at any time. */
export const DEFAULT_DECODE_TIMEOUT_MS = 30 * 60 * 1000;
const KILL_GRACE_MS = 2_000;
const SHA256 = /^[0-9a-f]{64}$/u;

/**
 * Runs the macOS host decode helper (F-S10-05, D65) for one media file at a time per request and puts its output where the
 * engine reads it: `<exchangeRoot>/<sha256 of the file>/` (the engine mounts the exchange root read-only). The helper
 * never touches the network and writes only into that directory; this module checks the paths it is given, bounds the run
 * with a timeout and an abort signal, and reaps every child at shutdown.
 *
 * @param {object} options
 * @param {string | null} options.helperPath  absolute path of the helper binary, or null where there is none (Windows, Linux)
 * @param {string | null} options.exchangeRoot  absolute directory that `OO_DOCS_EXCHANGE` points at
 */
export function createMediaDecoder({
  helperPath: givenHelperPath = null,
  locateHelper = null,
  exchangeRoot,
  platform = process.platform,
  spawn = nodeSpawn,
  fsPromises = fs,
  hashFile = sha256OfFile,
  timers = { setTimeout, clearTimeout },
  logger = console,
} = {}) {
  const running = new Set();
  let shutDown = false;
  let helperPath = givenHelperPath;
  let located = givenHelperPath !== null || locateHelper === null;

  /** Whether this computer can decode at all; the helper itself is looked up on first use (`ready`). */
  const available = () => platform === "darwin" && typeof exchangeRoot === "string" && path.isAbsolute(exchangeRoot) && (located ? typeof helperPath === "string" && path.isAbsolute(helperPath) : true);

  async function ready() {
    if (!located) {
      located = true;
      helperPath = await locateHelper().catch(() => null);
    }
    return available();
  }

  async function decode({ sourcePath, timeoutMs = DEFAULT_DECODE_TIMEOUT_MS, signal, onProgress } = {}) {
    if (!(await ready())) throw new SuperOntologyError("SO_PLATFORM_UNSUPPORTED");
    if (shutDown) throw new SuperOntologyError("SO_MEDIA_CANCELLED");
    if (typeof sourcePath !== "string" || !path.isAbsolute(sourcePath)) throw new SuperOntologyError("SO_INVALID_REQUEST");
    let stat;
    try { stat = await fsPromises.stat(sourcePath); } catch { throw new SuperOntologyError("SO_UPLOAD_SOURCE_MISSING"); }
    if (!stat.isFile()) throw new SuperOntologyError("SO_UPLOAD_SOURCE_MISSING");
    const digest = await hashFile(sourcePath);
    if (!SHA256.test(digest)) throw new SuperOntologyError("SO_MEDIA_DECODE_FAILED");
    const output = path.join(exchangeRoot, digest);
    // The output is always one level below the exchange root and named by the digest: nothing a request says can move it.
    if (path.dirname(output) !== path.normalize(exchangeRoot).replace(/\/$/u, "")) throw new SuperOntologyError("SO_INVALID_REQUEST");
    await fsPromises.mkdir(exchangeRoot, { recursive: true, mode: 0o700 });

    return new Promise((resolve, reject) => {
      let child;
      try {
        child = spawn(helperPath, ["--input", sourcePath, "--output", output, "--expect-sha256", digest], { stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
      } catch {
        reject(new SuperOntologyError("SO_MEDIA_DECODE_FAILED"));
        return;
      }
      const entry = { child, finished: false };
      running.add(entry);
      let stdout = "";
      let stderrBuffer = "";
      let outcome = null;
      let killTimer = null;
      const stop = (reason) => {
        if (entry.finished || outcome) return;
        outcome = reason;
        child.kill("SIGTERM");
        killTimer = timers.setTimeout(() => { child.kill("SIGKILL"); }, KILL_GRACE_MS);
        killTimer.unref?.();
      };
      const deadline = timers.setTimeout(() => stop("timeout"), timeoutMs);
      deadline.unref?.();
      const onAbort = () => stop("cancelled");
      if (signal?.aborted) onAbort(); else signal?.addEventListener?.("abort", onAbort, { once: true });

      child.stdout?.on("data", (chunk) => { if (stdout.length < 1 << 20) stdout += chunk.toString("utf8"); });
      child.stderr?.on("data", (chunk) => {
        stderrBuffer += chunk.toString("utf8");
        let newline = stderrBuffer.indexOf("\n");
        while (newline !== -1) {
          const line = stderrBuffer.slice(0, newline);
          stderrBuffer = stderrBuffer.slice(newline + 1);
          newline = stderrBuffer.indexOf("\n");
          try {
            const parsed = JSON.parse(line);
            if (parsed?.progress && Number.isFinite(parsed.progress.done_ms)) onProgress?.({ doneMs: parsed.progress.done_ms, totalMs: parsed.progress.total_ms, stage: String(parsed.progress.stage ?? "") });
          } catch { /* Not a progress line: ignored. */ }
        }
      });
      child.once("error", () => {
        finish();
        reject(new SuperOntologyError("SO_MEDIA_DECODE_FAILED"));
      });
      child.once("close", (code) => {
        finish();
        if (outcome === "timeout") { fsPromises.rm(output, { recursive: true, force: true }).catch(() => {}); reject(new SuperOntologyError("SO_TIMEOUT", { retryable: true })); return; }
        if (outcome === "cancelled") { reject(new SuperOntologyError("SO_MEDIA_CANCELLED")); return; }
        if (code === 0) {
          try {
            const manifest = JSON.parse(stdout);
            if (manifest?.source_blob_sha256 !== digest) throw new Error("digest");
            resolve(Object.freeze({
              sha256: digest, helperVersion: String(manifest.helper_version ?? ""), durationMs: Number(manifest.duration_ms) || 0,
              hasAudio: typeof manifest.audio === "string", frames: Array.isArray(manifest.pts_ms) ? manifest.pts_ms.length : 0, still: typeof manifest.image === "string",
            }));
          } catch { reject(new SuperOntologyError("SO_MEDIA_DECODE_FAILED")); }
          return;
        }
        logger.debug?.("Media decoder stopped.", { code });
        // 6: the file changed after it was queued; the engine's own check would say the same (DOCS_PARSE_MEDIA_HOST_DECODE_MISMATCH).
        reject(new SuperOntologyError(code === 6 ? "DOCS_PARSE_MEDIA_HOST_DECODE_MISMATCH" : code === 3 ? "SO_UPLOAD_SOURCE_MISSING" : "SO_MEDIA_DECODE_FAILED", { retryable: code === 6 }));
      });

      function finish() {
        entry.finished = true;
        running.delete(entry);
        timers.clearTimeout(deadline);
        if (killTimer) timers.clearTimeout(killTimer);
        signal?.removeEventListener?.("abort", onAbort);
      }
    });
  }

  /** Stops every child and refuses new work: no helper outlives the app. */
  async function shutdown() {
    shutDown = true;
    for (const entry of [...running]) entry.child.kill("SIGKILL");
  }

  return Object.freeze({ available, ready, decode, shutdown, activeCount: () => running.size });
}

/**
 * Where the helper is: next to the app's resources when packaged (`extraResources`), otherwise in the build folder of the
 * repository. Returns `null` off macOS so nothing else ever tries to run it (the Windows build does not carry it, D33).
 */
export async function resolveMediaDecoderPath({ platform = process.platform, resourcesPath = process.resourcesPath, repositoryRoot, fsPromises = fs } = {}) {
  if (platform !== "darwin") return null;
  const candidates = [
    ...(typeof resourcesPath === "string" && resourcesPath ? [
      path.join(resourcesPath, "app.asar.unpacked", "native", "super-drive-media-decoder", "build", "super-drive-media-decoder"),
      path.join(resourcesPath, "super-drive-media-decoder", "super-drive-media-decoder"),
    ] : []),
    ...(repositoryRoot ? [path.join(repositoryRoot, "native", "super-drive-media-decoder", "build", "super-drive-media-decoder")] : []),
  ];
  for (const candidate of candidates) {
    try { await fsPromises.access(candidate, fs.constants?.X_OK ?? 1); return candidate; } catch { /* Try the next place. */ }
  }
  return null;
}

async function sha256OfFile(file) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest("hex");
}
