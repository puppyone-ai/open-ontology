import { randomUUID, randomBytes } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { SuperOntologyError } from "../../../src/shared/super-ontology/errors.mjs";
import { DB_KINDS, dbKind } from "../../../src/shared/super-ontology/db-kinds.mjs";
import { displayCommand, failureOf, parseJson, redact, runOo } from "./cli-runner.mjs";

const slug = (name) => name.replace(/[^A-Za-z0-9]+/gu, "_").slice(0, 48);
const MAX_JOB_LINES = 600;
const FIRST_SOURCE_WITHOUT_MODEL = "SO_MODEL_REQUIRED";

/**
 * Handlers of the CLI parity channels (cli-requests.mjs). Everything goes through the `oo` executable the app already locates
 * (`oo source add`, `oo doctor`, `oo query` ...); there is no second implementation of the pipeline or the Gateway client here.
 * A result is `{ ok, exit, data, error, cli }`: a failing command is a normal answer carrying `error.code`, a message and the CLI's
 * hint, so the screen can say what to do next; only a missing executable or a bad request throws.
 */
export function createCliHandlers({
  locator, getUserExecutable = () => null, client = null, grants, stateDir, fsPromises = fs.promises, run = runOo,
  environment = process.env, homedir = os.homedir, platform = process.platform, appVersion = null,
}) {
  const dataDir = stateDir ? path.dirname(stateDir) : null;
  const home = () => environment.SO_CLI_HOME ?? homedir();
  const secretsDir = () => path.join(home(), ".config", "open-ontology", "secrets");
  const stackFile = () => (dataDir ? path.join(dataDir, "stack.yaml") : null);
  const gatewayFile = () => (dataDir ? path.join(dataDir, "gateway.json") : null);
  let located = null;

  async function executable() {
    if (environment.SO_CLI_OO) return { path: environment.SO_CLI_OO, env: { ...process.env, ...(environment.SO_CLI_HOME ? { HOME: environment.SO_CLI_HOME } : {}) } };
    if (!located || Date.now() - located.at > 60_000) {
      const found = await locator.locate({ userExecutable: getUserExecutable() });
      located = { at: Date.now(), found };
    }
    if (!located.found.oo) throw new SuperOntologyError("SO_OO_MISSING");
    return { path: located.found.oo.path, env: { ...located.found.environment, ...(environment.SO_CLI_HOME ? { HOME: environment.SO_CLI_HOME } : {}) } };
  }

  /** One `oo` call -> the common answer. `json: false` keeps stdout as text. */
  async function call(args, { stdin = null, timeoutMs, json = true, accept = [0], cliArgs = args } = {}) {
    const exe = await executable();
    let result;
    try { result = await run(exe.path, args, { env: exe.env, stdin, ...(timeoutMs ? { timeoutMs } : {}) }); } catch { throw new SuperOntologyError("SO_OO_INVALID"); }
    const cli = displayCommand(cliArgs);
    const okExit = accept.includes(result.code) && !result.timedOut && !result.truncated;
    const data = json ? parseJson(result.stdout) : result.stdout;
    if (okExit && (!json || data !== null)) return { ok: true, exit: result.code, data: redactDeep(data), error: null, cli };
    return { ok: false, exit: result.code, data: json && data && result.code !== 0 ? null : redactDeep(data), error: failureOf(result), cli };
  }

  const redactDeep = (value) => (typeof value === "string" ? redact(value) : Array.isArray(value) ? value.map(redactDeep) : value && typeof value === "object" ? Object.fromEntries(Object.entries(value).map(([k, v]) => [k, redactDeep(v)])) : value);

  async function ensureSecretsDir() {
    const dir = secretsDir();
    await fsPromises.mkdir(path.dirname(dir), { recursive: true, mode: 0o700 });
    await fsPromises.mkdir(dir, { recursive: true, mode: 0o700 });
    await fsPromises.chmod(dir, 0o700);
    return dir;
  }

  async function writePrivate(file, text) {
    const handle = await fsPromises.open(file, fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL | fs.constants.O_NOFOLLOW, 0o600);
    try { await handle.writeFile(text, "utf8"); await handle.sync(); } finally { await handle.close(); }
  }

  async function stackExists() {
    const file = stackFile();
    if (!file) return false;
    try { return (await fsPromises.lstat(file)).isFile(); } catch { return false; }
  }

  function requireDataDir() { if (!dataDir) throw new SuperOntologyError("SO_SERVE_STATE_DIR_INVALID"); }

  /**
   * The CA given as a granted file or pasted PEM -> a private (0600) copy in the secrets folder that `oo` can read: `oo` refuses
   * credential files that other users can read, and a certificate someone picked is usually world-readable. `temporary` copies are
   * removed by the caller when the work is done without saving anything.
   */
  async function resolveCa({ tlsCaGrantId, tlsCaPem }, sender, label) {
    if (tlsCaGrantId && tlsCaPem) throw new SuperOntologyError("SO_INVALID_REQUEST");
    let pem = null;
    if (tlsCaGrantId) {
      const source = grants.resolve(sender, tlsCaGrantId, "ca-file");
      try { pem = await fsPromises.readFile(source, "utf8"); } catch { throw new SuperOntologyError("SO_INVALID_REQUEST"); }
      if (pem.length > 65536) throw new SuperOntologyError("SO_INVALID_REQUEST");
    } else if (tlsCaPem) pem = tlsCaPem;
    if (pem === null) return { file: null };
    if (!/-----BEGIN CERTIFICATE-----[\s\S]+-----END CERTIFICATE-----/u.test(pem)) throw new SuperOntologyError("SO_INVALID_REQUEST");
    const dir = await ensureSecretsDir();
    const file = path.join(dir, `ca_${slug(label)}_${randomBytes(4).toString("hex")}.pem`);
    await writePrivate(file, pem.endsWith("\n") ? pem : `${pem}\n`);
    return { file };
  }

  const kindOf = (id) => { const kind = dbKind(id); if (!kind || !kind.wired) throw new SuperOntologyError("SO_INVALID_REQUEST"); return kind; };
  const portOf = (kind, port) => port ?? kind.port;

  async function modelForStack() {
    if (!client) return null;
    try {
      const { value } = await client.request("modelConfigGet", {});
      if (!value?.configured || typeof value.key_ref !== "string" || typeof value.base_url !== "string") return null;
      return { provider: value.provider, base_url: value.base_url, model: value.model, key: { secret_ref: value.key_ref } };
    } catch { return null; }
  }

  async function gatewayConfig() {
    const file = gatewayFile();
    if (!file) return null;
    try { const doc = JSON.parse(await fsPromises.readFile(file, "utf8")); return typeof doc?.url === "string" && typeof doc?.tokenFile === "string" ? doc : null; } catch { return null; }
  }

  const connArgs = (gateway) => ["--json", "--gateway", gateway.url, "--transport", gateway.transport ?? "production", "--token-file", gateway.tokenFile, ...(gateway.caFile ? ["--ca-file", gateway.caFile] : [])];
  const notConnected = (cli) => ({ ok: false, exit: null, data: null, error: { code: "SO_GATEWAY_NOT_CONNECTED", message: "No Gateway is connected.", hint: null }, cli });

  const jobs = new Map();
  let activeJob = null;

  return {
    "cli-db-kinds": async () => ({ kinds: DB_KINDS, stackFile: stackFile(), stackExists: await stackExists(), secretsDir: secretsDir() }),

    "cli-db-list": async () => {
      requireDataDir();
      if (!(await stackExists())) return { ok: true, exit: 0, data: { sources: [], stackExists: false }, error: null, cli: displayCommand(["source", "ls", "--config", stackFile(), "--json"]) };
      const result = await call(["source", "ls", "--config", stackFile(), "--json"]);
      if (result.ok) result.data = { stackExists: true, sources: (result.data?.sources ?? []).filter((source) => source.kind === "database") };
      return result;
    },

    /** Reads the database with `oo init <fields>` (resolve, port, TLS, sign-in); writes nothing. */
    "cli-db-check": async (request, { sender } = {}) => {
      const kind = kindOf(request.kind);
      const label = `check${randomBytes(3).toString("hex")}`;
      const dir = await ensureSecretsDir();
      const pwFile = path.join(dir, `chk_${label}_pw`);
      let ca = { file: null };
      try {
        if (!kind.preflight) return { ok: false, exit: null, data: null, error: { code: "SO_CHECK_AFTER_ADD", message: "This kind is checked after it is added.", hint: null }, cli: "" };
        await writePrivate(pwFile, request.password);
        ca = await resolveCa(request, sender, label);
        const args = ["init", "--engine", kind.id, "--host", request.host, "--port", String(portOf(kind, request.port)), kind.nameField === "service" ? "--service" : "--database", request.database, "--user", request.user, "--password-file", pwFile, ...(ca.file ? ["--ca-file", ca.file] : []), "--json"];
        const display = ["init", "--engine", kind.id, "--host", request.host, "--port", String(portOf(kind, request.port)), kind.nameField === "service" ? "--service" : "--database", request.database, "--user", request.user, "--password-file", "<password file>", ...(ca.file ? ["--ca-file", ca.file] : []), "--json"];
        const result = await call(args, { timeoutMs: 45_000, cliArgs: display, accept: [0, 1] });
        if (result.ok && result.data?.signed_in === false) {
          const blocked = (result.data.checks ?? []).find((c) => c.status === "blocked");
          result.ok = false;
          result.error = { code: blocked && blocked.name !== "sign_in" ? "SO_CHECK_BLOCKED" : "SO_CHECK_NOT_SIGNED_IN", message: blocked ? `${blocked.name}: ${blocked.detail}` : "The database was reached but did not accept the sign-in.", hint: null };
        }
        return result;
      } finally {
        await fsPromises.rm(pwFile, { force: true }).catch(() => undefined);
        if (ca.file) await fsPromises.rm(ca.file, { force: true }).catch(() => undefined);
      }
    },

    "cli-db-add": async (request, { sender } = {}) => {
      requireDataDir();
      const kind = kindOf(request.kind);
      if (kind.tls === "required" && !request.tlsCaGrantId && !request.tlsCaPem) return { ok: false, exit: null, data: null, error: { code: "SO_TLS_CA_REQUIRED", message: "A certificate authority is required.", hint: null }, cli: "" };
      const dir = await ensureSecretsDir();
      const stack = stackFile();
      const ca = await resolveCa(request, sender, request.name);
      const port = portOf(kind, request.port);
      const include = request.include.flatMap((entry) => ["--include", entry]);
      const exclude = (request.exclude ?? []).flatMap((entry) => ["--exclude", entry]);
      const common = ["--name", request.name, "--host", request.host, "--port", String(port), "--database", request.database, "--user", request.user];
      const cleanup = async (files) => { await Promise.all(files.map((file) => fsPromises.rm(file, { force: true }).catch(() => undefined))); };
      if (await stackExists()) {
        const args = ["source", "add", kind.id, ...common, "--config", stack, ...(ca.file ? ["--tls-ca-file", ca.file] : []), ...include, ...exclude, "--password-stdin", "--json"];
        const result = await call(args, { stdin: `${request.password}\n`, timeoutMs: 30_000 });
        if (!result.ok && ca.file) await cleanup([ca.file]);
        return result;
      }
      // The first source creates the stack file the pipeline reads (the model comes from the engine's model settings).
      const model = await modelForStack();
      if (!model) { if (ca.file) await cleanup([ca.file]); return { ok: false, exit: null, data: null, error: { code: FIRST_SOURCE_WITHOUT_MODEL, message: "A model is needed before the first database.", hint: null }, cli: "" }; }
      const pwFile = path.join(dir, `src_${slug(request.name)}_pw`);
      try { await writePrivate(pwFile, request.password); } catch {
        if (ca.file) await cleanup([ca.file]);
        return { ok: false, exit: null, data: null, error: { code: "SO_SOURCE_SECRET_EXISTS", message: "A credential file for this name already exists.", hint: null }, cli: "" };
      }
      const doc = {
        version: "oo.stack/v1",
        sources: [{ id: request.name, engine: kind.id, host: request.host, port, database: request.database, user: request.user, password: { secret_ref: `file:${pwFile}` }, ...(ca.file ? { tls_ca: { secret_ref: `file:${ca.file}` } } : {}), scope: { include: request.include, exclude: request.exclude ?? [] } }],
        model,
      };
      const temp = `${stack}.${process.pid}.tmp`;
      try {
        await writePrivate(temp, `${JSON.stringify(doc, null, 2)}\n`);
        await fsPromises.rename(temp, stack);
      } catch {
        await cleanup([temp, pwFile, ...(ca.file ? [ca.file] : [])]);
        throw new SuperOntologyError("SO_SECRET_STORE_UNAVAILABLE");
      }
      const result = await call(["init", "--config", stack, "--json"], { timeoutMs: 30_000 });
      if (!result.ok) { await cleanup([stack, pwFile, ...(ca.file ? [ca.file] : [])]); }
      else result.data = { ...result.data, name: request.name, created_stack: true };
      return result;
    },

    "cli-db-remove": async ({ name }) => { requireDataDir(); return call(["source", "rm", name, "--config", stackFile(), "--json"]); },
    "cli-db-test": async ({ name }) => { requireDataDir(); return call(["source", "test", name, "--config", stackFile(), "--json"], { timeoutMs: 20_000 }); },
    "cli-doctor": async () => { requireDataDir(); return call(["doctor", "--config", stackFile(), "--json"], { timeoutMs: 180_000, accept: [0, 1] }); },

    "cli-pipeline": async ({ action }) => {
      requireDataDir();
      if (action === "status") return call(["status", "--config", stackFile(), "--json"], { timeoutMs: 30_000 });
      const gateway = await gatewayConfig();
      const args = ["freshness", "--summary"];
      return gateway ? call([...args, ...connArgs(gateway)], { timeoutMs: 60_000 }) : notConnected(displayCommand([...args, "--json"]));
    },

    "cli-job-start": async ({ action }) => {
      requireDataDir();
      if (activeJob && jobs.get(activeJob)?.state === "running") throw new SuperOntologyError("SO_UPLOAD_CONFLICT");
      const exe = await executable();
      const args = [action, "--config", stackFile(), "--yes", "--json"];
      const id = randomUUID();
      const job = { id, action, state: "running", exit: null, lines: [], cli: displayCommand(args), startedAt: Date.now() };
      jobs.set(id, job); activeJob = id;
      for (const [key, old] of jobs) if (jobs.size > 8 && old.state !== "running") jobs.delete(key);
      const onLine = (line) => {
        job.lines.push(line);
        if (job.lines.length > MAX_JOB_LINES) { const extra = job.lines.length - MAX_JOB_LINES; job.lines.splice(0, extra); job.dropped = (job.dropped ?? 0) + extra; }
      };
      void run(exe.path, args, { env: exe.env, timeoutMs: 30 * 60_000, onLine, maxBytes: 64 * 1024 * 1024 }).then((result) => {
        job.exit = result.code; job.state = result.code === 0 && !result.timedOut ? "succeeded" : "failed";
        if (job.state === "failed") job.error = failureOf(result);
      }, () => { job.state = "failed"; job.error = { code: "SO_OO_INVALID", message: "oo could not be started.", hint: null }; });
      return { jobId: id, cli: job.cli };
    },

    "cli-job-read": async ({ jobId, since = 0 }) => {
      const job = jobs.get(jobId);
      if (!job) throw new SuperOntologyError("SO_INVALID_REQUEST");
      const base = job.dropped ?? 0;
      return { jobId, action: job.action, state: job.state, exit: job.exit, error: job.error ?? null, cli: job.cli, startedAt: job.startedAt, offset: Math.max(since, base), next: base + job.lines.length, lines: job.lines.slice(Math.max(0, since - base)) };
    },

    "cli-gateway-read": async () => {
      const gateway = await gatewayConfig();
      return gateway ? { configured: true, url: gateway.url, transport: gateway.transport ?? "production", tokenFile: gateway.tokenFile, caFile: gateway.caFile ?? null } : { configured: false, url: null, transport: "production", tokenFile: null, caFile: null };
    },

    "cli-gateway-set": async ({ url, tokenGrantId, caGrantId, transport = "production" }, { sender } = {}) => {
      requireDataDir();
      let parsed;
      try { parsed = new URL(url); } catch { throw new SuperOntologyError("SO_INVALID_REQUEST"); }
      if (!["http:", "https:"].includes(parsed.protocol) || parsed.username || parsed.password || parsed.search || parsed.hash || (parsed.pathname !== "/" && parsed.pathname !== "")) throw new SuperOntologyError("SO_INVALID_REQUEST");
      const tokenFile = grants.resolve(sender, tokenGrantId, "token-file");
      const caFile = caGrantId ? grants.resolve(sender, caGrantId, "ca-file") : null;
      const doc = { url: parsed.origin, transport, tokenFile, ...(caFile ? { caFile } : {}) };
      const file = gatewayFile();
      const temp = `${file}.${process.pid}.tmp`;
      await fsPromises.rm(temp, { force: true });
      await writePrivate(temp, `${JSON.stringify(doc)}\n`);
      await fsPromises.rename(temp, file);
      return { configured: true, url: doc.url, transport, tokenFile, caFile };
    },

    "cli-gateway-clear": async () => { const file = gatewayFile(); if (file) await fsPromises.rm(file, { force: true }); return { configured: false }; },

    "cli-query": async (request) => {
      const gateway = await gatewayConfig();
      const shown = (parts) => displayCommand([...parts, "--json", "--gateway", gateway?.url ?? "<gateway url>", "--token-file", gateway?.tokenFile ?? "<token file>"]);
      const need = (name) => { if (request[name] === undefined) throw new SuperOntologyError("SO_INVALID_REQUEST"); return request[name]; };
      let head; let tail = [];
      switch (request.op) {
        case "query": head = ["query", ...(request.maxRows ? ["--max-rows", String(request.maxRows)] : []), ...(request.timeoutMs ? ["--timeout-ms", String(request.timeoutMs)] : [])]; tail = ["--", need("sql")]; break;
        case "validate": head = ["validate"]; tail = ["--", need("sql")]; break;
        case "tables": head = ["tables"]; break;
        case "describe": head = ["describe"]; tail = ["--", need("table")]; break;
        case "freshness": head = ["freshness", "--summary"]; break;
        case "knowledge-search": head = ["knowledge", "search", ...(request.pathPrefix ? ["--path-prefix", request.pathPrefix] : []), ...(request.offset ? ["--offset", String(request.offset)] : []), ...(request.limit ? ["--limit", String(request.limit)] : [])]; tail = ["--", need("text")]; break;
        default: head = ["knowledge", "cat"]; tail = ["--", need("path")];
      }
      if (!gateway) return notConnected(`${shown(head)} ${tail.map((t) => displayCommand([t]).slice(3)).join(" ")}`);
      const result = await call([...head, ...connArgs(gateway), ...tail], { timeoutMs: (request.timeoutMs ?? 60_000) + 10_000 });
      return result;
    },

    "cli-diagnostics": async () => {
      const exe = await executable().catch(() => null);
      const base = { platform: { os: platform, arch: process.arch, node: process.versions.node, app: appVersion }, paths: { stackFile: stackFile(), stackExists: await stackExists(), secretsDir: secretsDir(), dataDir }, oo: exe ? { path: exe.path } : null };
      if (!exe) return { ...base, version: null, check: null, runtime: null, serve: null };
      const [version, check, runtime, serve] = await Promise.all([
        call(["--version", "--json"], { timeoutMs: 10_000 }),
        call(["check", "--json"], { timeoutMs: 30_000, accept: [0, 1] }),
        call(["docs", "runtime", "status", "--json"], { timeoutMs: 20_000, accept: [0, 1] }),
        call(["serve", "--status", "--json"], { timeoutMs: 20_000, accept: [0, 1] }),
      ]);
      return { ...base, version, check, runtime, serve };
    },

    "cli-ontology-export": async ({ grantId, files }, { sender } = {}) => {
      const target = grants.resolve(sender, grantId, "export-dir");
      const stamp = new Date().toISOString().replaceAll(":", "-").slice(0, 19);
      const root = path.join(target, `ontology-yaml-${stamp}`);
      await fsPromises.mkdir(root, { recursive: true });
      for (const file of files) {
        const full = path.resolve(root, file.name);
        if (!full.startsWith(`${root}${path.sep}`)) throw new SuperOntologyError("SO_INVALID_REQUEST");
        await fsPromises.mkdir(path.dirname(full), { recursive: true });
        await fsPromises.writeFile(full, file.text, { encoding: "utf8", flag: "wx", mode: 0o644 });
      }
      return { directory: root, count: files.length, cli: displayCommand(["ontology", "export", "--format", "yaml", "--pack", "<pack.json>", "--out", `${root}/ontology.yaml`]) };
    },
  };
}
