import { SuperOntologyError, pickDetails } from "../../shared/super-ontology/errors.mjs";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u;
const KEY = /^[A-Za-z0-9._:~-]{1,200}$/u;
const PATH = /^[^\0\\]{1,1024}$/u;
const BINARY_LIMIT_BYTES = 64 * 1024 * 1024;
const RESPONSE_LIMIT_BYTES = 8 * 1024 * 1024;
const RATE_LIMIT_MAX_WAIT_MS = 10_000;

export const TIMEOUTS_MS = Object.freeze({ read: 10_000, write: 30_000, compile: 60_000, share: 120_000 });

/**
 * The route allow-list (20-frontend §1.4.2): only templates of 10 §8.3 that
 * Desktop uses. A route says how its path parameters are validated, which
 * headers it needs and how long it may take. Nothing here is built from
 * renderer input except validated parameters.
 */
const route = (method, template, options = {}) => Object.freeze({ method, template, timeout: method === "GET" ? "read" : "write", ...options });
const rawRoutes = {
  health: route("GET", "/v1/docs/health"),
  drivesList: route("GET", "/v1/docs/drives"),
  driveCreate: route("POST", "/v1/docs/drives", { idempotent: true }),
  driveGet: route("GET", "/v1/docs/drives/{id}", { params: { id: UUID } }),
  driveUpdate: route("PATCH", "/v1/docs/drives/{id}", { params: { id: UUID }, ifMatch: true }),
  driveDelete: route("DELETE", "/v1/docs/drives/{id}", { params: { id: UUID }, ifMatch: true }),
  driveSync: route("POST", "/v1/docs/drives/{id}:sync", { params: { id: UUID }, idempotent: true }),
  filesList: route("GET", "/v1/docs/drives/{id}/files", { params: { id: UUID } }),
  driveTree: route("GET", "/v1/docs/drives/{id}/tree", { params: { id: UUID } }),
  fileDetail: route("GET", "/v1/docs/files/{fid}", { params: { fid: UUID } }),
  fileBlocks: route("GET", "/v1/docs/files/{fid}/blocks", { params: { fid: UUID } }),
  filesRetry: route("POST", "/v1/docs/files:retry", { idempotent: true }),
  entitiesList: route("GET", "/v1/docs/entities"),
  entityGet: route("GET", "/v1/docs/entities/{eid}", { params: { eid: KEY } }),
  resolve: route("POST", "/v1/docs/resolve"),
  reviewList: route("GET", "/v1/docs/review"),
  reviewDecide: route("POST", "/v1/docs/review:decide", { idempotent: true }),
  packCompile: route("POST", "/v1/docs/context-packs:compile", { idempotent: true, timeout: "compile" }),
  packFile: route("GET", "/v1/docs/context-packs/{pid}/files/{path}", { params: { pid: KEY, path: PATH }, binary: true, timeout: "share" }),
  packGet: route("GET", "/v1/docs/context-packs/{pid}", { params: { pid: KEY } }),
  sharesCreate: route("POST", "/v1/docs/shares", { idempotent: true, timeout: "share" }),
  sharesList: route("GET", "/v1/docs/shares"),
  shareGet: route("GET", "/v1/docs/shares/{sid}", { params: { sid: UUID } }),
  shareRefresh: route("POST", "/v1/docs/shares/{sid}:refresh", { params: { sid: UUID }, idempotent: true, timeout: "share" }),
  shareRevoke: route("POST", "/v1/docs/shares/{sid}:revoke", { params: { sid: UUID }, idempotent: true }),
  suggestionsList: route("GET", "/v1/docs/suggestions"),
  suggestionsDigest: route("GET", "/v1/docs/suggestions:digest"),
  suggestionGet: route("GET", "/v1/docs/suggestions/{id}", { params: { id: UUID } }),
  suggestionDiff: route("GET", "/v1/docs/suggestions/{id}/diff", { params: { id: UUID } }),
  suggestionDecide: route("POST", "/v1/docs/suggestions/{id}:decide", { params: { id: UUID } }),
  shareUpdates: route("GET", "/v1/docs/shares/{sid}/updates", { params: { sid: UUID } }),
  shareGroupRefresh: route("POST", "/v1/docs/share-groups/{gid}:refresh", { params: { gid: UUID }, idempotent: true, timeout: "share" }),
  constraintsList: route("GET", "/v1/docs/constraints"),
  factCorrect: route("POST", "/v1/docs/facts/{id}:correct", { params: { id: UUID }, idempotent: true, ifMatch: true }),
  entitiesMerge: route("POST", "/v1/docs/entities:merge", { idempotent: true, ifMatch: true }),
  entitySplit: route("POST", "/v1/docs/entities/{eid}:split", { params: { eid: KEY }, idempotent: true, ifMatch: true }),
  modelConfigGet: route("GET", "/v1/docs/config/model"),
  modelConfigPut: route("PUT", "/v1/docs/config/model", { ifMatch: true }),
  usage: route("GET", "/v1/docs/usage"),
  ontologyVersions: route("GET", "/v1/docs/ontology/versions"),
  // Loopback API only: the audit of Shares and the Agent tokens (docs-api 1.6).
  auditShares: route("GET", "/v1/docs/audit/shares"),
  auditVerify: route("GET", "/v1/docs/audit/verify"),
  agentTokenCreate: route("POST", "/v1/docs/agent-tokens"),
  agentTokenList: route("GET", "/v1/docs/agent-tokens"),
  agentTokenRevoke: route("DELETE", "/v1/docs/agent-tokens/{id}", { params: { id: UUID } }),
};
export const ROUTES = Object.freeze(rawRoutes);

