import path from "node:path";
import { SuperOntologyError } from "../../../../src/shared/super-ontology/errors.mjs";
import { runAskLoop } from "./ask-loop.mjs";
import { createAskTools, createSourceRegistry } from "./ask-tools.mjs";
import { createModelClient, ModelError, readKeyRef } from "./model-client.mjs";

const TOKEN_NAME = "super-ontology-app";
const TOKEN_FILE = "ask-agent.json";
const TOKEN_TTL_SECONDS = 2_592_000;
const RENEW_BEFORE_MS = 24 * 3600 * 1000;
const TOOL_TIMEOUT_MS = 25_000;
const ASK_DEADLINE_MS = 240_000;
const BODY_LIMIT = 4 * 1024 * 1024;
const ENGINE_CODES = new Set(["SO_ENGINE_NOT_RUNNING", "SO_ENGINE_UNREACHABLE", "SO_TIMEOUT", "SO_SERVE_CREDENTIALS_INVALID"]);

/**
 * Ask (the Ontology tab): a bounded tool-calling loop in this process against the model configured in Settings.
 * The tools read through an internal read-only Agent token the app creates and keeps in a 0600 file next to the other secrets;
 * the model key is read here from the engine's `file:` reference. The renderer gets steps, the answer and source references:
 * never a token, a key or an endpoint.
 */
