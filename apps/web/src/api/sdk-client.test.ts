import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  CONTROL_IDENTITY_ERROR_CODES,
  GROUP_PUBLICATION,
  PRECISE_OPERATION_IDS,
  SELFBUILD_COMMIT,
  SELFBUILD_ERROR_CODES,
  SELFBUILD_HTTP,
  callOperation,
  clearSameOriginTransport,
  createRuntimeRegistry,
  csrfFromCurrent,
  invokeOperation,
  parseControlIdentityError,
  parseBuildList,
  parseLogoutResult,
  parseSelfbuildError,
  prepareCall,
  registerRuntime,
  registerSameOriginTransport,
} from "@open-ontology/sdk-ts";

const csrf = "a".repeat(43);

function current(overrides: Record<string, unknown> = {}) {
  return {
    contract: "oo.control-identity/v1",
    operation: "getIdentityCurrent",
    subject: "fixture-admin",
    issuer: "https://idp.example",
    config_commit: "ab".repeat(32),
    revision: 4,
    install_state: "initialized",
    projects: [{ project_id: "alpha", role: "admin", capabilities: ["identity.current", "project.admin"] }],
    csrf_token: csrf,
    session: { idle_expires_at: "2026-10-08T00:00:00Z", absolute_expires_at: "2026-10-08T01:00:00Z", revoked: false },
    authority: "management_session",
    session_class: "management_cookie",
    data_grants: "none",
    owner_certification: "not_issued",
    agent_session: "not_issued",
    harness_session: "not_issued",
    gateway_token: "not_issued",
    customer_integration: "not_claimed",
    ...overrides,
  };
}

function identityError(overrides: Record<string, unknown> = {}) {
  return {
    contract: "oo.control-identity/v1",
    error: { code: "csrf.rejected", message: "csrf", retryable: false },
    authority: "management_session",
    data_grants: "none",
    owner_certification: "not_issued",
    agent_session: "not_issued",
    harness_session: "not_issued",
    gateway_token: "not_issued",
    customer_integration: "not_claimed",
    ...overrides,
  };
}

function selfbuildError(overrides: Record<string, unknown> = {}) {
  return {
    contract: "oo.selfbuild-control-api/v1",
    error: { code: "dependency.not_ready", message: "waiting", retryable: true },
    management_identity: "not_verified",
    published: false,
    active: false,
    data_grants: "none",
    compiled_pack: "not_published",
    ...overrides,
  };
}

function operationIds(relativePath: string): string[] {
  const text = readFileSync(join(process.cwd(), relativePath), "utf8");
  return [...text.matchAll(/^ {6}operationId: ([A-Za-z0-9]+)$/gm)].map((match) => match[1] ?? "");
}