const invalid = () => new SuperOntologyError("SO_INVALID_REQUEST");

/** A colon is a legal path character (RFC 3986) and entity keys carry one (`person:<id>`); the engine does not decode `%3A` in a path segment. */
const encodeSegment = (value) => encodeURIComponent(value).replace(/%3A/gu, ":");

function buildPath(definition, params = {}) {
  return definition.template.replace(/\{([a-z]+)\}/gu, (_whole, name) => {
    const pattern = definition.params?.[name];
    const value = params[name];
    if (!pattern || typeof value !== "string" || !pattern.test(value)) throw invalid();
    return name === "path" ? value.split("/").map(encodeURIComponent).join("/") : encodeSegment(value);
  });
}

function buildQuery(query = {}) {
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null) continue;
    search.set(key, Array.isArray(value) ? value.join(",") : String(value));
  }
  const text = search.toString();
  return text ? `?${text}` : "";
}

async function readBytes(response) {
  const declared = Number(response.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > BINARY_LIMIT_BYTES) throw new SuperOntologyError("SO_RESPONSE_INVALID");
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.byteLength > BINARY_LIMIT_BYTES) throw new SuperOntologyError("SO_RESPONSE_INVALID");
  return bytes;
}

async function readLimited(response) {
  const declared = Number(response.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > RESPONSE_LIMIT_BYTES) throw new SuperOntologyError("SO_RESPONSE_INVALID");
  const reader = response.body?.getReader?.();
  if (!reader) return "";
  const decoder = new TextDecoder();
  let received = 0;
  let text = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    received += value.byteLength;
    if (received > RESPONSE_LIMIT_BYTES) {
      await reader.cancel().catch(() => {});
      throw new SuperOntologyError("SO_RESPONSE_INVALID");
    }
    text += decoder.decode(value, { stream: true });
  }
  return text + decoder.decode();
}

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const isTimeout = (error) => error?.name === "TimeoutError" || error?.name === "AbortError";

/**
 * HTTP client of the control API (20-frontend §1.4.2). It runs in the main
 * process only. `credentials()` returns `{ baseUrl, token }` for the live
 * instance; the base URL is loopback by construction and never comes from
 * the renderer. Failures become SuperOntologyError (`message` is never kept).
 *
 * @param {object} options
 * @param {() => ({ baseUrl: string, token: string } | null)} options.credentials
 * @param {() => Promise<unknown>} [options.reloadCredentials] re-read serve.json and the token after a 401
 * @param {() => void} [options.onAuthFailure] called when a 401 persists after the token was re-read
 */
