/**
 * Legal client for published operation strings.
 * B04 is schema_frozen at the 7bb contract. B02 and B07 have no committed OpenAPI.
 * CSRF comes only from a typed getIdentityCurrent body. This module does not call fetch.
 */

import { SELFBUILD_COMMIT, SELFBUILD_HTTP, SELFBUILD_OPENAPI, bindingById } from "./bindings";

export const CONTROL_IDENTITY_ERROR_CODES = [
  "malformed",
  "unauthenticated",
  "forbidden",
  "origin.rejected",
  "csrf.rejected",
  "revision.conflict",
  "state.rejected",
  "nonce.rejected",
  "pkce.rejected",
  "redirect.rejected",
  "issuer.rejected",
  "audience.rejected",
  "token.expired",
  "token.rejected",
  "kid.unknown",
  "kid.ambiguous",
  "alg.rejected",
  "jwks.rejected",
  "discovery.rejected",
  "bootstrap.expired",
  "bootstrap.replay",
  "bootstrap.rejected",
  "bootstrap.subject_mismatch",
  "bootstrap.unavailable",
  "commit.unknown",
  "unavailable",
  "not_found",
  "method.not_allowed",
] as const;

export const SELFBUILD_ERROR_CODES = [
  "malformed",
  "unauthenticated",
  "forbidden",
  "origin.rejected",
  "csrf.rejected",
  "revision.conflict",
  "idempotency.conflict",
  "not_found",
  "method.not_allowed",
  "dependency.not_ready",
  "membership.refused",
  "classification.incomplete",
  "pii.unknown",
  "batch.accept_all",
  "commit.unknown",
  "resume.uncertain",
] as const;

export type ControlIdentityErrorCode = (typeof CONTROL_IDENTITY_ERROR_CODES)[number];
export type SelfbuildErrorCode = (typeof SELFBUILD_ERROR_CODES)[number];

export type ParsedProductError<Code extends string> = {
  ok: true;
  code: Code;
  message: string;
  retryable: boolean;
};

export type ProductErrorFailure = {
  ok: false;
  reason: "invalid_schema";
};

const IDENTITY_ERROR_CODE_SET = new Set<string>(CONTROL_IDENTITY_ERROR_CODES);
const SELFBUILD_ERROR_CODE_SET = new Set<string>(SELFBUILD_ERROR_CODES);
const IDENTITY_PRECISE_IDS = [
  "startAuthorizationCodeLogin",
  "completeAuthorizationCodeCallback",
  "getIdentityCurrent",
  "logoutManagementSession",
  "consumeBootstrapToken",
] as const;
const PRECISE_IDS = new Set<string>([...IDENTITY_PRECISE_IDS, ...SELFBUILD_HTTP.map(([operationId]) => operationId)]);

export const PRECISE_OPERATION_IDS: readonly string[] = [...IDENTITY_PRECISE_IDS, ...SELFBUILD_HTTP.map(([operationId]) => operationId)];

export type GroupName = "B02" | "B04" | "B07";

export type GroupPublication = {
  group: GroupName;
  state: "unbound" | "schema_frozen";
  openApi: string;
  contractCommit: string | null;
  ready: false;
  live: false;
  journey: false;
  release: false;
  note: string;
};

const UNPUBLISHED = "git 里没有已提交的 CONTRACT 或 OpenAPI。未跟踪工作区文件不能登记为 schema_frozen。";

