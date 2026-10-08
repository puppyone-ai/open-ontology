/**
 * Legal client for published operation strings.
 * B04 is schema_frozen at the 7bb contract. B02 and B07 have no committed OpenAPI.
 * CSRF comes only from a typed getIdentityCurrent body.
 * invokeOperation and prepareCall do not call fetch.
 * callOperation fetches only after a deployer registers a trusted same-origin transport.
 * A listBuilds 200 is accepted only when it matches the published list document.
 * Other selfbuild successes stay invalid_schema until their published documents are parsed.
 * invokeOperation remains schema_only for every B04 id.
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
    note: "schema_frozen。合同 7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9。a373 交接写明 E56 未验收，生产者二进制未交付，身份产品读取未验收。登记 origin 保持 null。callOperation 可以发送请求；没有验收过的成功响应不会被当成构建完成。ready、live、journey、release 都是 false。",
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

const BUILD_LIST_REQUIRED = [
  "active",
  "builds",
  "compiled_pack",
  "contract",
  "data_grants",
  "management_identity",
  "operation",
  "published",
] as const;

const BUILD_ITEM_REQUIRED = [
  "active",
  "build_id",
  "compiled_pack",
  "contract",
  "data_grants",
  "identity_revision",
  "input_sha256",
  "management_identity",
  "operation",
  "project_id",
  "published",
  "stage",
  "stages",
  "status",
] as const;

const BUILD_OPERATIONS = new Set(["createBuild", "getBuild", "listBuilds", "cancelBuild"]);
const BUILD_STATUSES = new Set(["queued", "running", "succeeded", "failed", "cancel_requested", "canceled"]);
const STAGE_NAMES = new Set(["S0", "S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9", "S10"]);
const STAGE_STATUSES = new Set(["pending", "running", "succeeded", "failed", "refused", "awaiting"]);

export type BuildListParse = { ok: true; count: number } | { ok: false; reason: "invalid_schema" };

/** Accepts the published listBuilds object. An empty builds array is a list, not a completed build. */
export function parseBuildList(value: unknown): BuildListParse {
  if (!isRecord(value) || !keysAllowed(value, BUILD_LIST_REQUIRED, ["next_cursor"])) {
    return { ok: false, reason: "invalid_schema" };
  }
  if (
    value.contract !== "oo.selfbuild-control-api/v1" ||
    value.operation !== "listBuilds" ||
    value.management_identity !== "verified" ||
    value.published !== false ||
    value.active !== false ||
    value.data_grants !== "none" ||
    value.compiled_pack !== "not_published" ||
    !Array.isArray(value.builds)
  ) {
    return { ok: false, reason: "invalid_schema" };
  }
  if ("next_cursor" in value && (typeof value.next_cursor !== "string" || value.next_cursor.length < 1 || value.next_cursor.length > 256)) {
    return { ok: false, reason: "invalid_schema" };
  }
  if (value.builds.some((item) => !validBuildItem(item))) {
    return { ok: false, reason: "invalid_schema" };
  }
  return { ok: true, count: value.builds.length };
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

const DESTROYED_TRANSPORT_HOSTS = new Set([
  "oo-fe01-edge",
  "oo-fe01-proof",
  "oo-fe01-idp",
  "oo-fe01-pg",
  "oo-fe01-m7-edge",
  "oo-fe01-m7-proof",
  "oo-fe01-m7-idp",
  "oo-fe01-m7-pg",
  "oo-fe01-m9-edge",
  "oo-fe01-m9-proof",
  "oo-fe01-m9-idp",
  "oo-fe01-m9-pg",
  "oo-fe01-m12-edge",
  "oo-fe01-m12-proof",
  "oo-fe01-m12-idp",
  "oo-fe01-m12-pg",
  "oo-fe01-m13-edge",
  "oo-fe01-m13-proof",
  "oo-fe01-m13-idp",
  "oo-fe01-m13-pg",
  "oo-fe01-m13-b04",
  "oo-fe01-m14-edge",
  "oo-fe01-m14-proof",
  "oo-fe01-m14-idp",
  "oo-fe01-m14-pg",
  "oo-fe01-m14-b04",
]);

const CALL_FLAGS = {
  live: false,
  journey: false,
  release: false,
  customerIntegration: "not_claimed",
} as const;

const LOGOUT_KEYS = [
  "agent_session",
  "authority",
  "contract",
  "customer_integration",
  "data_grants",
  "gateway_token",
  "harness_session",
  "operation",
  "outcome",
  "owner_certification",
];

const BOOTSTRAP_KEYS = [
  "agent_session",
  "contract",
  "customer_integration",
  "customer_owner",
  "data_grants",
  "fixture_class",
  "gateway_token",
  "harness_session",
  "install_state",
  "operation",
  "outcome",
  "owner_certification",
];

const QUERY_KEYS: Readonly<Record<string, readonly string[]>> = {
  startAuthorizationCodeLogin: ["return_to"],
  completeAuthorizationCodeCallback: ["code", "state", "error"],
  listBuilds: ["project_id", "limit", "cursor"],
  listReviewItems: ["project_id", "build_id"],
  getReviewItem: ["project_id", "build_id"],
  listClassifications: ["project_id"],
};

export type SameOriginTransport = {
  origin: string;
  pageOrigin: string;
  live: false;
  journey: false;
  release: false;
  customerIntegration: "not_claimed";
};

export type TransportRegistration = { ok: true; transport: SameOriginTransport } | { ok: false; reason: "origin_rejected" };

export type CallSuccess = {
  ok: true;
  operationId: string;
  status: number;
  body: unknown;
  location: string | null;
  fetched: true;
  live: false;
  journey: false;
  release: false;
  customerIntegration: "not_claimed";
};

export type CallFailure = {
  ok: false;
  operationId: string;
  reason: "origin_rejected" | "not_precise" | "csrf_required" | "invalid_schema" | "http_error" | "transport_failed";
  status: number | null;
  errorCode: string | null;
  fetched: boolean;
  live: false;
  journey: false;
  release: false;
  customerIntegration: "not_claimed";
};

export type CallResult = CallSuccess | CallFailure;

export type LogoutParse = { ok: true; outcome: "revoked"; customerIntegration: "not_claimed" } | { ok: false; reason: "invalid_schema" };

export type BootstrapParse =
  | {
      ok: true;
      outcome: "consumed";
      installState: "initialized";
      fixtureClass: "explicit_test_fixture";
      customerOwner: false;
      customerIntegration: "not_claimed";
    }
  | { ok: false; reason: "invalid_schema" };

type ActiveTransport = {
  origin: string;
  pageOrigin: string;
  fetchImpl: typeof fetch | null;
};

let activeTransport: ActiveTransport | null = null;

export function registerSameOriginTransport(input: { origin: string; pageOrigin: string; fetchImpl?: typeof fetch }): TransportRegistration {
  const origin = httpsOrigin(input.origin);
  const pageOrigin = httpsOrigin(input.pageOrigin);
  if (!origin || !pageOrigin || origin !== pageOrigin) {
    return { ok: false, reason: "origin_rejected" };
  }
  activeTransport = { origin, pageOrigin, fetchImpl: input.fetchImpl ?? null };
  return { ok: true, transport: publicTransport(activeTransport) };
}

export function clearSameOriginTransport(): void {
  activeTransport = null;
}

export function registeredTransport(): SameOriginTransport | null {
  return activeTransport ? publicTransport(activeTransport) : null;
}

export function parseLogoutResult(value: unknown): LogoutParse {
  if (!isRecord(value) || !sameKeys(value, LOGOUT_KEYS)) {
    return { ok: false, reason: "invalid_schema" };
  }
  if (
    value.contract !== "oo.control-identity/v1" ||
    value.operation !== "logoutManagementSession" ||
    value.outcome !== "revoked" ||
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
  return { ok: true, outcome: "revoked", customerIntegration: "not_claimed" };
}

export function parseBootstrapResult(value: unknown): BootstrapParse {
  if (!isRecord(value) || !sameKeys(value, BOOTSTRAP_KEYS)) {
    return { ok: false, reason: "invalid_schema" };
  }
  if (
    value.contract !== "oo.control-identity/v1" ||
    value.operation !== "consumeBootstrapToken" ||
    value.outcome !== "consumed" ||
    value.install_state !== "initialized" ||
    value.fixture_class !== "explicit_test_fixture" ||
    value.customer_owner !== false ||
    value.owner_certification !== "not_issued" ||
    value.data_grants !== "none" ||
    value.agent_session !== "not_issued" ||
    value.harness_session !== "not_issued" ||
    value.gateway_token !== "not_issued" ||
    value.customer_integration !== "not_claimed"
  ) {
    return { ok: false, reason: "invalid_schema" };
  }
  return {
    ok: true,
    outcome: "consumed",
    installState: "initialized",
    fixtureClass: "explicit_test_fixture",
    customerOwner: false,
    customerIntegration: "not_claimed",
  };
}

/** Sends one published operation through the registered same-origin transport. */
export async function callOperation(input: {
  operationId: string;
  query?: Record<string, string>;
  params?: Record<string, string>;
  body?: unknown;
  current?: unknown;
}): Promise<CallResult> {
  const operation = bindingById(input.operationId);
  if (!operation || !PRECISE_IDS.has(input.operationId) || operation.method === null || operation.path === null) {
    return callFailure(input.operationId, "not_precise", null, null, false);
  }
  if (!activeTransport) {
    return callFailure(input.operationId, "origin_rejected", null, null, false);
  }
  const transport = activeTransport;
  const inBrowser = transport.fetchImpl === null && typeof window !== "undefined" && typeof window.location?.origin === "string";
  if (inBrowser && window.location.origin !== transport.origin) {
    return callFailure(input.operationId, "origin_rejected", null, null, false);
  }
  const path = fillPath(operation.path, input.params);
  if (!path) {
    return callFailure(input.operationId, "not_precise", null, null, false);
  }
  const query = checkedQuery(input.operationId, input.query);
  if (!query.ok) {
    return callFailure(input.operationId, query.reason, null, null, false);
  }
  const mutation = operation.method === "POST";
  const csrf = mutation ? csrfFromCurrent(input.current) : null;
  if (mutation && (!csrf || !csrf.ok)) {
    return callFailure(input.operationId, "csrf_required", null, null, false);
  }
  if (input.operationId === "consumeBootstrapToken" && !validBootstrapBody(input.body)) {
    return callFailure(input.operationId, "invalid_schema", null, null, false);
  }
  if (input.operationId === "logoutManagementSession" && input.body !== undefined) {
    return callFailure(input.operationId, "invalid_schema", null, null, false);
  }
  if (!mutation && input.body !== undefined) {
    return callFailure(input.operationId, "invalid_schema", null, null, false);
  }
  const url = `${inBrowser ? "" : transport.origin}${path}${queryString(query.query)}`;
  const headers = new Headers();
  headers.set("accept", "application/json");
  if (input.body !== undefined) {
    headers.set("content-type", "application/json");
  }
  if (csrf && csrf.ok) {
    headers.set("x-oo-csrf", csrf.token);
  }
  if (!inBrowser) {
    headers.set("origin", transport.origin);
  }
  const fetchFn = transport.fetchImpl ?? globalThis.fetch;
  let response: Response;
  try {
    response = await fetchFn(url, {
      method: operation.method,
      headers,
      body: input.body === undefined ? undefined : JSON.stringify(input.body),
      credentials: "include",
      redirect: "manual",
    });
  } catch {
    return callFailure(input.operationId, "transport_failed", null, null, false);
  }
  return classifyResponse(input.operationId, operation.group, response);
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

function keysAllowed(value: Record<string, unknown>, required: readonly string[], optional: readonly string[]): boolean {
  const keys = Object.keys(value);
  const allowed = new Set<string>([...required, ...optional]);
  return required.every((key) => keys.includes(key)) && keys.every((key) => allowed.has(key));
}

function validBuildItem(value: unknown): boolean {
  if (!isRecord(value) || !keysAllowed(value, BUILD_ITEM_REQUIRED, ["failure_code"])) {
    return false;
  }
  if (
    value.contract !== "oo.selfbuild-control-api/v1" ||
    typeof value.operation !== "string" ||
    !BUILD_OPERATIONS.has(value.operation) ||
    typeof value.build_id !== "string" ||
    !/^b_[0-9a-f]{32}$/.test(value.build_id) ||
    typeof value.project_id !== "string" ||
    !/^[a-z][a-z0-9_]{0,62}$/.test(value.project_id) ||
    typeof value.status !== "string" ||
    !BUILD_STATUSES.has(value.status) ||
    typeof value.stage !== "string" ||
    !STAGE_NAMES.has(value.stage) ||
    !Number.isSafeInteger(value.identity_revision) ||
    (value.identity_revision as number) < 1 ||
    typeof value.input_sha256 !== "string" ||
    !/^[0-9a-f]{64}$/.test(value.input_sha256) ||
    value.management_identity !== "verified" ||
    value.published !== false ||
    value.active !== false ||
    value.data_grants !== "none" ||
    value.compiled_pack !== "not_published" ||
    !Array.isArray(value.stages)
  ) {
    return false;
  }
  if ("failure_code" in value && value.failure_code !== null && typeof value.failure_code !== "string") {
    return false;
  }
  return value.stages.every((stage) => validStage(stage));
}

function validStage(value: unknown): boolean {
  if (!isRecord(value) || !keysAllowed(value, ["stage", "status"], ["output_sha256", "detail_code"])) {
    return false;
  }
  if (typeof value.stage !== "string" || !STAGE_NAMES.has(value.stage) || typeof value.status !== "string" || !STAGE_STATUSES.has(value.status)) {
    return false;
  }
  if ("output_sha256" in value && value.output_sha256 !== null && (typeof value.output_sha256 !== "string" || !/^[0-9a-f]{64}$/.test(value.output_sha256))) {
    return false;
  }
  return !("detail_code" in value) || value.detail_code === null || typeof value.detail_code === "string";
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

function publicTransport(active: ActiveTransport): SameOriginTransport {
  return { origin: active.origin, pageOrigin: active.pageOrigin, ...CALL_FLAGS };
}

function httpsOrigin(value: string): string | null {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return null;
  }
  if (url.protocol !== "https:" || url.username !== "" || url.password !== "") {
    return null;
  }
  if ((url.pathname !== "/" && url.pathname !== "") || url.search !== "" || url.hash !== "") {
    return null;
  }
  if (url.hostname.length === 0 || DESTROYED_TRANSPORT_HOSTS.has(url.hostname)) {
    return null;
  }
  return url.origin;
}

function fillPath(template: string, params: Record<string, string> | undefined): string | null {
  const names = [...template.matchAll(/\{([A-Za-z0-9_]+)\}/g)].map((match) => match[1] ?? "");
  let path = template;
  for (const name of names) {
    const value = params?.[name];
    if (!value || value.includes("/") || value.includes("?") || value.includes("#")) {
      return null;
    }
    path = path.replace(`{${name}}`, encodeURIComponent(value));
  }
  return path.includes("{") || path.includes("}") ? null : path;
}

function checkedQuery(
  operationId: string,
  query: Record<string, string> | undefined,
): { ok: true; query: Record<string, string> } | { ok: false; reason: "not_precise" | "invalid_schema" } {
  const entries = Object.entries(query ?? {});
  if (entries.some(([key]) => key === "bootstrap_token")) {
    return { ok: false, reason: "not_precise" };
  }
  const allowed = QUERY_KEYS[operationId];
  if (!allowed) {
    return entries.length === 0 ? { ok: true, query: {} } : { ok: false, reason: "not_precise" };
  }
  const next: Record<string, string> = {};
  for (const [key, value] of entries) {
    if (!allowed.includes(key)) {
      return { ok: false, reason: "not_precise" };
    }
    if (key === "return_to" && !validReturnTo(value)) {
      return { ok: false, reason: "invalid_schema" };
    }
    if ((key === "code" || key === "state" || key === "error") && !validQueryToken(value)) {
      return { ok: false, reason: "invalid_schema" };
    }
    if (key === "project_id" && !/^[a-z][a-z0-9_]{0,62}$/.test(value)) {
      return { ok: false, reason: "invalid_schema" };
    }
    if (key === "build_id" && !/^b_[0-9a-f]{32}$/.test(value)) {
      return { ok: false, reason: "invalid_schema" };
    }
    if (key === "limit" && !/^(?:[1-9]|[1-4][0-9]|50)$/.test(value)) {
      return { ok: false, reason: "invalid_schema" };
    }
    if (key === "cursor" && (value.length < 1 || value.length > 256)) {
      return { ok: false, reason: "invalid_schema" };
    }
    next[key] = value;
  }
  return { ok: true, query: next };
}

function validReturnTo(value: string): boolean {
  if (value.length < 1 || value.length > 128 || !value.startsWith("/")) {
    return false;
  }
  if (value.includes("//") || value.includes("\\") || value.includes("?") || value.includes("#") || value.includes("..")) {
    return false;
  }
  return [...value].every((char) => /[A-Za-z0-9/._-]/.test(char));
}

function validQueryToken(value: string): boolean {
  return value.length > 0 && value.length <= 4096 && !/\s/.test(value);
}

function validBootstrapBody(value: unknown): boolean {
  return isRecord(value) && sameKeys(value, ["token"]) && typeof value.token === "string" && /^[A-Za-z0-9_-]{43}$/.test(value.token);
}

function queryString(query: Record<string, string>): string {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    params.append(key, value);
  }
  const text = params.toString();
  return text.length === 0 ? "" : `?${text}`;
}

function callFailure(
  operationId: string,
  reason: CallFailure["reason"],
  status: number | null,
  errorCode: string | null,
  fetched: boolean,
): CallFailure {
  return { ok: false, operationId, reason, status, errorCode, fetched, ...CALL_FLAGS };
}

function callSuccess(operationId: string, status: number, body: unknown, location: string | null): CallSuccess {
  return { ok: true, operationId, status, body, location, fetched: true, ...CALL_FLAGS };
}

function acceptedSelfbuild(operationId: string, status: number, value: unknown): boolean {
  if (!isRecord(value) || value.contract !== "oo.selfbuild-control-api/v1" || value.management_identity !== "verified" || value.published !== false || value.data_grants !== "none") {
    return false;
  }
  if (status === 200 && operationId === "listBuilds") {
    return parseBuildList(value).ok;
  }
  if ((operationId === "createBuild" || operationId === "cancelBuild") && status === 202) {
    return validBuildItem(value) && value.operation === operationId;
  }
  if (operationId === "getBuild" && status === 200) {
    return validBuildItem(value) && value.operation === "getBuild";
  }
  if (status !== 200) {
    return false;
  }
  if (operationId === "getBuildCards") {
    return keysAllowed(value, ["build_id", "contract", "data_grants", "management_identity", "operation", "published", "sample_policy"], ["cards"])
      && value.operation === "getBuildCards"
      && value.sample_policy === "none"
      && typeof value.build_id === "string"
      && /^b_[0-9a-f]{32}$/.test(value.build_id)
      && (!("cards" in value) || isRecord(value.cards));
  }
  if (operationId === "getBuildCandidates") {
    return keysAllowed(value, ["build_id", "contract", "data_grants", "management_identity", "operation", "published", "suggestions"], ["generation"])
      && value.operation === "getBuildCandidates"
      && typeof value.build_id === "string"
      && /^b_[0-9a-f]{32}$/.test(value.build_id)
      && isRecord(value.suggestions)
      && (!("generation" in value) || isRecord(value.generation));
  }
  if (operationId === "listReviewItems") {
    return keysAllowed(value, ["contract", "data_grants", "items", "management_identity", "operation", "published"], [])
      && value.operation === "listReviewItems"
      && Array.isArray(value.items)
      && value.items.every((item) => validReviewItem(item));
  }
  if (operationId === "getReviewItem") {
    return validReviewItem(value);
  }
  if (operationId === "previewReviewBatch" || operationId === "previewClassifications") {
    return keysAllowed(value, ["contract", "data_grants", "items", "management_identity", "operation", "published", "wrote"], [])
      && value.operation === operationId
      && value.wrote === false
      && Array.isArray(value.items)
      && value.items.every((item) => isRecord(item));
  }
  if (operationId === "listClassifications") {
    return keysAllowed(value, ["columns", "contract", "data_grants", "management_identity", "operation", "published"], [])
      && value.operation === "listClassifications"
      && Array.isArray(value.columns)
      && value.columns.every((item) => isRecord(item));
  }
  if (operationId === "decideReviewItem" || operationId === "confirmClassifications") {
    return keysAllowed(value, ["code", "contract", "data_grants", "grants", "ledger_authenticated", "ledger_management_identity", "management_identity", "operation", "published"], ["revision"])
      && value.operation === operationId
      && typeof value.code === "string"
      && value.code.length > 0
      && value.code.length <= 64
      && value.ledger_authenticated === false
      && value.ledger_management_identity === "not_claimed"
      && value.grants === "not_authorized"
      && (!("revision" in value) || (Number.isSafeInteger(value.revision) && (value.revision as number) >= 1));
  }
  if (operationId === "compileBuildPack" || operationId === "getBuildPack") {
    return keysAllowed(value, ["active", "build_id", "compiled_pack", "contract", "data_grants", "management_identity", "operation", "output_sha256", "producer", "producer_authenticated", "published"], [])
      && value.operation === operationId
      && typeof value.build_id === "string"
      && /^b_[0-9a-f]{32}$/.test(value.build_id)
      && typeof value.output_sha256 === "string"
      && /^[0-9a-f]{64}$/.test(value.output_sha256)
      && isRecord(value.producer)
      && value.producer_authenticated === false
      && value.active === false
      && value.compiled_pack === "not_published";
  }
  return false;
}

function validReviewItem(value: unknown): boolean {
  if (!isRecord(value) || !keysAllowed(value, ["candidate_sha256", "contract", "data_grants", "item_id", "ledger_authenticated", "management_identity", "operation", "project_id", "published", "source_sha256"], ["build_id", "decision", "evidence_sha256", "schema_sha256"])) {
    return false;
  }
  return value.contract === "oo.selfbuild-control-api/v1"
    && value.operation === "getReviewItem"
    && typeof value.item_id === "string"
    && value.item_id.length > 0
    && typeof value.project_id === "string"
    && /^[a-z][a-z0-9_]{0,62}$/.test(value.project_id)
    && typeof value.candidate_sha256 === "string"
    && /^[0-9a-f]{64}$/.test(value.candidate_sha256)
    && typeof value.source_sha256 === "string"
    && /^[0-9a-f]{64}$/.test(value.source_sha256)
    && value.management_identity === "verified"
    && value.ledger_authenticated === false
    && value.published === false
    && value.data_grants === "none"
    && (!("build_id" in value) || (typeof value.build_id === "string" && /^b_[0-9a-f]{32}$/.test(value.build_id)))
    && (!("schema_sha256" in value) || typeof value.schema_sha256 === "string")
    && (!("evidence_sha256" in value) || typeof value.evidence_sha256 === "string")
    && (!("decision" in value) || value.decision === null || isRecord(value.decision));
}

async function classifyResponse(operationId: string, group: string, response: Response): Promise<CallResult> {
  const location = response.headers.get("location");
  const text = await response.text();
  if ((operationId === "startAuthorizationCodeLogin" || operationId === "completeAuthorizationCodeCallback") && response.status === 302 && location) {
    return callSuccess(operationId, 302, null, location);
  }
  const parsed = parseJsonText(text);
  if (!parsed.ok) {
    return callFailure(operationId, "invalid_schema", response.status, null, true);
  }
  if (response.status === 200 && operationId === "getIdentityCurrent" && csrfFromCurrent(parsed.value).ok) {
    return callSuccess(operationId, 200, parsed.value, location);
  }
  if (response.status === 200 && operationId === "logoutManagementSession" && parseLogoutResult(parsed.value).ok) {
    return callSuccess(operationId, 200, parsed.value, location);
  }
  if (response.status === 200 && operationId === "consumeBootstrapToken" && parseBootstrapResult(parsed.value).ok) {
    return callSuccess(operationId, 200, parsed.value, location);
  }
  if (acceptedSelfbuild(operationId, response.status, parsed.value)) {
    return callSuccess(operationId, response.status, parsed.value, location);
  }
  const error = group === "selfbuild" ? parseSelfbuildError(parsed.value) : parseControlIdentityError(parsed.value);
  if (error.ok) {
    return callFailure(operationId, "http_error", response.status, error.code, true);
  }
  return callFailure(operationId, "invalid_schema", response.status, null, true);
}

function parseJsonText(text: string): { ok: true; value: unknown } | { ok: false } {
  if (text.length === 0) {
    return { ok: true, value: null };
  }
  try {
    return { ok: true, value: JSON.parse(text) as unknown };
  } catch {
    return { ok: false };
  }
}
