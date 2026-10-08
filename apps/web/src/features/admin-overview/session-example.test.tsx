import { fireEvent, render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import { callOperation, clearSameOriginTransport, registerSameOriginTransport } from "@open-ontology/sdk-ts";
import { IdentityProvider } from "../../app/identity-context";
import { appRoutes } from "../../app/router";

const CSRF = "a".repeat(43);
const TOKEN = "b".repeat(43);
const ORIGIN = "https://admin.example";

function envelope(overrides: Record<string, unknown> = {}) {
  return {
    contract: "oo.control-identity/v1",
    operation: "getIdentityCurrent",
    subject: "fixture-admin",
    issuer: "https://idp.example",
    config_commit: "ab".repeat(32),
    revision: 4,
    install_state: "initialized",
    projects: [
      {
        project_id: "alpha",
        role: "admin",
        capabilities: ["binding.read", "identity.current", "project.admin", "session.revoke"],
      },
    ],
    csrf_token: CSRF,
    session: {
      idle_expires_at: "2026-10-08T00:00:00Z",
      absolute_expires_at: "2026-10-08T01:00:00Z",
      revoked: false,
    },
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

function requestUrl(input: RequestInfo | URL): string {
  if (typeof input === "string") return input;
  if (input instanceof URL) return input.toString();
  return input.url;
}

function headerValue(headers: HeadersInit | undefined, name: string): string | null {
  return new Headers(headers).get(name);
}

describe("admin session example", () => {
  afterEach(() => {
    clearSameOriginTransport();
    vi.unstubAllGlobals();
  });

  it("initializes, shows the current admin, and logs out through the service", async () => {
    const seen: string[] = [];
    let phase: "new" | "ready" | "gone" = "new";
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = requestUrl(input);
        const method = init?.method ?? "GET";
        seen.push(`${method} ${url}`);
        if (url === "/control/v1/identity/current") {
          if (phase === "new") {
            return new Response(JSON.stringify(envelope({ install_state: "uninitialized", projects: [] })), {
              status: 200,
              headers: { "content-type": "application/json" },
            });
          }
          if (phase === "ready") {
            return new Response(JSON.stringify(envelope()), { status: 200, headers: { "content-type": "application/json" } });
          }
          return new Response(null, { status: 401 });
        }
        if (url === "/control/v1/auth/bootstrap/consume") {
          expect(method).toBe("POST");
          expect(headerValue(init?.headers, "x-oo-csrf")).toBe(CSRF);
          expect(init?.body).toBe(JSON.stringify({ token: TOKEN }));
          phase = "ready";
          return new Response(
            JSON.stringify({
              outcome: "consumed",
              install_state: "initialized",
              customer_integration: "not_claimed",
              customer_owner: false,
              fixture_class: "explicit_test_fixture",
            }),
            { status: 200, headers: { "content-type": "application/json" } },
          );
        }
        if (url === "/control/v1/auth/logout") {
          expect(method).toBe("POST");
          expect(headerValue(init?.headers, "x-oo-csrf")).toBe(CSRF);
          phase = "gone";
          return new Response(JSON.stringify({ outcome: "revoked", customer_integration: "not_claimed" }), {
            status: 200,
            headers: { "content-type": "application/json" },
          });
        }
        throw new Error(`unexpected ${method} ${url}`);
      }),
    );
    const router = createMemoryRouter(appRoutes, { initialEntries: ["/admin"] });
    render(
      <IdentityProvider>
        <RouterProvider router={router} />
      </IdentityProvider>,
    );
    const tokenInput = await screen.findByLabelText("引导令牌");
    expect(screen.getByRole("heading", { name: "初始化" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "提交初始化" })).toBeTruthy();
    fireEvent.change(tokenInput, { target: { value: TOKEN } });
    const form = tokenInput.closest("form");
    if (!form) {
      throw new Error("bootstrap form missing");
    }
    fireEvent.submit(form);
    expect(await screen.findByText("已经初始化。客户集成 not_claimed。")).toBeTruthy();
    fireEvent.click(screen.getByRole("link", { name: "管理入口" }));
    expect(await screen.findByRole("heading", { name: "总览" })).toBeTruthy();
    expect(screen.getByText("fixture-admin · alpha · admin")).toBeTruthy();
    expect(screen.getByRole("button", { name: "刷新" })).toBeDisabled();
    fireEvent.click(screen.getByRole("link", { name: "角色" }));
    expect(await screen.findByRole("heading", { name: "角色" })).toBeTruthy();
    expect(screen.getByText("主体 fixture-admin")).toBeTruthy();
    expect(screen.getByText("初始化状态 已初始化")).toBeTruthy();
    expect(screen.getByText("B06.roles 预检 not_precise")).toBeTruthy();
    expect(screen.queryByText("B01.projectRoleBinding 预检 not_precise")).toBeNull();
    expect(screen.getByRole("button", { name: "应用" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "退出" }));
    expect(await screen.findByRole("heading", { name: "管理登录" })).toBeTruthy();
    expect(screen.queryByText("主体 fixture-admin")).toBeNull();
    expect(screen.queryByRole("heading", { name: "总览" })).toBeNull();
    expect(seen).toEqual([
      "GET /control/v1/identity/current",
      "POST /control/v1/auth/bootstrap/consume",
      "GET /control/v1/identity/current",
      "POST /control/v1/auth/logout",
      "GET /control/v1/identity/current",
    ]);
  });

  it("sends current, bootstrap, and logout on one https origin and does not accept a build", async () => {
    const seen: string[] = [];
    let currentCount = 0;
    const registered = registerSameOriginTransport({
      origin: ORIGIN,
      pageOrigin: ORIGIN,
      fetchImpl: async (input, init) => {
        const url = requestUrl(input);
        const method = init?.method ?? "GET";
        seen.push(`${method} ${url}`);
        expect(url.startsWith(`${ORIGIN}/control/v1/`)).toBe(true);
        expect(headerValue(init?.headers, "origin")).toBe(ORIGIN);
        if (url === `${ORIGIN}/control/v1/identity/current`) {
          currentCount += 1;
          if (currentCount === 1) {
            return new Response(JSON.stringify(envelope({ install_state: "uninitialized", projects: [] })), {
              status: 200,
              headers: { "content-type": "application/json" },
            });
          }
          if (currentCount === 2) {
            return new Response(JSON.stringify(envelope()), { status: 200, headers: { "content-type": "application/json" } });
          }
          return new Response(
            JSON.stringify({
              contract: "oo.control-identity/v1",
              authority: "management_session",
              data_grants: "none",
              owner_certification: "not_issued",
              agent_session: "not_issued",
              harness_session: "not_issued",
              gateway_token: "not_issued",
              customer_integration: "not_claimed",
              error: { code: "unauthenticated", message: "signed out", retryable: false },
            }),
            { status: 401, headers: { "content-type": "application/json" } },
          );
        }
        if (url === `${ORIGIN}/control/v1/auth/bootstrap/consume`) {
          expect(method).toBe("POST");
          expect(headerValue(init?.headers, "x-oo-csrf")).toBe(CSRF);
          expect(init?.body).toBe(JSON.stringify({ token: TOKEN }));
          return new Response(
            JSON.stringify({
              contract: "oo.control-identity/v1",
              operation: "consumeBootstrapToken",
              outcome: "consumed",
              install_state: "initialized",
              fixture_class: "explicit_test_fixture",
              customer_owner: false,
              owner_certification: "not_issued",
              data_grants: "none",
              agent_session: "not_issued",
              harness_session: "not_issued",
              gateway_token: "not_issued",
              customer_integration: "not_claimed",
            }),
            { status: 200, headers: { "content-type": "application/json" } },
          );
        }
        if (url === `${ORIGIN}/control/v1/auth/logout`) {
          expect(method).toBe("POST");
          expect(init?.body).toBeUndefined();
          expect(headerValue(init?.headers, "x-oo-csrf")).toBe(CSRF);
          return new Response(
            JSON.stringify({
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
            }),
            { status: 200, headers: { "content-type": "application/json" } },
          );
        }
        if (url === `${ORIGIN}/control/v1/builds`) {
          return new Response("no-build", { status: 404, headers: { "content-type": "text/plain" } });
        }
        throw new Error(`unexpected ${method} ${url}`);
      },
    });
    expect(registered.ok).toBe(true);
    const unbound = await callOperation({ operationId: "B01.projectRoleBinding" });
    expect(unbound).toMatchObject({ ok: false, reason: "not_precise", fetched: false, live: false, journey: false, release: false });
    expect(await callOperation({ operationId: "registerSource" })).toMatchObject({ ok: false, reason: "not_precise", fetched: false });
    expect(seen).toEqual([]);
    const first = await callOperation({ operationId: "getIdentityCurrent" });
    expect(first.ok).toBe(true);
    if (!first.ok) {
      throw new Error("current was rejected");
    }
    expect(first).toMatchObject({ fetched: true, live: false, journey: false, release: false, customerIntegration: "not_claimed" });
    const consumed = await callOperation({
      operationId: "consumeBootstrapToken",
      current: first.body,
      body: { token: TOKEN },
    });
    expect(consumed).toMatchObject({ ok: true, fetched: true, live: false, customerIntegration: "not_claimed" });
    const second = await callOperation({ operationId: "getIdentityCurrent" });
    expect(second.ok).toBe(true);
    if (!second.ok) {
      throw new Error("initialized current was rejected");
    }
    const loggedOut = await callOperation({ operationId: "logoutManagementSession", current: second.body });
    expect(loggedOut).toMatchObject({ ok: true, fetched: true, live: false, journey: false, release: false });
    const signedOut = await callOperation({ operationId: "getIdentityCurrent" });
    expect(signedOut).toMatchObject({
      ok: false,
      status: 401,
      reason: "http_error",
      errorCode: "unauthenticated",
      fetched: true,
      live: false,
    });
    const builds = await callOperation({ operationId: "listBuilds" });
    expect(builds).toMatchObject({ ok: false, fetched: true, status: 404, reason: "invalid_schema", live: false, journey: false, release: false });
    expect(seen).toEqual([
      `GET ${ORIGIN}/control/v1/identity/current`,
      `POST ${ORIGIN}/control/v1/auth/bootstrap/consume`,
      `GET ${ORIGIN}/control/v1/identity/current`,
      `POST ${ORIGIN}/control/v1/auth/logout`,
      `GET ${ORIGIN}/control/v1/identity/current`,
      `GET ${ORIGIN}/control/v1/builds`,
    ]);
  });
});