export function createControlClient({
  credentials,
  reloadCredentials = async () => false,
  onAuthFailure = () => {},
  fetchImpl = globalThis.fetch,
  logger = console,
  timers = { setTimeout },
  timeouts = TIMEOUTS_MS,
} = {}) {
  async function execute(definition, { params, query, body, operationId, revision, extraHeaders } = {}, attempt = { retried401: false, retried429: false }) {
    const live = credentials();
    if (!live) throw new SuperOntologyError("SO_ENGINE_NOT_RUNNING", { retryable: true });
    const url = `${live.baseUrl}${buildPath(definition, params)}${buildQuery(query)}`;
    const headers = {
      Authorization: `Bearer ${live.token}`,
      "OO-Docs-Api-Min": "1.0",
      Accept: definition.binary ? "*/*" : "application/json",
      ...extraHeaders,
    };
    if (definition.idempotent) {
      if (typeof operationId !== "string" || operationId.length === 0) throw invalid();
      headers["Idempotency-Key"] = operationId;
    }
    if (definition.ifMatch) {
      if (!Number.isSafeInteger(revision) || revision < 0) throw invalid();
      headers["If-Match"] = `"r${revision}"`;
    }
    let payload;
    if (body !== undefined) {
      payload = JSON.stringify(body);
      headers["Content-Type"] = "application/json";
    }
    let response;
    try {
      response = await fetchImpl(url, {
        method: definition.method, headers, body: payload,
        signal: AbortSignal.timeout(timeouts[definition.timeout]), redirect: "error",
      });
    } catch (error) {
      if (isTimeout(error)) throw new SuperOntologyError("SO_TIMEOUT", { retryable: true });
      throw new SuperOntologyError("SO_ENGINE_UNREACHABLE", { retryable: true });
    }

    const apiMajor = Number.parseInt(String(response.headers.get("OO-Docs-Api") ?? "").split(".")[0], 10);
    if (Number.isInteger(apiMajor) && apiMajor !== 1) {
      await response.arrayBuffer().catch(() => {});
      throw new SuperOntologyError("SO_ENGINE_INCOMPATIBLE");
    }

    if (response.status === 429 && !attempt.retried429) {
      await response.arrayBuffer().catch(() => {});
      const waitSeconds = Number(response.headers.get("Retry-After"));
      const wait = Number.isFinite(waitSeconds) && waitSeconds >= 0 ? Math.min(waitSeconds * 1000, RATE_LIMIT_MAX_WAIT_MS) : 1000;
      await new Promise((resolve) => { timers.setTimeout(resolve, wait); });
      return execute(definition, { params, query, body, operationId, revision, extraHeaders }, { ...attempt, retried429: true });
    }

    if (definition.binary && response.ok) {
      try { return { value: await readBytes(response), replayed: false, status: response.status }; } catch (error) {
        if (error instanceof SuperOntologyError) throw error;
        throw new SuperOntologyError("SO_ENGINE_UNREACHABLE", { retryable: true });
      }
    }
    let text;
    try {
      text = await readLimited(response);
    } catch (error) {
      if (error instanceof SuperOntologyError) throw error;
      throw new SuperOntologyError("SO_ENGINE_UNREACHABLE", { retryable: true });
    }
    let parsed = null;
    if (text.length > 0) {
      try { parsed = JSON.parse(text); } catch { parsed = undefined; }
    }

    if (!response.ok) {
      const envelope = isObject(parsed?.error) ? parsed.error : parsed;
      const code = typeof envelope?.code === "string" ? envelope.code : null;
      if (code === "DOCS_ENGINE_UNAUTHENTICATED" && !attempt.retried401) {
        // The token rotates on every daemon start: re-read it once and retry.
        await reloadCredentials();
        return execute(definition, { params, query, body, operationId, revision, extraHeaders }, { ...attempt, retried401: true });
      }
      if (code === "DOCS_ENGINE_UNAUTHENTICATED") onAuthFailure();
      if (!code) throw new SuperOntologyError("SO_RESPONSE_INVALID", { httpStatus: response.status });
      // The engine's `message` is English text for logs only; keep the code, never the text.
      logger.debug?.("Control API error.", { code, httpStatus: response.status });
      throw new SuperOntologyError(code, {
        retryable: envelope.retryable === true,
        details: pickDetails(code, envelope.details),
        httpStatus: response.status,
      });
    }
    if (response.status === 204 || text.length === 0) return { value: null, replayed: false, status: response.status };
    if (parsed === undefined) throw new SuperOntologyError("SO_RESPONSE_INVALID", { httpStatus: response.status });
    return { value: parsed, replayed: response.headers.get("OO-Idempotent-Replay") === "true", status: response.status };
  }

  return Object.freeze({
    /** @returns {Promise<{ value: any, replayed: boolean, status: number }>} */
    async request(name, options) {
      const definition = ROUTES[name];
      if (!definition) throw invalid();
      return execute(definition, options);
    },
  });
}
