import { invalidRequest } from "./errors.mjs";
import { CLI_CHANNELS, CLI_OPERATION_ID_REQUIRED, createCliRequestFields } from "./cli-requests.mjs";
import { ONTOLOGY_CHANNELS, ONTOLOGY_OPERATION_ID_REQUIRED, createOntologyRequestFields } from "./ontology-requests.mjs";

export const SUPER_ONTOLOGY_CHANNEL_PREFIX = "super-ontology:";

/** L7 and L8 channels (Cloud proxy mode, audit, runtime): placed after the ontology channels. */
export const ENTERPRISE_CHANNELS = Object.freeze([
  "cloud-mode-read", "cloud-mode-set", "audit-shares", "audit-verify", "cloud-token-create", "cloud-token-revoke", "engine-set-runtime",
  "engine-connect-remote", "engine-disconnect-remote", "engine-state-read", "engine-data-use",
]);

/** Agent › Connect: Agent tokens (docs-api 1.6) and the connection text for an Agent. Nothing here writes into another app's files. */
export const AGENT_CHANNELS = Object.freeze(["agent-token-create", "agent-token-list", "agent-token-revoke", "agent-connect-snippet"]);

/** Ontology › Ask: one question answered by a bounded tool loop in the main process; progress comes back on the `ask-progress` push. */
export const ASK_CHANNELS = Object.freeze(["ask-run", "ask-cancel"]);

/** Request/response channels implemented so far. `electron/preload.cjs` repeats this list (a sandboxed preload cannot import it); a test keeps both equal. */
export const SUPER_ONTOLOGY_CHANNELS = Object.freeze([
  "engine-read", "engine-preflight", "engine-start", "engine-stop", "engine-restart", "engine-set-keep",
  "engine-reveal-logs", "pick-path", "engine-set-executable",
  "drives-list", "drive-get", "drive-create", "my-files-ensure", "drive-update", "drive-delete", "drive-sync", "drive-tree", "files-list",
  "events-subscribe", "events-since", "events-unsubscribe", "upload-start", "upload-cancel",
  "files-retry", "files-retry-matching", "file-detail", "file-blocks", "file-preview-url",
  "suggestions-list", "suggestions-digest", "suggestion-diff", "suggestion-decide", "suggestions-decide-source",
  ...ONTOLOGY_CHANNELS,
  ...ENTERPRISE_CHANNELS,
  ...AGENT_CHANNELS,
  ...ASK_CHANNELS,
  ...CLI_CHANNELS,
]);

/** Push channels main -> renderer. Same duplication rule as above. */
export const SUPER_ONTOLOGY_PUSH_CHANNELS = Object.freeze([
  "engine-changed", "events", "connection-changed", "resync-required", "upload-progress", "ask-progress",
]);

const OPERATION_ID = /^[A-Za-z0-9_-]{16,128}$/u;
const GRANT_ID = /^[A-Za-z0-9-]{8,64}$/u;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u;
const CURSOR = /^[A-Za-z0-9_.=~-]{1,1024}$/u;
const STATE_NAME = /^[a-z_]{1,32}$/u;
const CONTROL_CHARACTERS = /[\u0000-\u001f\u007f]/u;
// DrivePath of docs-api (contract): relative, no "//", no "." or ".." segments.
const DRIVE_PATH = /^(?!\/)(?!.*\/\/)(?!.*(^|\/)\.\.?(\/|$)).*$/su;