describe("legal client", () => {
  it("binds precise strings to the published OpenAPI and leaves B02 and B07 unpublished", () => {
    const identityIds = operationIds("../../tools/control-identity/openapi/control-identity.openapi.yaml");
    const selfbuildIds = operationIds("../../tools/selfbuild-control-api/openapi/selfbuild-control-api.openapi.yaml");
    expect(identityIds).toEqual(PRECISE_OPERATION_IDS.slice(0, 5));
    expect([...selfbuildIds].sort()).toEqual([...SELFBUILD_HTTP.map(([operationId]) => operationId)].sort());
    expect(new Set(selfbuildIds).size).toBe(SELFBUILD_HTTP.length);
    expect(PRECISE_OPERATION_IDS).toEqual([...identityIds, ...SELFBUILD_HTTP.map(([operationId]) => operationId)]);
    expect(GROUP_PUBLICATION.B02.openApi).toBe("absent");
    expect(GROUP_PUBLICATION.B02.state).toBe("unbound");
    expect(GROUP_PUBLICATION.B02.contractCommit).toBeNull();
    expect(GROUP_PUBLICATION.B02.ready).toBe(false);
    expect(GROUP_PUBLICATION.B07.openApi).toBe("absent");
    expect(GROUP_PUBLICATION.B07.ready).toBe(false);
    expect(GROUP_PUBLICATION.B04.state).toBe("schema_frozen");
    expect(GROUP_PUBLICATION.B04.contractCommit).toBe(SELFBUILD_COMMIT);
    expect(GROUP_PUBLICATION.B04.ready).toBe(false);
    expect(GROUP_PUBLICATION.B04.live).toBe(false);
    expect(GROUP_PUBLICATION.B04.journey).toBe(false);
    expect(GROUP_PUBLICATION.B04.release).toBe(false);
  });

  it("parses published error enums and rejects an imprecise body", () => {
    const identitySchema = JSON.parse(
      readFileSync(join(process.cwd(), "../../tools/control-identity/schema/error.schema.json"), "utf8"),
    ) as { properties: { error: { properties: { code: { enum: string[] } } } } };
    const selfbuildSchema = JSON.parse(
      readFileSync(join(process.cwd(), "../../tools/selfbuild-control-api/schema/error.schema.json"), "utf8"),
    ) as { properties: { error: { properties: { code: { enum: string[] } } } } };
    expect(identitySchema.properties.error.properties.code.enum).toEqual([...CONTROL_IDENTITY_ERROR_CODES]);
    expect(selfbuildSchema.properties.error.properties.code.enum).toEqual([...SELFBUILD_ERROR_CODES]);
    expect(createHash("sha256").update(readFileSync(join(process.cwd(), "../../tools/selfbuild-control-api/schema/error.schema.json"))).digest("hex")).toBe(
      "ab989b7de7c402f3bfe44a08e87053b296901ec172914ebc8d2a48518efd08fe",
    );
    expect(parseControlIdentityError(identityError()).ok).toBe(true);
    expect(parseSelfbuildError(selfbuildError())).toMatchObject({ ok: true, code: "dependency.not_ready", retryable: true });
    expect(parseControlIdentityError(identityError({ customer_integration: "verified" }))).toEqual({ ok: false, reason: "invalid_schema" });
    expect(parseSelfbuildError(selfbuildError({ published: true }))).toEqual({ ok: false, reason: "invalid_schema" });
    expect(parseSelfbuildError(selfbuildError({ extra: true }))).toEqual({ ok: false, reason: "invalid_schema" });
    expect(parseControlIdentityError(identityError({ error: { code: "OK", message: "x", retryable: false } }))).toEqual({
      ok: false,
      reason: "invalid_schema",
    });
    expect(parseSelfbuildError(selfbuildError({ error: { code: "malformed", message: "", retryable: false } }))).toEqual({
      ok: false,
      reason: "invalid_schema",
    });
    expect(parseSelfbuildError(selfbuildError({ error: { code: "malformed", message: "x".repeat(161), retryable: false } }))).toEqual({
      ok: false,
      reason: "invalid_schema",
    });
  });

  it("takes CSRF only from a current envelope and does not call fetch", () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const parsed = csrfFromCurrent(current());
    expect(parsed).toEqual({ ok: true, headerName: "X-OO-CSRF", token: csrf, revision: 4 });
    expect(csrfFromCurrent({ authorized: true, csrf_token: csrf })).toEqual({ ok: false, reason: "not_current" });
    expect(csrfFromCurrent(current({ csrf_token: "short" }))).toEqual({ ok: false, reason: "not_current" });
    expect(csrfFromCurrent(current({ revision: 1.5 }))).toEqual({ ok: false, reason: "not_current" });
    expect(csrfFromCurrent(current({ extra: true }))).toEqual({ ok: false, reason: "not_current" });

    const registry = createRuntimeRegistry();
    const accepted = registry.register({ group: "B04", origin: null, ready: false, serviceCommit: SELFBUILD_COMMIT });
    expect(accepted.accepted).toBe(true);
    expect(accepted.origin).toBeNull();
    expect(accepted.ready).toBe(false);
    expect(accepted.reason).toBe("schema_only");
    const built = prepareCall({ operationId: "createBuild", registration: accepted, current: current() });
    expect(built).toEqual({ ok: false, operationId: "createBuild", reason: "schema_only", headerName: "X-OO-CSRF", csrfBound: true });
    expect(JSON.stringify(built).includes(csrf)).toBe(false);
    expect(prepareCall({ operationId: "createBuild", registration: accepted, current: { authorized: true } })).toEqual({
      ok: false,
      operationId: "createBuild",
      reason: "csrf_required",
      headerName: null,
      csrfBound: false,
    });
    expect(prepareCall({ operationId: "listBuilds", registration: accepted, current: { authorized: true } })).toEqual({
      ok: false,
      operationId: "listBuilds",
      reason: "schema_only",
      headerName: null,
      csrfBound: false,
    });
    expect(prepareCall({ operationId: "logoutManagementSession", registration: null, current: current() })).toEqual({
      ok: false,
      operationId: "logoutManagementSession",
      reason: "not_connected",
      headerName: "X-OO-CSRF",
      csrfBound: true,
    });
    expect(prepareCall({ operationId: "getIdentityCurrent", registration: null, current: null })).toEqual({
      ok: false,
      operationId: "getIdentityCurrent",
      reason: "not_connected",
      headerName: null,
      csrfBound: false,
    });
    expect(prepareCall({ operationId: "B02.connectorMatrix", registration: null, current: current() }).reason).toBe("not_precise");
    expect(prepareCall({ operationId: "B04.builder", registration: accepted, current: current() }).reason).toBe("not_precise");
    expect(prepareCall({ operationId: "B07.secretWrite", registration: null, current: current() }).reason).toBe("not_precise");
    expect(fetchSpy).not.toHaveBeenCalled();
    fetchSpy.mockRestore();
  });

  it("refuses a runtime origin, a ready flag, and an unpublished group", () => {
    expect(registerRuntime({ group: "B04", origin: "https://edge.example", ready: false, serviceCommit: SELFBUILD_COMMIT })).toMatchObject({
      accepted: false,
      origin: null,
      ready: false,
      reason: "origin_refused",
    });
    expect(registerRuntime({ group: "B04", origin: null, ready: true, serviceCommit: SELFBUILD_COMMIT })).toMatchObject({
      accepted: false,
      reason: "ready_refused",
      serviceCommit: null,
    });
    expect(registerRuntime({ group: "B04", origin: null, ready: false, serviceCommit: "abc" })).toMatchObject({
      accepted: false,
      reason: "commit_mismatch",
    });
    const sources = registerRuntime({ group: "B02", origin: null, ready: false, serviceCommit: null });
    const secrets = registerRuntime({ group: "B07", origin: null, ready: false, serviceCommit: "7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9" });
    expect(sources).toMatchObject({ accepted: false, state: "unbound", reason: "unpublished", serviceCommit: null, live: false });
    expect(secrets).toMatchObject({ accepted: false, state: "unbound", reason: "unpublished", live: false, journey: false, release: false });
    expect(prepareCall({ operationId: "decideReviewItem", registration: sources, current: current() }).reason).toBe("registration_refused");
  });
});

