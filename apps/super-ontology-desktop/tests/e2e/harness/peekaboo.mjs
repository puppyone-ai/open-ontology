// Operating a packaged app through Peekaboo (volume 25 section 6, T2): every action addresses an element by its accessible name, never by a
// test id. macOS Accessibility and Screen Recording permission must be granted to the host that runs Peekaboo; `permissions()` reports it.
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const execute = promisify(execFile);

// The runner is started from a Terminal window (run-t2.command), and that window is the process the permissions were granted to, so
// Peekaboo runs in-process (`--no-remote`) instead of asking a bridge host that may not hold them. SO_PEEKABOO_FLAGS overrides it.
const DEFAULT_FLAGS = (process.env.SO_PEEKABOO_FLAGS ?? "--no-remote").split(" ").filter(Boolean);

// Chromium nests the page deep in the accessibility tree; with Peekaboo's default depth the text fields and the buttons of a screen's body are cut off
// ("ax_truncated_depth") and cannot be addressed by name. Walk deeper.
const AX_ENV = { PEEKABOO_AX_MAX_DEPTH: process.env.PEEKABOO_AX_MAX_DEPTH ?? "40" };

export function createPeekaboo({ app, binary = "peekaboo", flags = DEFAULT_FLAGS, run = (file, args, options) => execute(file, args, { maxBuffer: 16 * 1024 * 1024, ...options, env: { ...process.env, ...AX_ENV, ...options?.env } }) } = {}) {
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
    // A query matches by substring ("Ontology" also finds the row "Ontology version 1" on Home), so the observation is searched for an element whose accessible
    // name IS the name and that one is clicked by its id; only a name nothing is called exactly goes to Peekaboo as a query.
    click: (name, { right = false, double = false, modifiers, strategy } = {}) => retrying(async () => {
      const { stdout } = await call(["see", ...target, "--json", "--web-focus"]);
      let found = null;
      try {
        const data = JSON.parse(stdout).data;
        const exact = (data.ui_elements ?? []).filter((element) => element.is_actionable && (element.label === name || element.title === name));
        // The last one: a dialog is the last thing in the page, and its button shares the name of the button that opened it.
        const element = exact.filter((entry) => ["button", "checkbox", "textField", "radioButton", "link", "menuItem"].includes(entry.role)).at(-1) ?? exact.at(-1);
        if (element) found = { id: element.id, snapshot: data.snapshot_id };
      } catch { /* not JSON: fall back to the query */ }
      const options = ["--foreground", "--wait-for", "10s", ...(right ? ["--right"] : []), ...(double ? ["--double"] : []), ...(modifiers ? ["--modifiers", modifiers] : []), ...(strategy ? ["--input-strategy", strategy] : [])];
      return call(found ? ["click", "--on", found.id, "--snapshot", found.snapshot, ...target, ...options] : ["click", name, ...target, ...options]);
    }, 4),
    /** Types into the focused element: click the field first. */
    // `native`: the window in front is the app's own open panel (a second window of the process, so a process target is ambiguous): no target, the keys go to the key window.
    // Peekaboo refuses to confirm typing into a window it cannot pin (the open panel), so that one goes through System Events (same Accessibility grant).
    type: (text, { native = false } = {}) => (native ? run("osascript", ["-e", "on run argv", "-e", "tell application \"System Events\" to keystroke (item 1 of argv)", "-e", "end run", text], {}) : call(["type", text, ...target])),
    /** Return through System Events (the open panel needs Return, keypad Enter, Return: complete the path, go there, choose). */
    nativeReturn: (code = 36) => run("osascript", ["-e", `tell application "System Events" to key code ${code}`], {}),
    press: (chord, { native = false } = {}) => call(["press", chord, ...(native ? [] : target), "--foreground"]),
    /** Drags from one global point to another (a file from a Finder window onto the app): across apps, so no target and the real pointer. */
    drag: (from, to, { duration = "1500ms" } = {}) => call(["drag", "--from", `${from[0]},${from[1]}`, "--to", `${to[0]},${to[1]}`, "--duration", duration, "--foreground"]),
    /** Scrolls the page under the pointer: what is below the fold is not in the accessibility tree until it is scrolled to. */
    scroll: (direction = "down", amount = 10) => call(["scroll", "--direction", direction, "--amount", String(amount), ...target, "--foreground"]),
    /** The accessibility elements of the app's window as Peekaboo reports them (name, role, whether they can be acted on): for the evidence of a failed run. */
    elements: async () => { const { stdout } = await retrying(() => call(["see", ...target, "--json", "--web-focus"])); return JSON.parse(stdout).data?.ui_elements ?? []; },
    see: (path, { json = false } = {}) => retrying(() => call(["see", ...target, "--path", path, "--format", "png", ...(json ? ["--json"] : [])])),
  });
}
