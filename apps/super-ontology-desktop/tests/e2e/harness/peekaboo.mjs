// Operating a packaged app through Peekaboo (volume 25 section 6, T2): every action addresses an element by its accessible name, never by a
// test id. macOS Accessibility and Screen Recording permission must be granted to the host that runs Peekaboo; `permissions()` reports it.
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const execute = promisify(execFile);

// The runner is started from a Terminal window (run-t2.command), and that window is the process the permissions were granted to, so
// Peekaboo runs in-process (`--no-remote`) instead of asking a bridge host that may not hold them. SO_PEEKABOO_FLAGS overrides it.
const DEFAULT_FLAGS = (process.env.SO_PEEKABOO_FLAGS ?? "--no-remote").split(" ").filter(Boolean);

export function createPeekaboo({ app, binary = "peekaboo", flags = DEFAULT_FLAGS, run = (file, args, options) => execute(file, args, { maxBuffer: 16 * 1024 * 1024, ...options }) } = {}) {
  // Each launch is a new process: the harness points Peekaboo at that exact PID (`PID:<n>`) and drops cached snapshots, so a click never
  // resolves to the window of the process a previous scenario or relaunch ended ("window not found" / "snapshot is stale").
  const target = ["--app", app];
  // A failed call says why: Peekaboo's own message (stderr, else stdout) is part of the error.
  const call = (args, options) => run(binary, [...args, ...flags], options).catch((error) => {
    error.message = `${error.message}\n${String(error.stderr || error.stdout || "").trim().slice(0, 600)}`;
    throw error;
  });
  // The app's window can be replaced while Peekaboo prepares a capture ("window changed", "target not found"): look again before failing.
  const retrying = async (action, tries = 8) => { for (let attempt = 1; ; attempt += 1) { try { return await action(); } catch (error) { if (attempt >= tries) throw error; await new Promise((resolve) => { setTimeout(resolve, 1500); }); } } };
  return Object.freeze({
    /** Point every later call at this process and forget snapshots of the ones before it. */
    async retarget(pid) { target[1] = `PID:${pid}`; await run(binary, ["clean", "--all-snapshots", ...flags], {}).catch(() => undefined); },
    /** `{ granted, missing, raw }`: Screen Recording and Accessibility are required. */
    async permissions() {
      const { stdout } = await call(["permissions"]).catch((error) => ({ stdout: `${error.stdout ?? ""}${error.stderr ?? ""}` }));
      const missing = [...stdout.matchAll(/^(.+?) \(Required\): Not Granted/gmu)].map((match) => match[1]);
      return { granted: missing.length === 0 && /Required\): Granted/u.test(stdout), missing, raw: stdout };
    },
    // A click by name resolves against the latest snapshot, which shows the screen BEFORE the previous action; observe first so the name is looked up on what is on screen now.
    click: (name, { right = false, double = false, modifiers } = {}) => retrying(async () => { await call(["see", ...target, "--json"]); return call(["click", name, ...target, "--foreground", "--wait-for", "10s", ...(right ? ["--right"] : []), ...(double ? ["--double"] : []), ...(modifiers ? ["--modifiers", modifiers] : [])]); }, 4),
    /** Types into the focused element: click the field first. */
    // `native`: the window in front is the app's own open panel (a second window of the process, so a process target is ambiguous): no target, the keys go to the key window.
    // Peekaboo refuses to confirm typing into a window it cannot pin (the open panel), so that one goes through System Events (same Accessibility grant).
    type: (text, { native = false } = {}) => (native ? run("osascript", ["-e", "on run argv", "-e", "tell application \"System Events\" to keystroke (item 1 of argv)", "-e", "end run", text], {}) : call(["type", text, ...target])),
    /** Return through System Events (the open panel needs Return, keypad Enter, Return: complete the path, go there, choose). */
    nativeReturn: (code = 36) => run("osascript", ["-e", `tell application "System Events" to key code ${code}`], {}),
    press: (chord, { native = false } = {}) => call(["press", chord, ...(native ? [] : target), "--foreground"]),
    see: (path, { json = false } = {}) => retrying(() => call(["see", ...target, "--path", path, "--format", "png", ...(json ? ["--json"] : [])])),
  });
}