function logoutBody() {
  return {
    contract: "oo.control-identity/v1",
    operation: "logoutManagementSession",
    outcome: "revoked",
    authority: "management_session",
    data_grants: "none",
    owner_certification: "not_issued",
    agent_session: "not_issued",
    harness_session: "not_issued",
    gateway_token: "not_issued",
    customer_integration: "not_claimed",
  };
}

describe("same-origin transport", () => {
  afterEach(() => {
    clearSameOriginTransport();
  });

  it("fetches a trusted https origin with one CSRF and one Origin", async () => {
    const seen: Array<{ url: string; init: RequestInit | undefined }> = [];
    const registered = registerSameOriginTransport({
      origin: "https://edge.example",
      pageOrigin: "https://edge.example",
      fetchImpl: async (input, init) => {
        seen.push({ url: String(input), init });
        return new Response(JSON.stringify(logoutBody()), { status: 200, headers: { "content-type": "application/json" } });
      },
    });
    expect(registered.ok).toBe(true);
    const result = await callOperation({ operationId: "logoutManagementSession", current: current() });
    expect(result).toMatchObject({
      ok: true,
      operationId: "logoutManagementSession",
      status: 200,
      fetched: true,
      live: false,
      journey: false,
      release: false,
      customerIntegration: "not_claimed",
    });
    expect(JSON.stringify(result).includes(csrf)).toBe(false);
    expect(seen).toHaveLength(1);
    expect(seen[0]?.url).toBe("https://edge.example/control/v1/auth/logout");
    const headers = new Headers(seen[0]?.init?.headers);
    expect(headers.get("origin")).toBe("https://edge.example");
    expect(headers.get("x-oo-csrf")).toBe(csrf);
    expect(seen[0]?.init?.method).toBe("POST");
    expect(prepareCall({ operationId: "logoutManagementSession", registration: null, current: current() }).reason).toBe("not_connected");
    expect(invokeOperation("getIdentityCurrent")).toEqual({ ok: false, reason: "not_connected", operationId: "getIdentityCurrent" });
    expect(seen).toHaveLength(1);
  });

  it("rejects http, a destroyed host, a mismatched page, and a bad success body", async () => {
    const fetchImpl = vi.fn(async () => new Response("nope", { status: 200 }));
    expect(registerSameOriginTransport({ origin: "http://edge.example", pageOrigin: "http://edge.example", fetchImpl })).toEqual({
      ok: false,
      reason: "origin_rejected",
    });
    expect(registerSameOriginTransport({ origin: "https://oo-fe01-m9-edge:8443", pageOrigin: "https://oo-fe01-m9-edge:8443", fetchImpl })).toEqual({
      ok: false,
      reason: "origin_rejected",
    });
    expect(registerSameOriginTransport({ origin: "https://oo-fe01-m12-edge:8443", pageOrigin: "https://oo-fe01-m12-edge:8443", fetchImpl })).toEqual({
      ok: false,
      reason: "origin_rejected",
    });
    expect(registerSameOriginTransport({ origin: "https://oo-fe01-m13-edge:8443", pageOrigin: "https://oo-fe01-m13-edge:8443", fetchImpl })).toEqual({
      ok: false,
      reason: "origin_rejected",
    });
    expect(registerSameOriginTransport({ origin: "https://edge.example/app", pageOrigin: "https://edge.example/app", fetchImpl })).toEqual({
      ok: false,
      reason: "origin_rejected",
    });
    expect(registerSameOriginTransport({ origin: "https://edge.example", pageOrigin: "https://other.example", fetchImpl })).toEqual({
      ok: false,
      reason: "origin_rejected",
    });
    expect(await callOperation({ operationId: "getIdentityCurrent" })).toMatchObject({ ok: false, reason: "origin_rejected", fetched: false });
    expect(fetchImpl).not.toHaveBeenCalled();

    registerSameOriginTransport({ origin: "https://edge.example", pageOrigin: "https://edge.example" });
    const pageFetch = vi.spyOn(globalThis, "fetch");
    const blocked = await callOperation({ operationId: "getIdentityCurrent" });
    expect(blocked).toMatchObject({ ok: false, reason: "origin_rejected", fetched: false });
    expect(pageFetch).not.toHaveBeenCalled();
    pageFetch.mockRestore();

    const bad = registerSameOriginTransport({
      origin: "https://edge.example",
      pageOrigin: "https://edge.example",
      fetchImpl: async () => new Response("{", { status: 200, headers: { "content-type": "application/json" } }),
    });
    expect(bad.ok).toBe(true);
    const invalid = await callOperation({ operationId: "getIdentityCurrent" });
    expect(invalid).toMatchObject({ ok: false, reason: "invalid_schema", fetched: true, status: 200, live: false });
    expect(parseLogoutResult({ outcome: "revoked" })).toEqual({ ok: false, reason: "invalid_schema" });
  });

  it("parses a fetched selfbuild error instead of staying not_connected", async () => {
    const fetchImpl = vi.fn(async () => new Response(JSON.stringify(selfbuildError()), { status: 503, headers: { "content-type": "application/json" } }));
    registerSameOriginTransport({ origin: "https://edge.example", pageOrigin: "https://edge.example", fetchImpl });
    const result = await callOperation({ operationId: "listBuilds" });
    expect(fetchImpl).toHaveBeenCalledTimes(1);
    expect(result).toMatchObject({
      ok: false,
      operationId: "listBuilds",
      reason: "http_error",
      status: 503,
      errorCode: "dependency.not_ready",
      fetched: true,
      live: false,
      journey: false,
      release: false,
      customerIntegration: "not_claimed",
    });
    expect(prepareCall({ operationId: "listBuilds", registration: null, current: null }).reason).toBe("registration_refused");
  });

  it("accepts an empty published listBuilds document and still refuses the registry call", async () => {
    const body = {
      contract: "oo.selfbuild-control-api/v1",
      operation: "listBuilds",
      builds: [],
      management_identity: "verified",
      published: false,
      active: false,
      data_grants: "none",
      compiled_pack: "not_published",
    };
    const fetchImpl = vi.fn(async () => new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } }));
    registerSameOriginTransport({ origin: "https://edge.example", pageOrigin: "https://edge.example", fetchImpl });
    const result = await callOperation({ operationId: "listBuilds", query: { project_id: "alpha" } });
    expect(fetchImpl).toHaveBeenCalledTimes(1);
    expect(result).toMatchObject({
      ok: true,
      operationId: "listBuilds",
      status: 200,
      fetched: true,
      live: false,
      journey: false,
      release: false,
      customerIntegration: "not_claimed",
    });
    expect(result.ok ? result.body : null).toMatchObject({ builds: [], published: false, compiled_pack: "not_published" });
    expect(parseBuildList(body)).toEqual({ ok: true, count: 0 });
    expect(parseBuildList({ ...body, leaked: true })).toEqual({ ok: false, reason: "invalid_schema" });
    expect(invokeOperation("listBuilds")).toMatchObject({ ok: false, reason: "schema_only" });
    const accepted = createRuntimeRegistry().register({ group: "B04", origin: null, ready: false, serviceCommit: SELFBUILD_COMMIT });
    expect(prepareCall({ operationId: "listBuilds", registration: accepted, current: null }).reason).toBe("schema_only");
  });
});