/** Field validators: return the normalized value or throw SO_INVALID_REQUEST. */
export const field = Object.freeze({
  enum: (values) => (value) => {
    if (typeof value !== "string" || !values.includes(value)) throw invalidRequest();
    return value;
  },
  boolean: (value) => {
    if (typeof value !== "boolean") throw invalidRequest();
    return value;
  },
  grantId: (value) => {
    if (typeof value !== "string" || !GRANT_ID.test(value)) throw invalidRequest();
    return value;
  },
  uuid: (value) => {
    if (typeof value !== "string" || !UUID.test(value)) throw invalidRequest();
    return value;
  },
  int: (min, max) => (value) => {
    if (!Number.isSafeInteger(value) || value < min || value > max) throw invalidRequest();
    return value;
  },
  string: (maxLength, { minLength = 1 } = {}) => (value) => {
    if (typeof value !== "string" || value.length < minLength || value.length > maxLength || CONTROL_CHARACTERS.test(value)) throw invalidRequest();
    return value;
  },
  /** Drive-relative path; the empty string is the Drive root. */
  drivePath: (value) => {
    if (typeof value !== "string" || value.length > 1024 || CONTROL_CHARACTERS.test(value) || !DRIVE_PATH.test(value)) throw invalidRequest();
    return value;
  },
  cursor: (value) => {
    if (typeof value !== "string" || !CURSOR.test(value)) throw invalidRequest();
    return value;
  },
  detailIncludes: (value) => {
    if (!Array.isArray(value) || value.length > 4) throw invalidRequest();
    return value.map((item) => field.enum(["history", "entities", "facts", "shares"])(item));
  },
  pattern: (expression) => (value) => {
    if (typeof value !== "string" || !expression.test(value)) throw invalidRequest();
    return value;
  },
  scopeList: (value) => {
    if (!Array.isArray(value) || value.length === 0 || value.length > 4 || new Set(value).size !== value.length) throw invalidRequest();
    return value.map((item) => field.enum(["docs:read", "docs:suggest", "docs:write", "db:read"])(item));
  },
  uuidList: (maxItems) => (value) => {
    if (!Array.isArray(value) || value.length === 0 || value.length > maxItems) throw invalidRequest();
    return value.map((item) => field.uuid(item));
  },
  stringList: (maxItems, maxLength) => (value) => {
    if (!Array.isArray(value) || value.length > maxItems) throw invalidRequest();
    return value.map((item) => {
      if (typeof item !== "string" || item.length === 0 || item.length > maxLength || CONTROL_CHARACTERS.test(item)) throw invalidRequest();
      return item;
    });
  },
  /** Access-policy prefixes: relative Drive paths, no empty entry, no `.`/`..`; a trailing slash is dropped. */
  prefixList: (maxItems) => (value) => {
    if (!Array.isArray(value) || value.length > maxItems) throw invalidRequest();
    return value.map((item) => {
      const trimmed = typeof item === "string" ? item.replace(/\/+$/u, "") : item;
      if (typeof trimmed !== "string" || trimmed.length === 0) throw invalidRequest();
      return field.drivePath(trimmed);
    });
  },
  fileStates: (value) => {
    if (!Array.isArray(value) || value.length === 0 || value.length > 24) throw invalidRequest();
    return value.map((item) => {
      if (typeof item !== "string" || !STATE_NAME.test(item)) throw invalidRequest();
      return item;
    });
  },
  /** DrivePatch (contract): only these keys, each validated; at least one. */
  drivePatch: (value) => {
    if (!value || typeof value !== "object" || Array.isArray(value)) throw invalidRequest();
    const allowed = {
      name: field.string(128),
      model_enabled: field.boolean,
      llm_daily_calls: field.int(0, 1_000_000),
      paused: field.boolean,
      exclude: field.stringList(64, 256),
    };
    const patch = {};
    for (const key of Object.keys(value)) {
      if (key === "review") {
        const review = value.review;
        if (!review || typeof review !== "object" || Array.isArray(review)) throw invalidRequest();
        for (const reviewKey of Object.keys(review)) if (reviewKey !== "auto_publish_bp") throw invalidRequest();
        if (review.auto_publish_bp === undefined) throw invalidRequest();
        patch.review = { auto_publish_bp: field.int(5000, 10_000)(review.auto_publish_bp) };
      } else if (Object.hasOwn(allowed, key)) {
        patch[key] = allowed[key](value[key]);
      } else {
        throw invalidRequest();
      }
    }
    if (Object.keys(patch).length === 0) throw invalidRequest();
    return patch;
  },
  optional: (validate) => ({ optional: true, validate }),
});

