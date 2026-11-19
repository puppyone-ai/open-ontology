import { invalidRequest } from "./errors.mjs";

/**
 * Request validators of the L4 and L6 channels (reviews, entities, shares, usage, model, media). Built from the base
 * `field` set of requests.mjs so the two modules do not import each other. Nothing here accepts a host path: paths come
 * from grants, not from the renderer.
 */
const KEY = /^[A-Za-z0-9._:~-]{1,200}$/u;
const MAX_JSON_BYTES = 16 * 1024;

export const ONTOLOGY_CHANNELS = Object.freeze([
  "entities-search", "entity-detail", "resolve", "review-list", "review-decide", "pack-compile", "pack-read",
  "shares-list", "share-get", "share-mount", "share-refresh", "share-revoke", "share-reveal", "share-updates", "share-group-refresh",
  "usage-read", "ontology-versions", "model-config-read", "model-config-save",
  "fact-correct", "entity-merge", "entity-split", "constraints-list", "media-decode", "media-decode-cancel",
]);

/** Channels of this module whose request must carry an `operationId`. */
export const ONTOLOGY_OPERATION_ID_REQUIRED = Object.freeze([
  "review-decide", "pack-compile", "share-mount", "share-refresh", "share-revoke", "share-group-refresh",
  "fact-correct", "entity-merge", "entity-split",
]);

const ENTITY_TYPES = ["Person", "Organization", "Project", "Decision", "Document", "Topic", "Event", "Term", "Thing"];
const REVIEW_KINDS = [
  "low_confidence_fact", "entity_merge", "alias_conflict", "orphaned_overlay", "type_promotion", "suggestion",
  "reprocess_diff", "constraint_conflict", "speaker_identity",
];
const SHARE_STATES = ["active", "drifted", "refreshed", "expired", "revoked"];
const REFRESH_POLICIES = ["manual", "at-checkpoint", "pinned"];

