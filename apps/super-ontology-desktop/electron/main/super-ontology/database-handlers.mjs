import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { SuperOntologyError } from "../../../src/shared/super-ontology/errors.mjs";

const FILE_MODE = 0o600;
const DIRECTORY_MODE = 0o700;
const RUN_TIMEOUT_MS = 30_000;
const PLACEHOLDER = "placeholder";
/**
 * `oo source add` registers into an existing stack file, and a stack file needs one source and a model block. The first database
 * therefore goes in beside this placeholder source, which is removed again right after. The model is a reference only; nothing
 * is called with it until the person configures a model (Settings).
 */
const BOOTSTRAP_STACK = {
  version: "oo.stack/v1",
  sources: [{ id: PLACEHOLDER, engine: "postgres", host: "localhost", port: 5432, database: "bootstrap", user: "bootstrap", password: { secret_ref: "env:OO_BOOTSTRAP_UNUSED" }, scope: { include: ["public.*"], exclude: [] } }],
  model: { provider: "openai-compatible", base_url: "https://api.openai.com/v1", model: "unset", key: { secret_ref: "env:OO_MODEL_KEY" } },
};

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);

function defaultRun(file, args, { env, input, timeoutMs = RUN_TIMEOUT_MS } = {}) {
  return new Promise((resolve) => {
    const child = execFile(file, args, { env, timeout: timeoutMs, maxBuffer: 1024 * 1024, windowsHide: true }, (error, stdout, stderr) => {
      resolve({ code: error ? (typeof error.code === "number" ? error.code : 1) : 0, stdout: String(stdout ?? ""), stderr: String(stderr ?? "") });
    });
    if (input !== undefined) child.stdin?.end(input); else child.stdin?.end();
  });
}

/** The `{ error: { code } }` line `oo --json` prints on stderr, or null. */
function cliErrorCode(stderr) {
  for (const line of stderr.split("\n")) {
    try { const parsed = JSON.parse(line); if (isObject(parsed?.error) && typeof parsed.error.code === "string") return parsed.error.code; } catch { /* not JSON */ }
  }
  return null;
}

function failFrom(result) {
  const code = cliErrorCode(result.stderr);
  if (code === "SO_SOURCE_EXISTS") return new SuperOntologyError("SO_DB_SOURCE_EXISTS");
  if (code === "SO_SOURCE_BAD_CREDENTIAL" || (code && ["SO_SOURCE_BAD_STACK", "SO_SOURCE_LAST"].includes(code) === false && code.startsWith("SO_SOURCE_"))) return new SuperOntologyError("SO_DB_SOURCE_INVALID");
  if (result.code === 2) return new SuperOntologyError("SO_DB_SOURCE_INVALID");
  return new SuperOntologyError("SO_DB_SOURCE_FAILED");
}

function publicSource(row) {
  const config = isObject(row.config) ? row.config : {};
  return {
    name: String(row.name), kind: String(row.backend), host: String(config.host ?? ""), port: Number(config.port ?? 0), database: String(config.database ?? ""), user: String(config.user ?? ""),
    include: Array.isArray(config.include) ? config.include.map(String) : [], tls: config.tls_ca === true,
  };
}

/**
 * Database sources (D83): the same `oo source add postgres|mysql` the command line runs, with the password handed over on stdin
 * (never on a command line), kept by the CLI as a 0600 file in the secrets directory, and only a `file:` reference in the stack.
 * The trust anchor is written as a 0600 file next to it. Nothing here returns or logs a credential.
 */