export const GROUP_PUBLICATION: { readonly B02: GroupPublication; readonly B04: GroupPublication; readonly B07: GroupPublication } = {
  B02: {
    group: "B02",
    state: "unbound",
    openApi: "absent",
    contractCommit: null,
    ready: false,
    live: false,
    journey: false,
    release: false,
    note: UNPUBLISHED,
  },
  B04: {
    group: "B04",
    state: "schema_frozen",
    openApi: SELFBUILD_OPENAPI,
    contractCommit: SELFBUILD_COMMIT,
    ready: false,
    live: false,
    journey: false,
    release: false,
    note: "schema_frozen。合同 7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9。a373 交接写明 E56 未验收，生产者二进制未交付，身份产品读取未验收。origin 保持 null。本模块不发送请求。",
  },
  B07: {
    group: "B07",
    state: "unbound",
    openApi: "absent",
    contractCommit: null,
    ready: false,
    live: false,
    journey: false,
    release: false,
    note: UNPUBLISHED,
  },
};

export type RegisterInput = {
  group: GroupName;
  origin: string | null;
  ready: boolean;
  serviceCommit: string | null;
};

export type RuntimeRegistration = {
  group: GroupName;
  accepted: boolean;
  origin: null;
  ready: false;
  state: "unbound" | "schema_frozen";
  serviceCommit: string | null;
  live: false;
  journey: false;
  release: false;
  reason: "unpublished" | "schema_only" | "origin_refused" | "ready_refused" | "commit_mismatch";
};

export type CurrentCsrf =
  | { ok: true; headerName: "X-OO-CSRF"; token: string; revision: number }
  | { ok: false; reason: "not_current" };

export type PrepareRefusal = {
  ok: false;
  operationId: string;
  reason: "not_precise" | "csrf_required" | "schema_only" | "not_connected" | "registration_refused";
  headerName: "X-OO-CSRF" | null;
  csrfBound: boolean;
};

const CURRENT_KEYS = [
  "agent_session",
  "authority",
  "config_commit",
  "contract",
  "csrf_token",
  "customer_integration",
  "data_grants",
  "gateway_token",
  "harness_session",
  "install_state",
  "issuer",
  "operation",
  "owner_certification",
  "projects",
  "revision",
  "session",
  "session_class",
  "subject",
];

const SESSION_KEYS = ["absolute_expires_at", "idle_expires_at", "revoked"];
const PROJECT_KEYS = ["capabilities", "project_id", "role"];
const ERROR_KEYS = ["code", "message", "retryable"];
const IDENTITY_ERROR_KEYS = [
  "agent_session",
  "authority",
  "contract",
  "customer_integration",
  "data_grants",
  "error",
  "gateway_token",
  "harness_session",
  "owner_certification",
];
const SELFBUILD_ERROR_KEYS = ["active", "compiled_pack", "contract", "data_grants", "error", "management_identity", "published"];
const ROLES = new Set(["admin", "reviewer", "steward", "auditor", "business_owner"]);
const CAPABILITIES = new Set([
  "audit.read",
  "binding.read",
  "business.record",
  "identity.current",
  "ontology.draft",
  "project.admin",
  "review.decide",
  "session.revoke",
]);
const DATE_TIME = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/;

export function registerRuntime(input: RegisterInput): RuntimeRegistration {
  if (input.origin !== null) {
    return refused(input.group, "origin_refused");
  }
  if (input.ready) {
    return refused(input.group, "ready_refused");
  }
  const publication = GROUP_PUBLICATION[input.group];
  if (publication.state === "unbound") {
    return refused(input.group, "unpublished");
  }
  if (input.serviceCommit !== publication.contractCommit) {
    return refused(input.group, "commit_mismatch");
  }
  return {
    group: input.group,
    accepted: true,
    origin: null,
    ready: false,
    state: "schema_frozen",
    serviceCommit: publication.contractCommit,
    live: false,
    journey: false,
    release: false,
    reason: "schema_only",
  };
}

export function createRuntimeRegistry(): {
  register: (input: RegisterInput) => RuntimeRegistration;
  get: (group: GroupName) => RuntimeRegistration | null;
} {
  const entries = new Map<GroupName, RuntimeRegistration>();
  return {
    register(input) {
      const record = registerRuntime(input);
      entries.set(input.group, record);
      return record;
    },
    get(group) {
      return entries.get(group) ?? null;
    },
  };
}

