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
  // The CLI's class-style errors carry `CODE` or `CODE: text` in `message`.
  const coded = typeof error?.message === "string" ? /^([A-Z][A-Z0-9_]{2,63})(?::\s*(.*))?$/su.exec(error.message) : null;
  const code = typeof error?.code === "string" ? error.code : coded ? coded[1] : "SO_CLI_FAILED";
  const plain = redact(result.stderr).split(/\n/u).find((line) => line.trim() && !line.trim().startsWith("{")) ?? `oo exited with ${result.code}`;
  const message = redact(coded ? (coded[2] ?? "") : typeof error?.message === "string" && error.message !== code ? error.message : plain);
  return { code, message: message.slice(0, 600), hint: typeof doc?.hint === "string" ? redact(doc.hint).slice(0, 600) : null };
}

const STAGE_NAMES = ["load", "candidate", "builder", "self_review", "publish", "gateway", "first_token", "share_prompt"];

/**
 * The CLI's `oo.build-chain/v1` document (what `oo build --json` prints and the stage record it keeps) reduced to what a screen shows:
 * the stages in order with their counters, and whether the chain is waiting for the owner's review. Never carries a path or a secret.
 */
export function summarizeBuild(document) {
  if (!document || typeof document !== "object" || !Array.isArray(document.stages)) return null;
  const stages = document.stages.filter((stage) => stage && typeof stage.name === "string").map((stage) => ({
    name: stage.name, status: typeof stage.status === "string" ? stage.status : "pending", completed: Number.isFinite(stage.completed) ? stage.completed : 0, total: Number.isFinite(stage.total) ? stage.total : 1,
    attempts: Number.isFinite(stage.attempts) ? stage.attempts : 0, error: typeof stage.error === "string" && stage.error !== "" ? redact(stage.error).slice(0, 700) : null,
  })).sort((a, b) => STAGE_NAMES.indexOf(a.name) - STAGE_NAMES.indexOf(b.name));
  const waiting = stages.find((stage) => stage.name === "self_review");
  const review = waiting?.status === "blocked" && /^REVIEW_FILE_REQUIRED/u.test(waiting.error ?? "");
  return {
    status: typeof document.status === "string" ? document.status : "unknown", publication: typeof document.publication === "string" ? document.publication : null,
    gateway: typeof document.gateway_health === "string" ? document.gateway_health : null, profile: typeof document.profile === "string" && /^[A-Za-z0-9_-]{1,64}$/u.test(document.profile) ? document.profile : null,
    modelCalls: Number.isFinite(document.model_calls) ? document.model_calls : null, review, stages,
  };
}

/** A blocked stage's `CODE: cause` text (the CLI names the cause, e.g. a certificate SAN or a missing grant) as the failure of a build. */
export function buildFailureOf(build, result) {
  const stage = build.stages.find((entry) => entry.error && !/^REVIEW_FILE_REQUIRED/u.test(entry.error));
  if (!stage) return failureOf(result);
  const coded = /^([A-Z][A-Z0-9_]{2,63})(?::\s*([\s\S]*))?$/u.exec(stage.error);
  return { code: coded ? coded[1] : "SO_CLI_FAILED", message: redact(coded ? (coded[2] ?? "") : stage.error).slice(0, 600), hint: null, stage: stage.name };
}