/**
 * Allowed request fields per channel. Every request may also carry `operationId`.
 * A field is a validator, or `field.optional(validator)`; absent required
 * fields and unknown fields are rejected.
 */
const ASK_ID = /^[A-Za-z0-9_-]{8,64}$/u;
/** Question and answer text keep their line breaks and tabs; any other control character is refused. */
const askText = (max, { minLength = 1 } = {}) => (value) => {
  if (typeof value !== "string" || value.length < minLength || value.length > max || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/u.test(value)) throw invalidRequest();
  return value;
};
const askHistory = (value) => {
  if (!Array.isArray(value) || value.length > 4) throw invalidRequest();
  return value.map((turn) => {
    if (!turn || typeof turn !== "object" || Array.isArray(turn) || Object.keys(turn).some((name) => !["question", "answer"].includes(name))) throw invalidRequest();
    return { question: askText(2000)(turn.question), answer: askText(4000, { minLength: 0 })(turn.answer) };
  });
};

const REQUEST_FIELDS = Object.freeze({
  ...createOntologyRequestFields(field),
  "ask-run": Object.freeze({ askId: field.pattern(ASK_ID), question: askText(2000), history: field.optional(askHistory) }),
  "ask-cancel": Object.freeze({ askId: field.pattern(ASK_ID) }),
  ...createCliRequestFields(field),
  "engine-read": Object.freeze({}),
  "engine-preflight": Object.freeze({}),
  "engine-start": Object.freeze({ keepEngineOnQuit: field.optional(field.boolean) }),
  "engine-stop": Object.freeze({}),
  "engine-restart": Object.freeze({}),
  "engine-set-keep": Object.freeze({ keepEngineOnQuit: field.boolean }),
  "engine-reveal-logs": Object.freeze({ target: field.optional(field.enum(["logs", "state"])) }),
  "pick-path": Object.freeze({ purpose: field.enum(["drive-root", "mount-target", "oo-executable", "token-file", "ca-file", "export-dir"]) }),
  "engine-set-executable": Object.freeze({ grantId: field.grantId }),
  "engine-connect-remote": Object.freeze({ url: field.string(2048), tokenGrantId: field.grantId }),
  "engine-disconnect-remote": Object.freeze({}),
  "engine-state-read": Object.freeze({}),
  "engine-data-use": Object.freeze({ mode: field.enum(["cli", "app"]) }),
  "engine-set-runtime": Object.freeze({ runtime: field.enum(["auto", "docker", "lite"]) }),
  "events-subscribe": Object.freeze({ sinceSeq: field.optional(field.int(0, Number.MAX_SAFE_INTEGER)) }),
  "events-since": Object.freeze({ instanceId: field.string(128), seq: field.int(0, Number.MAX_SAFE_INTEGER) }),
  "events-unsubscribe": Object.freeze({}),
  "upload-start": Object.freeze({ driveId: field.uuid, folder: field.drivePath, paths: field.stringList(1000, 4096) }),
  "upload-cancel": Object.freeze({ batchId: field.uuid, itemId: field.optional(field.uuid) }),
  "files-retry": Object.freeze({ fileIds: field.uuidList(500), mode: field.optional(field.enum(["normal", "safe"])) }),
  "files-retry-matching": Object.freeze({
    driveId: field.uuid,
    path: field.optional(field.drivePath),
    recursive: field.optional(field.boolean),
    states: field.optional(field.fileStates),
    q: field.optional(field.string(256)),
    mode: field.optional(field.enum(["normal", "safe"])),
  }),
  "file-detail": Object.freeze({ fileId: field.uuid, include: field.optional(field.detailIncludes) }),
  "file-blocks": Object.freeze({ fileId: field.uuid, page: field.optional(field.int(1, 100_000)), limit: field.optional(field.int(1, 500)), cursor: field.optional(field.cursor) }),
  "file-preview-url": Object.freeze({ fileId: field.uuid }),
  "drives-list": Object.freeze({}),
  "drive-get": Object.freeze({ driveId: field.uuid }),
  // fs needs `rootGrantId`; s3 needs `endpoint`, `bucket` and the two keys. The handler checks which set applies.
  "my-files-ensure": Object.freeze({}),
  "drive-create": Object.freeze({
    name: field.string(128),
    backend: field.enum(["fs", "s3", "oss", "cos", "r2"]),
    rootGrantId: field.optional(field.grantId),
    endpoint: field.optional(field.string(2048)),
    region: field.optional(field.string(64)),
    bucket: field.optional(field.string(63, { minLength: 3 })),
    prefix: field.optional(field.string(1024)),
    pathStyle: field.optional(field.boolean),
    accessKeyId: field.optional(field.string(1024)),
    secretAccessKey: field.optional(field.string(4096)),
    sessionToken: field.optional(field.string(8192)),
    /** R2 only: the 32-hex account id the engine derives the endpoint from. */
    accountId: field.optional(field.string(32, { minLength: 32 })),
    /** OSS and COS only: the in-cloud endpoint. */
    internal: field.optional(field.boolean),
    /** Drive access policy (docs-api 1.4): whole-segment path prefixes; empty allow means the whole Drive, deny always wins. */
    allowPrefixes: field.optional(field.prefixList(32)),
    denyPrefixes: field.optional(field.prefixList(32)),
    exclude: field.optional(field.stringList(64, 256)),
    modelEnabled: field.optional(field.boolean),
    llmDailyCalls: field.optional(field.int(0, 1_000_000)),
  }),
  "drive-update": Object.freeze({ driveId: field.uuid, revision: field.int(0, Number.MAX_SAFE_INTEGER), patch: field.drivePatch }),
  "drive-delete": Object.freeze({ driveId: field.uuid, revision: field.int(0, Number.MAX_SAFE_INTEGER) }),
  "drive-sync": Object.freeze({
    driveId: field.uuid,
    deep: field.optional(field.boolean),
    confirmMassDelete: field.optional(field.int(0, Number.MAX_SAFE_INTEGER)),
  }),
  "drive-tree": Object.freeze({ driveId: field.uuid, path: field.drivePath }),
  "suggestions-list": Object.freeze({
    status: field.optional(field.enum(["pending", "auto_accepted", "superseded_silently", "accepted", "rejected", "undone", "duplicate"])),
    shareId: field.optional(field.uuid), day: field.optional(field.string(10, { minLength: 10 })), limit: field.optional(field.int(1, 500)),
  }),
  "suggestions-digest": Object.freeze({ day: field.optional(field.string(10, { minLength: 10 })) }),
  "suggestion-diff": Object.freeze({ id: field.uuid }),
  "suggestion-decide": Object.freeze({ id: field.uuid, decision: field.enum(["accept", "reject", "undo"]), revision: field.optional(field.int(0, Number.MAX_SAFE_INTEGER)), note: field.optional(field.string(500)) }),
  "suggestions-decide-source": Object.freeze({ shareId: field.uuid, decision: field.enum(["accept", "reject"]) }),
  "cloud-mode-read": Object.freeze({}),
  "cloud-mode-set": Object.freeze({ mode: field.enum(["local", "cloud"]) }),
  "audit-shares": Object.freeze({
    driveId: field.optional(field.uuid), agent: field.optional(field.string(200)), tenant: field.optional(field.string(200)),
    since: field.optional(field.string(40)), until: field.optional(field.string(40)),
  }),
  "audit-verify": Object.freeze({}),
  "cloud-token-create": Object.freeze({
    tenant: field.string(200), driveIds: field.uuidList(64), label: field.optional(field.string(200)),
    expiresInSeconds: field.optional(field.int(60, 31_536_000)),
  }),
  "cloud-token-revoke": Object.freeze({ tokenId: field.uuid }),
  "agent-token-create": Object.freeze({
    name: field.pattern(/^[A-Za-z0-9._-]{1,64}$/u),
    scopes: field.scopeList,
    driveIds: field.optional(field.uuidList(256)),
    allDrives: field.optional(field.boolean),
    gwProfile: field.optional(field.pattern(/^[A-Za-z0-9._-]{1,64}$/u)),
    label: field.optional(field.string(200)),
    expiresInSeconds: field.optional(field.int(60, 2_592_000)),
  }),
  "agent-token-list": Object.freeze({}),
  "agent-token-revoke": Object.freeze({ tokenId: field.uuid }),
  "agent-connect-snippet": Object.freeze({ agent: field.enum(["generic", "claude", "cursor", "codex"]), profile: field.pattern(/^[A-Za-z0-9._-]{1,64}$/u) }),
  "files-list": Object.freeze({
    driveId: field.uuid,
    path: field.optional(field.drivePath),
    recursive: field.optional(field.boolean),
    states: field.optional(field.fileStates),
    q: field.optional(field.string(256)),
    sort: field.optional(field.enum(["path", "state_since"])),
    limit: field.optional(field.int(1, 500)),
    cursor: field.optional(field.cursor),
  }),
});