export function csrfFromCurrent(value: unknown): CurrentCsrf {
  if (!isRecord(value) || !sameKeys(value, CURRENT_KEYS)) {
    return { ok: false, reason: "not_current" };
  }
  if (
    value.contract !== "oo.control-identity/v1" ||
    value.operation !== "getIdentityCurrent" ||
    value.authority !== "management_session" ||
    value.session_class !== "management_cookie" ||
    value.data_grants !== "none" ||
    value.owner_certification !== "not_issued" ||
    value.agent_session !== "not_issued" ||
    value.harness_session !== "not_issued" ||
    value.gateway_token !== "not_issued" ||
    value.customer_integration !== "not_claimed" ||
    (value.install_state !== "uninitialized" && value.install_state !== "initialized")
  ) {
    return { ok: false, reason: "not_current" };
  }
  if (typeof value.subject !== "string" || codePoints(value.subject) < 1 || codePoints(value.subject) > 256) {
    return { ok: false, reason: "not_current" };
  }
  if (typeof value.issuer !== "string" || codePoints(value.issuer) < 8) {
    return { ok: false, reason: "not_current" };
  }
  if (typeof value.config_commit !== "string" || !/^[0-9a-f]{64}$/.test(value.config_commit)) {
    return { ok: false, reason: "not_current" };
  }
  if (typeof value.revision !== "number" || !Number.isInteger(value.revision) || value.revision < 1 || value.revision > Number.MAX_SAFE_INTEGER) {
    return { ok: false, reason: "not_current" };
  }
  if (typeof value.csrf_token !== "string" || !/^[A-Za-z0-9_-]{43}$/.test(value.csrf_token)) {
    return { ok: false, reason: "not_current" };
  }
  if (!isRecord(value.session) || !sameKeys(value.session, SESSION_KEYS) || value.session.revoked !== false) {
    return { ok: false, reason: "not_current" };
  }
  if (typeof value.session.idle_expires_at !== "string" || !DATE_TIME.test(value.session.idle_expires_at)) {
    return { ok: false, reason: "not_current" };
  }
  if (typeof value.session.absolute_expires_at !== "string" || !DATE_TIME.test(value.session.absolute_expires_at)) {
    return { ok: false, reason: "not_current" };
  }
  if (!Array.isArray(value.projects) || value.projects.some((item) => !validProject(item))) {
    return { ok: false, reason: "not_current" };
  }
  return { ok: true, headerName: "X-OO-CSRF", token: value.csrf_token, revision: value.revision };
}

export function parseControlIdentityError(value: unknown): ParsedProductError<ControlIdentityErrorCode> | ProductErrorFailure {
  const parsed = parseErrorBody(value, IDENTITY_ERROR_KEYS, IDENTITY_ERROR_CODE_SET);
  if (!parsed) {
    return { ok: false, reason: "invalid_schema" };
  }
  if (
    valueContract(value) !== "oo.control-identity/v1" ||
    !isRecord(value) ||
    value.authority !== "management_session" ||
    value.data_grants !== "none" ||
    value.owner_certification !== "not_issued" ||
    value.agent_session !== "not_issued" ||
    value.harness_session !== "not_issued" ||
    value.gateway_token !== "not_issued" ||
    value.customer_integration !== "not_claimed"
  ) {
    return { ok: false, reason: "invalid_schema" };
  }
  return { ok: true, code: parsed.code as ControlIdentityErrorCode, message: parsed.message, retryable: parsed.retryable };
}

