import { DOCS_ERROR_DETAIL_KEYS } from "./docs-contract.generated.mjs";

/** Main-process error codes for Super Drive (20-frontend §1.6). `DOCS_*` codes come from the engine and are passed through later. */
export const SO_ERROR_CODES = Object.freeze([
  "SO_BRIDGE_UNAVAILABLE", "SO_INVALID_REQUEST", "SO_PLATFORM_UNSUPPORTED", "SO_OO_MISSING", "SO_OO_INVALID",
  "SO_DOCKER_MISSING", "SO_DOCKER_STOPPED", "SO_SERVE_IMAGE_INVALID", "SO_SERVE_STATE_DIR_INVALID",
  "SO_SERVE_PORT_IN_USE", "SO_SERVE_START_TIMEOUT", "SO_SERVE_CREDENTIALS_INVALID", "SO_ENGINE_NOT_RUNNING",
  "SO_ENGINE_UNREACHABLE", "SO_ENGINE_INCOMPATIBLE", "SO_ENGINE_NOT_OWNED", "SO_TIMEOUT", "SO_RESPONSE_INVALID",
  "SO_GRANT_REQUIRED", "SO_EVENTS_GAP", "SO_UPLOAD_SOURCE_MISSING", "SO_UPLOAD_CONFLICT",
  "SO_UPLOAD_UNSUPPORTED_BACKEND", "SO_UPLOAD_DISK_FULL", "SO_UPLOAD_PERMISSION_DENIED", "SO_UPLOAD_FAILED", "SO_UPLOAD_TOO_MANY", "SO_PREVIEW_UNAVAILABLE", "SO_SECRET_STORE_UNAVAILABLE", "SO_WRITEBACK_CONFLICT", "SO_WRITEBACK_UNSUPPORTED", "SO_WRITEBACK_FAILED",
  "SO_EMBEDDED_RUNTIME_MISSING", "SO_CLOUD_NOT_CONFIGURED", "SO_CLOUD_SESSION_REQUIRED", "SO_CLOUD_ROUTE_UNAVAILABLE",
  "SO_MEDIA_DECODE_FAILED", "SO_MEDIA_CANCELLED",
  // Fallback for anything unexpected; never carries the native message.
  "SO_INTERNAL",
]);

/**
 * Per-code allow-list of `details` keys (10 §8.4): engine codes use the list
 * generated from errors.v1.json, `SO_*` codes carry none. Values must be
 * plain JSON scalars/arrays and can never contain a credential-looking string.
 */
const CREDENTIAL_LOOKING = /ood[tri]_/u;

function isSafeDetailValue(value, depth = 0) {
  if (value === null || ["string", "number", "boolean"].includes(typeof value)) {
    return typeof value !== "string" || (!CREDENTIAL_LOOKING.test(value) && value.length <= 2048);
  }
  if (Array.isArray(value) && depth < 2 && value.length <= 500) return value.every((item) => isSafeDetailValue(item, depth + 1));
  return false;
}

/** Keeps only allow-listed keys with safe values. */
export function pickDetails(code, details) {
  const allowed = DOCS_ERROR_DETAIL_KEYS[code] ?? [];
  const picked = {};
  if (details && typeof details === "object" && !Array.isArray(details)) {
    for (const key of allowed) {
      if (Object.hasOwn(details, key) && isSafeDetailValue(details[key])) picked[key] = details[key];
    }
  }
  return picked;
}

export class SuperOntologyError extends Error {
  constructor(code, { retryable = false, details = {}, httpStatus } = {}) {
    super(`Super Ontology: ${code}`);
    this.name = "SuperOntologyError";
    this.code = code;
    this.retryable = retryable;
    this.details = details;
    this.httpStatus = httpStatus;
  }
}

export function invalidRequest() {
  return new SuperOntologyError("SO_INVALID_REQUEST");
}

/** Never serialize a native Error: a remote party may have echoed a credential into its message. */
export function toErrorResult(error) {
  const known = error instanceof SuperOntologyError
    && (SO_ERROR_CODES.includes(error.code) || Object.hasOwn(DOCS_ERROR_DETAIL_KEYS, error.code));
  const code = known ? error.code : "SO_INTERNAL";
  const result = { code, retryable: known && error.retryable === true, details: known ? pickDetails(code, error.details) : {} };
  if (known && Number.isInteger(error.httpStatus)) result.httpStatus = error.httpStatus;
  return { ok: false, error: result };
}