export function createDatabaseHandlers({ lifecycle, directory, secretsDirectory, stateDir, fsPromises = fs.promises, run = defaultRun }) {
  const stackFile = directory ? path.join(directory, "stack.yaml") : null;
  const unavailable = () => new SuperOntologyError("SO_DB_SOURCE_FAILED");

  async function context() {
    const launch = lifecycle.launchInfo?.();
    if (!launch?.ooPath) throw new SuperOntologyError("SO_OO_MISSING");
    if (!stackFile || !secretsDirectory) throw unavailable();
    await fsPromises.mkdir(directory, { recursive: true, mode: DIRECTORY_MODE });
    await fsPromises.mkdir(secretsDirectory, { recursive: true, mode: DIRECTORY_MODE });
    const env = { ...(launch.environment ?? process.env), OO_DOCS_SECRETS_DIR: secretsDirectory };
    return { oo: launch.ooPath, env };
  }

  const cli = (ctx, args, input) => run(ctx.oo, [...args, "--config", stackFile, "--json", "--non-interactive", ...(stateDir ? ["--state-dir", stateDir] : [])], { env: ctx.env, input });

  async function writeNew(file, text) {
    const handle = await fsPromises.open(file, fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL | fs.constants.O_NOFOLLOW, FILE_MODE);
    try { await handle.writeFile(text, "utf8"); await handle.sync(); } finally { await handle.close(); }
  }

  async function list(ctx) {
    if (!(await fsPromises.stat(stackFile).catch(() => null))) return [];
    const result = await cli(ctx, ["source", "ls"]);
    if (result.code !== 0) throw unavailable();
    let parsed;
    try { parsed = JSON.parse(result.stdout.trim().split("\n").filter(Boolean).pop() ?? "{}"); } catch { throw unavailable(); }
    return (Array.isArray(parsed.sources) ? parsed.sources : []).filter((row) => isObject(row) && row.kind === "database" && row.name !== PLACEHOLDER).map(publicSource);
  }

  return {
    "db-sources-list": async () => ({ items: await list(await context()) }),

    "db-source-add": async ({ kind, name, host, port, database, user, password, caPem, include }) => {
      const ctx = await context();
      const caFile = path.join(secretsDirectory, `db-${name}-ca.pem`);
      const createdStack = !(await fsPromises.stat(stackFile).catch(() => null));
      try {
        if (createdStack) await writeNew(stackFile, `${JSON.stringify(BOOTSTRAP_STACK)}\n`);
        await writeNew(caFile, caPem.endsWith("\n") ? caPem : `${caPem}\n`).catch((error) => { throw error?.code === "EEXIST" ? new SuperOntologyError("SO_DB_SOURCE_EXISTS") : unavailable(); });
      } catch (error) {
        if (createdStack) await fsPromises.rm(stackFile, { force: true }).catch(() => undefined);
        throw error instanceof SuperOntologyError ? error : unavailable();
      }
      const args = ["source", "add", kind, "--name", name, "--host", host, "--port", String(port), "--database", database, "--user", user, "--tls-ca-file", caFile, "--password-stdin", ...include.flatMap((entry) => ["--include", entry])];
      const result = await cli(ctx, args, password);
      if (result.code !== 0) {
        await fsPromises.rm(caFile, { force: true }).catch(() => undefined);
        if (createdStack) await fsPromises.rm(stackFile, { force: true }).catch(() => undefined);
        throw failFrom(result);
      }
      if (createdStack) await cli(ctx, ["source", "rm", PLACEHOLDER, "--keep-secret"]);
      const added = (await list(ctx)).find((entry) => entry.name === name);
      if (!added) throw unavailable();
      return added;
    },

    "db-source-test": async ({ name }) => {
      const ctx = await context();
      const result = await cli(ctx, ["source", "test", name]);
      if (result.code !== 0 && cliErrorCode(result.stderr) !== "SO_SOURCE_TEST_FAILED") throw unavailable();
      return { reachable: result.code === 0 };
    },

    "db-source-remove": async ({ name }) => {
      const ctx = await context();
      // A stack file always keeps one source: the last real one goes out with the placeholder back in its place.
      const remaining = await list(ctx);
      if (remaining.length === 1 && remaining[0].name === name) {
        try {
          const stack = JSON.parse(await fsPromises.readFile(stackFile, "utf8"));
          if (!stack.sources.some((source) => source.id === PLACEHOLDER)) {
            stack.sources.push(BOOTSTRAP_STACK.sources[0]);
            const temporary = `${stackFile}.${process.pid}.tmp`;
            await writeNew(temporary, `${JSON.stringify(stack)}\n`);
            await fsPromises.rename(temporary, stackFile);
          }
        } catch { throw unavailable(); }
      }
      const result = await cli(ctx, ["source", "rm", name]);
      if (result.code !== 0) throw failFrom(result);
      await fsPromises.rm(path.join(secretsDirectory, `db-${name}-ca.pem`), { force: true }).catch(() => undefined);
      return { removed: true };
    },
  };
}