export function parseSelfbuildError(value: unknown): ParsedProductError<SelfbuildErrorCode> | ProductErrorFailure {
  const parsed = parseErrorBody(value, SELFBUILD_ERROR_KEYS, SELFBUILD_ERROR_CODE_SET);
  if (!parsed || !isRecord(value)) {
    return { ok: false, reason: "invalid_schema" };
  }
  if (
    value.contract !== "oo.selfbuild-control-api/v1" ||
    (value.management_identity !== "verified" && value.management_identity !== "not_verified") ||
    value.published !== false ||
    value.active !== false ||
    value.data_grants !== "none" ||
    value.compiled_pack !== "not_published"
  ) {
    return { ok: false, reason: "invalid_schema" };
  }
  return { ok: true, code: parsed.code as SelfbuildErrorCode, message: parsed.message, retryable: parsed.retryable };
}

/** Binds a precise operation to the registration and the current CSRF. It does not send the call. */
export function prepareCall(input: { operationId: string; registration: RuntimeRegistration | null; current: unknown }): PrepareRefusal {
  const operation = bindingById(input.operationId);
  if (!operation || !PRECISE_IDS.has(input.operationId) || operation.method === null) {
    return refusal(input.operationId, "not_precise", false);
  }
  const mutation = operation.method === "POST";
  if (operation.group === "selfbuild") {
    if (!input.registration || input.registration.group !== "B04" || !input.registration.accepted) {
      return refusal(input.operationId, "registration_refused", false);
    }
    if (mutation && !csrfFromCurrent(input.current).ok) {
      return refusal(input.operationId, "csrf_required", false);
    }
    return refusal(input.operationId, "schema_only", mutation);
  }
  if (operation.group === "control-identity") {
    if (mutation && !csrfFromCurrent(input.current).ok) {
      return refusal(input.operationId, "csrf_required", false);
    }
    return refusal(input.operationId, "not_connected", mutation);
  }
  return refusal(input.operationId, "not_precise", false);
}

function refused(group: GroupName, reason: RuntimeRegistration["reason"]): RuntimeRegistration {
  const publication = GROUP_PUBLICATION[group];
  return {
    group,
    accepted: false,
    origin: null,
    ready: false,
    state: publication.state,
    serviceCommit: null,
    live: false,
    journey: false,
    release: false,
    reason,
  };
}

function refusal(operationId: string, reason: PrepareRefusal["reason"], csrfBound: boolean): PrepareRefusal {
  return {
    ok: false,
    operationId,
    reason,
    headerName: csrfBound ? "X-OO-CSRF" : null,
    csrfBound,
  };
}

function parseErrorBody(
  value: unknown,
  topKeys: readonly string[],
  codes: ReadonlySet<string>,
): { code: string; message: string; retryable: boolean } | null {
  if (!isRecord(value) || !sameKeys(value, topKeys) || !isRecord(value.error) || !sameKeys(value.error, ERROR_KEYS)) {
    return null;
  }
  if (typeof value.error.code !== "string" || !codes.has(value.error.code)) {
    return null;
  }
  if (typeof value.error.message !== "string" || codePoints(value.error.message) < 1 || codePoints(value.error.message) > 160) {
    return null;
  }
  if (typeof value.error.retryable !== "boolean") {
    return null;
  }
  return { code: value.error.code, message: value.error.message, retryable: value.error.retryable };
}

function valueContract(value: unknown): unknown {
  return isRecord(value) ? value.contract : undefined;
}

function validProject(value: unknown): boolean {
  if (!isRecord(value) || !sameKeys(value, PROJECT_KEYS)) {
    return false;
  }
  if (typeof value.project_id !== "string" || !/^[a-z][a-z0-9_]{0,62}$/.test(value.project_id)) {
    return false;
  }
  if (typeof value.role !== "string" || !ROLES.has(value.role)) {
    return false;
  }
  return Array.isArray(value.capabilities) && value.capabilities.every((item) => typeof item === "string" && CAPABILITIES.has(item));
}

function sameKeys(value: Record<string, unknown>, expected: readonly string[]): boolean {
  const keys = Object.keys(value).sort();
  return keys.length === expected.length && keys.every((key, index) => key === expected[index]);
}

function codePoints(value: string): number {
  return Array.from(value).length;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
