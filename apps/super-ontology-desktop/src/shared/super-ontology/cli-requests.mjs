import { invalidRequest } from "./errors.mjs";
import { DB_WIRED_KIND_IDS, HOST_NAME, SOURCE_NAME, TABLE_ENTRY } from "./db-kinds.mjs";

/**
 * Channels of the CLI parity lane (volume 25 section 7.1): database sources, the pipeline, the Gateway query surface, ontology files and
 * diagnostics. Every one is carried out by the `oo` executable (argument array, never a shell) and answers with the CLI command it ran.
 * No request carries a host path or an `oo` argument vector: paths come from grants, and the main process builds the arguments.
 */
export const CLI_CHANNELS = Object.freeze([
  "cli-db-kinds", "cli-db-list", "cli-db-check", "cli-db-add", "cli-db-remove", "cli-db-test",
  "cli-doctor", "cli-pipeline", "cli-job-start", "cli-job-read",
  "cli-gateway-read", "cli-gateway-set", "cli-gateway-clear", "cli-query",
  "cli-diagnostics", "cli-ontology-export",
]);

export const CLI_OPERATION_ID_REQUIRED = Object.freeze(["cli-db-add", "cli-db-remove", "cli-job-start", "cli-ontology-export"]);
export const QUERY_OPS = Object.freeze(["query", "validate", "tables", "describe", "freshness", "knowledge-search", "knowledge-cat"]);
const JOB_ACTIONS = ["up", "build", "down"];
const REL_PATH = /^(?!\/)(?!.*(^|\/)\.\.?(\/|$))[A-Za-z0-9._@+ -]+(\/[A-Za-z0-9._@+ -]+)*$/u;
const QUALIFIED = /^[A-Za-z0-9_$.\u0080-￿-]{1,200}$/u;
const MAX_EXPORT_BYTES = 4 * 1024 * 1024;

export function createCliRequestFields(field) {
  const entries = (max) => (value) => {
    if (!Array.isArray(value) || value.length > max) throw invalidRequest();
    return value.map((item) => {
      if (typeof item !== "string" || !TABLE_ENTRY.test(item)) throw invalidRequest();
      return item;
    });
  };
  const nonEmptyEntries = (max) => (value) => {
    const parsed = entries(max)(value);
    if (parsed.length === 0) throw invalidRequest();
    return parsed;
  };
  const text = (max, min = 0) => field.string(max, { minLength: min });
  const connection = {
    kind: field.enum(DB_WIRED_KIND_IDS),
    host: field.pattern(HOST_NAME),
    port: field.optional(field.int(1, 65535)),
    database: field.string(128),
    user: field.string(128),
    /** Write-only: reaches `oo` on its standard input or a private file; never returned, logged or put in a command line. */
    password: field.string(1024),
    tlsCaPem: field.optional(field.string(65536)),
    tlsCaGrantId: field.optional(field.grantId),
  };
  const exportFiles = (value) => {
    if (!Array.isArray(value) || value.length === 0 || value.length > 500) throw invalidRequest();
    let total = 0;
    return value.map((item) => {
      if (!item || typeof item !== "object" || Array.isArray(item)) throw invalidRequest();
      for (const key of Object.keys(item)) if (key !== "name" && key !== "text") throw invalidRequest();
      if (typeof item.name !== "string" || item.name.length > 300 || !REL_PATH.test(item.name)) throw invalidRequest();
      if (typeof item.text !== "string") throw invalidRequest();
      total += item.text.length;
      if (total > MAX_EXPORT_BYTES) throw invalidRequest();
      return { name: item.name, text: item.text };
    });
  };
  return {
    "cli-db-kinds": Object.freeze({}),
    "cli-db-list": Object.freeze({}),
    "cli-db-check": Object.freeze({ ...connection }),
    "cli-db-add": Object.freeze({
      ...connection, name: field.pattern(SOURCE_NAME), include: nonEmptyEntries(256), exclude: field.optional(entries(256)),
    }),
    "cli-db-remove": Object.freeze({ name: field.pattern(SOURCE_NAME) }),
    "cli-db-test": Object.freeze({ name: field.pattern(SOURCE_NAME) }),
    "cli-doctor": Object.freeze({}),
    "cli-pipeline": Object.freeze({ action: field.enum(["status", "freshness"]) }),
    "cli-job-start": Object.freeze({ action: field.enum(JOB_ACTIONS) }),
    "cli-job-read": Object.freeze({ jobId: field.uuid, since: field.optional(field.int(0, 1_000_000)) }),
    "cli-gateway-read": Object.freeze({}),
    "cli-gateway-set": Object.freeze({
      url: field.string(2048), tokenGrantId: field.grantId, caGrantId: field.optional(field.grantId),
      transport: field.optional(field.enum(["production", "local-development"])),
    }),
    "cli-gateway-clear": Object.freeze({}),
    "cli-query": Object.freeze({
      op: field.enum(QUERY_OPS),
      sql: field.optional(text(65536, 1)),
      table: field.optional(field.pattern(QUALIFIED)),
      text: field.optional(text(512, 1)),
      path: field.optional(field.string(1024)),
      pathPrefix: field.optional(field.string(1024)),
      offset: field.optional(field.int(0, 1_048_576)),
      limit: field.optional(field.int(1, 100)),
      maxRows: field.optional(field.int(1, 2000)),
      timeoutMs: field.optional(field.int(1, 120_000)),
    }),
    "cli-diagnostics": Object.freeze({}),
    "cli-ontology-export": Object.freeze({ grantId: field.grantId, files: exportFiles }),
  };
}
