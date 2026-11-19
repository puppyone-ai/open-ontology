// Operating a packaged app through Peekaboo (volume 25 section 6, T2): every action addresses an element by its accessible name, never by a
// test id. macOS Accessibility and Screen Recording permission must be granted to the host that runs Peekaboo; `permissions()` reports it.
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const execute = promisify(execFile);

// The runner is started from a Terminal window (run-t2.command), and that window is the process the permissions were granted to, so
// Peekaboo runs in-process (`--no-remote`) instead of asking a bridge host that may not hold them. SO_PEEKABOO_FLAGS overrides it.
const DEFAULT_FLAGS = (process.env.SO_PEEKABOO_FLAGS ?? "--no-remote").split(" ").filter(Boolean);

export function createPeekaboo({ app, binary = "peekaboo", flags = DEFAULT_FLAGS, run = (file, args, options) => execute(file, args, { maxBuffer: 16 * 1024 * 1024, ...options }) } = {}) {
  const target = ["--app", app];
  // A failed call says why: Peekaboo's own message (stderr, else stdout) is part of the error.
  const call = (args, options) => run(binary, [...args, ...flags], options).catch((error) => {
    error.message = `${error.message}\n${String(error.stderr || error.stdout || "").trim().slice(0, 600)}`;
    throw error;
  });
  return Object.freeze({
    /** `{ granted, missing, raw }`: Screen Recording and Accessibility are required. */
    async permissions() {
      const { stdout } = await call(["permissions"]).catch((error) => ({ stdout: `${error.stdout ?? ""}${error.stderr ?? ""}` }));
      const missing = [...stdout.matchAll(/^(.+?) \(Required\): Not Granted/gmu)].map((match) => match[1]);
      return { granted: missing.length === 0 && /Required\): Granted/u.test(stdout), missing, raw: stdout };
    },
    click: (name, { right = false, double = false, modifiers } = {}) => call(["click", name, ...target, "--foreground", "--wait-for", "10s", ...(right ? ["--right"] : []), ...(double ? ["--double"] : []), ...(modifiers ? ["--modifiers", modifiers] : [])]),
    /** Types into the focused element: click the field first. */
    type: (text) => call(["type", text, ...target]),
    press: (chord) => call(["press", chord, ...target]),
    see: (path, { json = false } = {}) => call(["see", ...target, "--path", path, "--format", "png", ...(json ? ["--json"] : [])]),
  });
}