export function createAskHandlers({
  client, credentials, secretsDirectory, fsPromises, listDatabases = async () => [], broadcast = () => {},
  fetchImpl = globalThis.fetch, model = createModelClient({ fetchImpl }), now = () => Date.now(), readKey = readKeyRef,
}) {
  const running = new Map();
  let tokenCache = null;
  let engineFailed = false;

  async function writePrivate(file, text) {
    await fsPromises.mkdir(path.dirname(file), { recursive: true, mode: 0o700 });
    await fsPromises.writeFile(file, text, { mode: 0o600 });
    await fsPromises.chmod(file, 0o600);
  }

  async function readStored(file) {
    try {
      const stat = await fsPromises.lstat(file);
      if (!stat.isFile() || (process.platform !== "win32" && (stat.mode & 0o077) !== 0)) return null;
      const doc = JSON.parse(await fsPromises.readFile(file, "utf8"));
      return typeof doc?.token === "string" && doc.token.startsWith("ooa_") ? doc : null;
    } catch { return null; }
  }

  /** The app's own read-only Agent token: reused while it has a day left and the database profile is unchanged, else renewed. */
  async function databaseProfile() {
    const databases = await Promise.race([listDatabases().catch(() => []), new Promise((resolve) => { setTimeout(() => resolve([]), 8000).unref?.(); })]);
    return databases.find((name) => /^[A-Za-z0-9._-]{1,64}$/u.test(name)) ?? null;
  }

  async function ensureToken({ force = false, profile = null } = {}) {
    if (!secretsDirectory) throw new SuperOntologyError("SO_SECRET_STORE_UNAVAILABLE");
    const file = path.join(secretsDirectory, TOKEN_FILE);
    const credential = credentials();
    const instance = credential?.baseUrl ?? "";
    if (!force) {
      const stored = tokenCache ?? await readStored(file);
      if (stored && stored.gwProfile === profile && stored.instance === instance && Date.parse(stored.expiresAt) - now() > RENEW_BEFORE_MS) { tokenCache = stored; return stored; }
    }
    // One live token per name: retire the old one (this app's, from a previous start) first.
    try {
      const { value } = await client.request("agentTokenList", { query: { state: "live" } });
      for (const item of value?.items ?? []) if (item.name === TOKEN_NAME) await client.request("agentTokenRevoke", { params: { id: item.id } }).catch(() => {});
    } catch { /* a missing list is not fatal: the create below says what is wrong */ }
    const body = (withDb) => ({ name: TOKEN_NAME, scopes: withDb ? ["docs:read", "db:read"] : ["docs:read"], all_drives: true, ...(withDb ? { gw_profile: profile } : {}), label: "Super Ontology app, Ask", expires_in_seconds: TOKEN_TTL_SECONDS });
    let created;
    try { created = (await client.request("agentTokenCreate", { body: body(profile !== null) })).value; } catch (error) {
      if (profile === null) throw error;
      created = (await client.request("agentTokenCreate", { body: body(false) })).value;
    }
    if (typeof created?.token !== "string" || !created.token.startsWith("ooa_")) throw new SuperOntologyError("SO_RESPONSE_INVALID");
    const stored = { token: created.token, id: created.id, gwProfile: created.gw_profile ?? null, expiresAt: created.expires_at, instance };
    try { await writePrivate(file, `${JSON.stringify(stored)}\n`); } catch { throw new SuperOntologyError("SO_SECRET_STORE_UNAVAILABLE"); }
    tokenCache = stored;
    return stored;
  }

  /** One HTTP call to the engine surface with the Agent token; a 401 renews the token once. */
  function makeHttp(profile) {
    return async function http(method, route, body, signal) {
      for (let attempt = 0; attempt < 2; attempt += 1) {
        const live = credentials();
        if (!live) throw new SuperOntologyError("SO_ENGINE_NOT_RUNNING", { retryable: true });
        const token = await ensureToken({ force: attempt === 1, profile });
        const limit = AbortSignal.timeout(TOOL_TIMEOUT_MS);
        let response;
        try {
          response = await fetchImpl(`${live.baseUrl}${route}`, {
            method, redirect: "error", signal: signal ? AbortSignal.any([signal, limit]) : limit,
            headers: { Authorization: `Bearer ${token.token}`, Accept: "application/json", "OO-Docs-Api-Min": "1.0", ...(body === undefined ? {} : { "Content-Type": "application/json" }) },
            ...(body === undefined ? {} : { body: JSON.stringify(body) }),
          });
        } catch (error) {
          if (signal?.aborted) throw error;
          engineFailed = true;
          throw new SuperOntologyError("SO_ENGINE_UNREACHABLE", { retryable: true });
        }
        if (response.status === 401 && attempt === 0) { await response.arrayBuffer().catch(() => {}); continue; }
        const text = (await response.text()).slice(0, BODY_LIMIT);
        let json = null;
        try { json = text ? JSON.parse(text) : null; } catch { json = null; }
        return { status: response.status, json };
      }
      return { status: 401, json: null };
    };
  }

  /** The file chip needs a file id; search results of the ontology surface name a drive and a path only. */
  async function withFileIds(sources) {
    const out = [];
    for (const source of sources) {
      if (source.kind === "file" && !source.fileId && source.driveId) {
        try {
          const { value } = await client.request("filesList", { params: { id: source.driveId }, query: { q: source.path.split("/").pop(), recursive: true, limit: 50 } });
          const hit = (value?.items ?? []).find((file) => file.path === source.path);
          out.push(hit ? { ...source, fileId: hit.id } : source);
          continue;
        } catch { /* keep the chip without a jump target */ }
      }
      out.push(source);
    }
    return out;
  }

  const outcome = (status, extra = {}) => ({ status, ...extra });

  return {
    "ask-run": async ({ askId, question, history = [] }) => {
      running.get(askId)?.abort();
      const controller = new AbortController();
      running.set(askId, controller);
      const signal = AbortSignal.any([controller.signal, AbortSignal.timeout(ASK_DEADLINE_MS)]);
      engineFailed = false;
      const emit = (event) => broadcast("ask-progress", { askId, ...event });
      try {
        if (!credentials()) return outcome("engine_down");
        let config;
        try { config = (await client.request("modelConfigGet", {})).value; } catch (error) {
          if (error instanceof SuperOntologyError && ENGINE_CODES.has(error.code)) return outcome("engine_down");
          throw error;
        }
        if (!config?.configured) return outcome("no_model");
        let key;
        try {
          key = await readKey(config.key_ref);
          // The engine accepts the address as a `file:` reference too (the model settings of `oo docs model set --base-url file:/...`).
          if (typeof config.base_url === "string" && config.base_url.startsWith("file:")) config = { ...config, base_url: await readKey(config.base_url) };
        } catch (error) {
          if (error instanceof ModelError) return outcome("key_unreadable");
          throw error;
        }
        const registry = createSourceRegistry();
        const tools = createAskTools({ http: makeHttp(await databaseProfile()), registry, control: (name, options) => client.request(name, options) });
        const bound = { chat: ({ system, messages, tools: offered, signal: s }) => model.chat({ config, key, system, messages, tools: offered, signal: s }) };
        let result;
        try {
          result = await runAskLoop({ question, history, model: bound, tools, registry, emit, signal });
        } catch (error) {
          if (error instanceof SuperOntologyError && ENGINE_CODES.has(error.code)) return outcome("engine_down");
          if (signal.aborted && !controller.signal.aborted) return outcome("model_timeout");
          throw error;
        }
        if (result.status === "cancelled") return outcome(controller.signal.aborted ? "cancelled" : "model_timeout", { steps: result.steps });
        if (result.status === "model_error") return outcome(result.kind === "timeout" ? "model_timeout" : result.kind === "rejected" ? "model_rejected" : "model_failed", { steps: result.steps, ...(result.httpStatus ? { httpStatus: result.httpStatus } : {}) });
        if (engineFailed && result.steps.length > 0 && result.steps.every((step) => step.ok === false)) return outcome("engine_down");
        const sources = result.sources ? await withFileIds(result.sources) : [];
        return outcome(result.status, {
          answer: result.answer, reason: result.reason ?? "", sources, steps: result.steps, searched: result.searched, cli: result.cli,
          uncited: result.uncited === true, budget: result.budget === true,
        });
      } finally {
        if (running.get(askId) === controller) running.delete(askId);
      }
    },
    "ask-cancel": async ({ askId }) => {
      const controller = running.get(askId);
      controller?.abort();
      return { cancelled: controller !== undefined };
    },
  };
}