export function createOntologyRequestFields(field) {
  const plainObject = (value) => {
    if (!value || typeof value !== "object" || Array.isArray(value)) throw invalidRequest();
    let size;
    try { size = JSON.stringify(value).length; } catch { throw invalidRequest(); }
    if (size > MAX_JSON_BYTES) throw invalidRequest();
    return value;
  };
  const key = (value) => {
    if (typeof value !== "string" || !KEY.test(value)) throw invalidRequest();
    return value;
  };
  const enumList = (values, max) => (value) => {
    if (!Array.isArray(value) || value.length === 0 || value.length > max) throw invalidRequest();
    return value.map((item) => field.enum(values)(item));
  };
  const items = (value) => {
    if (!Array.isArray(value) || value.length === 0 || value.length > 5000) throw invalidRequest();
    return value.map((item) => {
      if (!item || typeof item !== "object" || Array.isArray(item)) throw invalidRequest();
      for (const name of Object.keys(item)) if (!["id", "revision", "decision", "edit", "note"].includes(name)) throw invalidRequest();
      const parsed = {
        id: field.uuid(item.id),
        revision: field.int(0, Number.MAX_SAFE_INTEGER)(item.revision),
        decision: field.enum(["accept", "reject", "edit"])(item.decision),
      };
      if (item.edit !== undefined) parsed.edit = plainObject(item.edit);
      if (item.note !== undefined) parsed.note = field.string(500)(item.note);
      return parsed;
    });
  };
  const bySource = (value) => {
    if (!value || typeof value !== "object" || Array.isArray(value)) throw invalidRequest();
    for (const name of Object.keys(value)) if (!["fileId", "kinds", "decision", "asOfSeq"].includes(name)) throw invalidRequest();
    const parsed = { fileId: field.uuid(value.fileId), decision: field.enum(["accept", "reject"])(value.decision) };
    if (value.kinds !== undefined) parsed.kinds = enumList(REVIEW_KINDS, 16)(value.kinds);
    if (value.asOfSeq !== undefined) parsed.asOfSeq = field.int(0, Number.MAX_SAFE_INTEGER)(value.asOfSeq);
    return parsed;
  };
  const compile = (value) => {
    if (!value || typeof value !== "object" || Array.isArray(value)) throw invalidRequest();
    const allowed = ["driveId", "scope", "task", "seeds", "budget", "role", "version", "requireComplete", "waitMs", "minDriveRev", "vector"];
    for (const name of Object.keys(value)) if (!allowed.includes(name)) throw invalidRequest();
    const parsed = { driveId: field.uuid(value.driveId) };
    if (value.scope !== undefined) {
      if (!Array.isArray(value.scope) || value.scope.length > 64) throw invalidRequest();
      parsed.scope = value.scope.map((entry) => field.drivePath(entry));
    }
    if (value.task !== undefined) parsed.task = field.string(2000, { minLength: 0 })(value.task);
    if (value.seeds !== undefined) parsed.seeds = field.stringList(20, 200)(value.seeds);
    if (value.budget !== undefined) parsed.budget = field.int(200, 20_000)(value.budget);
    if (value.role !== undefined) parsed.role = field.enum(["viewer", "suggester", "editor"])(value.role);
    if (value.version !== undefined) parsed.version = field.int(1, Number.MAX_SAFE_INTEGER)(value.version);
    if (value.requireComplete !== undefined) parsed.requireComplete = field.boolean(value.requireComplete);
    if (value.waitMs !== undefined) parsed.waitMs = field.int(0, 120_000)(value.waitMs);
    if (value.minDriveRev !== undefined) parsed.minDriveRev = field.int(0, Number.MAX_SAFE_INTEGER)(value.minDriveRev);
    if (value.vector !== undefined) parsed.vector = field.enum(["hashgram/1"])(value.vector);
    return parsed;
  };
  const splitGroups = (value) => {
    if (!Array.isArray(value) || value.length < 2 || value.length > 20) throw invalidRequest();
    return value.map((group) => {
      if (!group || typeof group !== "object" || Array.isArray(group)) throw invalidRequest();
      for (const name of Object.keys(group)) if (!["aliases", "facts"].includes(name)) throw invalidRequest();
      const parsed = {};
      if (group.aliases !== undefined) parsed.aliases = field.stringList(200, 256)(group.aliases);
      if (group.facts !== undefined) parsed.facts = field.stringList(500, 64)(group.facts);
      return parsed;
    });
  };
  const rev = field.int(0, Number.MAX_SAFE_INTEGER);
  const limit = field.optional(field.int(1, 500));
  return Object.freeze({
    "entities-search": Object.freeze({ q: field.string(256, { minLength: 0 }), driveId: field.optional(field.uuid), type: field.optional(field.enum(ENTITY_TYPES)), limit, cursor: field.optional(field.cursor) }),
    "entity-detail": Object.freeze({ entityIdOrKey: key, includeHistory: field.optional(field.boolean) }),
    "resolve": Object.freeze({ text: field.string(2000), driveId: field.optional(field.uuid), types: field.optional(enumList(ENTITY_TYPES, 9)), limit: field.optional(field.int(1, 50)) }),
    "review-list": Object.freeze({
      driveId: field.uuid, kind: field.optional(field.enum(REVIEW_KINDS)), source: field.optional(field.uuid),
      status: field.optional(field.enum(["pending", "accepted", "rejected", "superseded"])), groupBy: field.optional(field.enum(["source"])), limit, cursor: field.optional(field.cursor),
    }),
    "review-decide": Object.freeze({ items: field.optional(items), bySource: field.optional(bySource) }),
    "pack-compile": Object.freeze({ compile }),
    "pack-read": Object.freeze({ packId: key }),
    "shares-list": Object.freeze({ driveId: field.optional(field.uuid), states: field.optional(enumList(SHARE_STATES, 5)), limit, cursor: field.optional(field.cursor) }),
    "share-get": Object.freeze({ shareId: field.uuid }),
    "share-mount": Object.freeze({
      compile, targetGrantId: field.grantId, expiresInSeconds: field.int(3600, 2_592_000),
      group: field.optional(field.string(128)), refreshPolicy: field.optional(field.enum(REFRESH_POLICIES)),
    }),
    "share-refresh": Object.freeze({ shareId: field.uuid }),
    "share-revoke": Object.freeze({ shareId: field.uuid, reason: field.optional(field.string(200)) }),
    "share-reveal": Object.freeze({ shareId: field.uuid }),
    "share-updates": Object.freeze({ shareId: field.uuid, limit, cursor: field.optional(field.cursor) }),
    "share-group-refresh": Object.freeze({ groupId: field.uuid }),
    "usage-read": Object.freeze({ driveId: field.optional(field.uuid), days: field.optional(field.int(1, 90)) }),
    "ontology-versions": Object.freeze({ limit, cursor: field.optional(field.cursor) }),
    "model-config-read": Object.freeze({}),
    "model-config-save": Object.freeze({
      provider: field.enum(["openai-compatible", "anthropic"]), baseUrl: field.string(2048), model: field.string(256),
      apiKey: field.optional(field.string(512)), revision: field.optional(rev),
    }),
    "fact-correct": Object.freeze({
      factId: field.uuid, revision: rev, action: field.enum(["mark_wrong", "set_value"]),
      value: field.optional(plainObject), reason: field.optional(field.string(500)),
    }),
    "entity-merge": Object.freeze({ into: key, from: field.stringList(50, 200), revision: rev }),
    "entity-split": Object.freeze({ entityId: key, revision: rev, groups: splitGroups }),
    "constraints-list": Object.freeze({ limit, cursor: field.optional(field.cursor) }),
    "media-decode": Object.freeze({ fileId: field.uuid }),
    "media-decode-cancel": Object.freeze({ fileId: field.uuid }),
  });
}