const S3_ONLY_FIELDS = Object.freeze(["endpoint", "region", "bucket", "prefix", "pathStyle", "accessKeyId", "secretAccessKey", "sessionToken", "accountId", "internal"]);

/** A folder Drive carries a grant and no object-store fields; an object-store Drive (s3, oss, cos, r2) carries no grant (its fields are checked by the handler, which can name the field). */
function checkDriveCreateShape(parsed) {
  if (parsed.backend === "fs") {
    if (parsed.rootGrantId === undefined || S3_ONLY_FIELDS.some((key) => parsed[key] !== undefined)) throw invalidRequest();
  } else if (parsed.rootGrantId !== undefined) {
    throw invalidRequest();
  }
}

/** Channels with host side effects must carry an `operationId` (it becomes the Idempotency-Key). */
export const OPERATION_ID_REQUIRED = Object.freeze(["drive-create", "my-files-ensure", "drive-sync", "files-retry", "files-retry-matching", ...ONTOLOGY_OPERATION_ID_REQUIRED, ...CLI_OPERATION_ID_REQUIRED]);

/** @returns {Readonly<Record<string, unknown>>} a validated, frozen copy; throws SuperOntologyError(SO_INVALID_REQUEST). */
export function parseSuperOntologyRequest(channel, raw) {
  if (typeof channel !== "string" || !Object.hasOwn(REQUEST_FIELDS, channel)) throw invalidRequest();
  const input = raw ?? {};
  if (typeof input !== "object" || Array.isArray(input)) throw invalidRequest();
  const schema = REQUEST_FIELDS[channel];
  for (const key of Object.keys(input)) {
    if (key !== "operationId" && !Object.hasOwn(schema, key)) throw invalidRequest();
  }
  const parsed = {};
  if (input.operationId !== undefined) {
    if (typeof input.operationId !== "string" || !OPERATION_ID.test(input.operationId)) throw invalidRequest();
    parsed.operationId = input.operationId;
  } else if (OPERATION_ID_REQUIRED.includes(channel)) {
    throw invalidRequest();
  }
  for (const [name, definition] of Object.entries(schema)) {
    const optional = typeof definition === "object" && definition.optional === true;
    const validate = optional ? definition.validate : definition;
    if (input[name] === undefined) {
      if (!optional) throw invalidRequest();
      continue;
    }
    parsed[name] = validate(input[name]);
  }
  if (channel === "drive-create") checkDriveCreateShape(parsed);
  return Object.freeze(parsed);
}
