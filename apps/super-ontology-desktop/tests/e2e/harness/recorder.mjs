// Screen recording for a T2 run with hard limits. A recording ends when its scenario ends, fails or times out, and it can never outlive
// the runner: it stops itself at a time cap (screencapture's own -V, so it holds even if the runner is killed), at a size cap (polled),
// and every live recorder is killed on runner exit or signal.
import { spawn } from "node:child_process";
import fs from "node:fs";

export const DEFAULT_MAX_MS = 10 * 60_000;
export const DEFAULT_MAX_BYTES = 1024 ** 3;
const live = new Set();
let hooked = false;

const killAll = () => { for (const recorder of live) recorder.kill(); };

function hook() {
  if (hooked) return;
  hooked = true;
  process.on("exit", killAll);
  for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) process.once(signal, () => { killAll(); process.exit(128); });
}

export function stopAllRecordings() { killAll(); }

export function liveRecorders() { return live.size; }

/** Start a recording to `file`. `stop()` is idempotent and resolves once the process has exited (SIGINT, then SIGKILL after `graceMs`). */
export function startRecording(file, { maxMs = DEFAULT_MAX_MS, maxBytes = DEFAULT_MAX_BYTES, graceMs = 5000, pollMs = 2000, command = "screencapture", args } = {}) {
  hook();
  const child = spawn(command, args ?? ["-v", "-V", String(Math.ceil(maxMs / 1000)), file], { stdio: "ignore" });
  let exited = child.exitCode !== null;
  const done = new Promise((resolve) => { child.once("exit", () => { exited = true; resolve(); }); child.once("error", () => { exited = true; resolve(); }); });
  const timers = [];
  const handle = {
    pid: child.pid,
    kill() { if (!exited) { try { child.kill("SIGKILL"); } catch { /* gone */ } } },
    async stop() {
      for (const timer of timers) clearTimeout(timer);
      if (!exited) {
        try { child.kill("SIGINT"); } catch { /* gone */ }
        const force = setTimeout(() => handle.kill(), graceMs);
        await done;
        clearTimeout(force);
      }
      live.delete(handle);
    },
  };
  live.add(handle);
  const cap = setTimeout(() => { void handle.stop(); }, maxMs + graceMs); // backstop to -V
  const size = setInterval(() => { try { if (fs.statSync(file).size > maxBytes) void handle.stop(); } catch { /* not yet written */ } }, pollMs);
  timers.push(cap, size);
  for (const timer of timers) timer.unref?.();
  done.then(() => { for (const timer of timers) clearTimeout(timer); live.delete(handle); });
  return handle;
}
