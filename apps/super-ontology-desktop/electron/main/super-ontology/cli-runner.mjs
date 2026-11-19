import { spawn as nodeSpawn } from "node:child_process";
import { StringDecoder } from "node:string_decoder";

const SECRET_LOOKING = /\b(?:ood[tri]_|ooa_|oodc_|sk-)[A-Za-z0-9_-]{6,}/gu;
export const redact = (text) => String(text ?? "").replace(SECRET_LOOKING, "[hidden]");

/** The command a person could paste into a terminal. Arguments are quoted only when needed; passwords never appear (they travel on stdin). */
export function shellQuote(value) {
  const text = String(value);
  return /^[A-Za-z0-9_./:@%+=,-]+$/u.test(text) ? text : `'${text.replaceAll("'", "'\\''")}'`;
}
export const displayCommand = (args) => ["oo", ...args.map(shellQuote)].join(" ");

/**
 * Runs `oo` with an argument array (never a shell), a bounded output and a deadline. Resolves with
 * `{ code, stdout, stderr, timedOut, truncated }`; rejects only when the process cannot be started.
 */
export function runOo(executable, args, { env, stdin = null, timeoutMs = 60_000, maxBytes = 4 * 1024 * 1024, spawn = nodeSpawn, onLine = null, cwd } = {}) {
  return new Promise((resolve, reject) => {
    let child;
    try { child = spawn(executable, args, { env, cwd, stdio: ["pipe", "pipe", "pipe"], windowsHide: true }); } catch (error) { reject(error); return; }
    const out = new StringDecoder("utf8");
    const err = new StringDecoder("utf8");
    let stdout = ""; let stderr = ""; let bytes = 0; let truncated = false; let timedOut = false; let settled = false;
    const carry = { out: "", err: "" };
    const lines = (which, chunk) => {
      if (!onLine) return;
      carry[which] += chunk;
      const parts = carry[which].split(/\r?\n/u);
      carry[which] = parts.pop() ?? "";
      for (const line of parts) if (line.length > 0) onLine(redact(line.slice(0, 2000)), which);
    };
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, timeoutMs);
    const add = (which) => (data) => {
      bytes += data.length;
      if (bytes > maxBytes) { truncated = true; child.kill("SIGKILL"); return; }
      const text = (which === "out" ? out : err).write(data);
      if (which === "out") stdout += text; else stderr += text;
      lines(which, text);
    };
    child.stdout.on("data", add("out"));
    child.stderr.on("data", add("err"));
    child.stdin.on("error", () => undefined);
    if (stdin !== null) child.stdin.end(stdin); else child.stdin.end();
    child.on("error", (error) => { if (settled) return; settled = true; clearTimeout(timer); reject(error); });
    child.on("close", (code) => {
      if (settled) return;
      settled = true; clearTimeout(timer);
      if (onLine) { for (const which of ["out", "err"]) if (carry[which]) onLine(redact(carry[which].slice(0, 2000)), which); }
      resolve({ code: code ?? -1, stdout, stderr, timedOut, truncated });
    });
  });
}

/** The last JSON document in the output (the CLI prints one per command, errors included), or null. */
export function parseJson(text) {
  const trimmed = String(text ?? "").trim();
  if (!trimmed) return null;
  try { return JSON.parse(trimmed); } catch { /* fall through to the last line */ }
  const last = trimmed.split(/\n/u).filter(Boolean).at(-1);
  try { return JSON.parse(last); } catch { return null; }
}

const jsonLines = (text) => String(text ?? "").split(/\n/u).map((line) => { try { return JSON.parse(line); } catch { return null; } }).filter((doc) => doc && typeof doc === "object");

/** Turns a failed run into `{ code, message, hint }` without any secret. */
export function failureOf(result) {
  if (result.timedOut) return { code: "SO_TIMEOUT", message: "The command did not finish in time.", hint: null };
  if (result.truncated) return { code: "SO_OUTPUT_TOO_LARGE", message: "The command printed too much.", hint: null };
  const docs = [...jsonLines(result.stdout), ...jsonLines(result.stderr), ...[parseJson(result.stdout)].filter(Boolean)];
  const doc = docs.find((d) => d.error && typeof d.error === "object") ?? docs.find((d) => typeof d.message === "string") ?? null;
  const error = doc?.error && typeof doc.error === "object" ? doc.error : doc;
  const code = typeof error?.code === "string" ? error.code : typeof error?.message === "string" && /^[A-Z0-9_]{3,64}$/u.test(error.message) ? error.message : "SO_CLI_FAILED";
  const plain = redact(result.stderr).split(/\n/u).find((line) => line.trim() && !line.trim().startsWith("{")) ?? `oo exited with ${result.code}`;
  const message = redact(typeof error?.message === "string" && error.message !== code ? error.message : plain);
  return { code, message: message.slice(0, 600), hint: typeof doc?.hint === "string" ? redact(doc.hint).slice(0, 600) : null };
}
