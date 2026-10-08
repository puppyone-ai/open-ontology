import { render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it, vi } from "vitest";
import { bindingById, invokeOperation } from "@open-ontology/sdk-ts";
import { parseManagementSession } from "../../api/identity-session";
import { adminSessionReady } from "../../app/admin-gate";
import { IdentityProvider } from "../../app/identity-context";
import { ADMIN_MODULES } from "../../app/module-registration";
import { appRoutes } from "../../app/router";
import { ADMIN_FEATURE_MODULES } from "./catalog";

const IDENTITY_OPERATIONS = [
  "startAuthorizationCodeLogin",
  "completeAuthorizationCodeCallback",
  "getIdentityCurrent",
  "logoutManagementSession",
  "consumeBootstrapToken",
] as const;

const CSRF = "a".repeat(43);

function envelope(overrides: Record<string, unknown> = {}) {
  return {
    contract: "oo.control-identity/v1",
    operation: "getIdentityCurrent",
    subject: "fixture-admin",
    issuer: "https://idp.example",
    config_commit: "ab".repeat(32),
    revision: 1,
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

function installIdentityFetch(status: number, body: unknown) {
  const fetchSpy = vi.fn(async (input: RequestInfo | URL) => {
    const url = requestUrl(input);
    if (url !== "/control/v1/identity/current") {
      throw new Error(`unexpected fetch ${url}`);
    }
    return new Response(body === null ? null : JSON.stringify(body), {
      status,
      headers: { "content-type": "application/json" },
    });
  });
  vi.stubGlobal("fetch", fetchSpy);
  return fetchSpy;
}

describe("guarded admin routes", () => {
  it("keeps the fixture catalog closed and does not load a module", async () => {
    expect(adminSessionReady()).toBe(false);
    expect(adminSessionReady({ status: "pending" })).toBe(false);
    expect(adminSessionReady({ status: "anonymous" })).toBe(false);
    for (const operationId of IDENTITY_OPERATIONS) {
      expect(bindingById(operationId)?.state).toBe("fixture_verified");
      expect(bindingById(operationId)?.live).toBe(false);
    }
    expect(bindingById("B01.projectRoleBinding")?.state).toBe("unbound");
    expect(bindingById("B04.builder")?.state).toBe("unbound");
    expect(bindingById("createBuild")?.state).toBe("schema_frozen");
    expect(bindingById("createBuild")?.live).toBe(false);
    const uninitialized = parseManagementSession(envelope({ install_state: "uninitialized", projects: [] }));
    const reviewer = parseManagementSession(
      envelope({
        subject: "fixture-reviewer",
        projects: [{ project_id: "beta", role: "reviewer", capabilities: ["identity.current", "review.decide"] }],
      }),
    );
    if (!uninitialized || !reviewer) {
      throw new Error("session envelope was rejected");
    }
    expect(adminSessionReady({ status: "session", session: uninitialized })).toBe(false);
    expect(adminSessionReady({ status: "session", session: reviewer })).toBe(false);
    expect(ADMIN_MODULES).toBe(ADMIN_FEATURE_MODULES);
    expect(ADMIN_MODULES).toHaveLength(41);
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
    for (const operationId of IDENTITY_OPERATIONS) {
      expect(invokeOperation(operationId)).toEqual({ ok: false, reason: "not_connected", operationId });
    }
    expect(invokeOperation("createBuild")).toEqual({ ok: false, reason: "schema_only", operationId: "createBuild" });
    const loads = ADMIN_MODULES.map((module) => vi.spyOn(module, "load"));
    try {
      for (const module of ADMIN_MODULES) {
        const entry = module.path.replace(/:[A-Za-z]+/g, "route-sample");
        const router = createMemoryRouter(appRoutes, { initialEntries: [entry] });
        const view = render(<RouterProvider router={router} />);
        expect(await screen.findByRole("heading", { name: "管理登录" })).toBeTruthy();
        expect(screen.queryByRole("heading", { name: module.title })).toBeNull();
        if (module.path.includes(":")) {
          expect(screen.queryByText("route-sample")).toBeNull();
        }
        view.unmount();
      }
      for (const load of loads) {
        expect(load).not.toHaveBeenCalled();
      }
      expect(fetchSpy).not.toHaveBeenCalled();
    } finally {
      for (const load of loads) {
        load.mockRestore();
      }
      vi.unstubAllGlobals();
    }
  });

  it("probes current and still does not load a module when the session is absent", async () => {
    const cases: { status: number; body: unknown; path: string; heading: string }[] = [
      { status: 401, body: null, path: "/admin", heading: "管理登录" },
      { status: 401, body: null, path: "/admin/sources/route-sample", heading: "管理登录" },
      { status: 200, body: { authorized: true }, path: "/admin", heading: "管理登录" },
      {
        status: 200,
        body: envelope({ install_state: "uninitialized", projects: [] }),
        path: "/admin",
        heading: "初始化",
      },
      {
        status: 200,
        body: envelope({
          subject: "fixture-reviewer",
          projects: [{ project_id: "beta", role: "reviewer", capabilities: ["identity.current", "review.decide"] }],
        }),
        path: "/admin",
        heading: "没有权限",
      },
    ];
    for (const item of cases) {
      const fetchSpy = installIdentityFetch(item.status, item.body);
      const loads = ADMIN_MODULES.map((module) => vi.spyOn(module, "load"));
      const router = createMemoryRouter(appRoutes, { initialEntries: [item.path] });
      const view = render(
        <IdentityProvider>
          <RouterProvider router={router} />
        </IdentityProvider>,
      );
      try {
        expect(await screen.findByRole("heading", { name: item.heading })).toBeTruthy();
        expect(screen.queryByRole("heading", { name: "总览" })).toBeNull();
        if (item.path.includes("route-sample")) {
          expect(screen.queryByText("route-sample")).toBeNull();
        }
        for (const load of loads) {
          expect(load).not.toHaveBeenCalled();
        }
        expect(fetchSpy).toHaveBeenCalled();
        for (const call of fetchSpy.mock.calls) {
          expect(requestUrl(call[0])).toBe("/control/v1/identity/current");
        }
      } finally {
        view.unmount();
        for (const load of loads) {
          load.mockRestore();
        }
        vi.unstubAllGlobals();
      }
    }
  });

  it("mounts the overview for project.admin and leaves its product operations unbound", async () => {
    const fetchSpy = installIdentityFetch(200, envelope());
    const loads = ADMIN_MODULES.map((module) => vi.spyOn(module, "load"));
    const router = createMemoryRouter(appRoutes, { initialEntries: ["/admin"] });
    const view = render(
      <IdentityProvider>
        <RouterProvider router={router} />
      </IdentityProvider>,
    );
    try {
      expect(await screen.findByRole("heading", { name: "总览" })).toBeTruthy();
      expect(screen.getByText("依赖 B02、B03、B04、B08、B10。")).toBeTruthy();
      expect(screen.getByText("B04.builder 状态 unbound，调用 not_connected")).toBeTruthy();
      expect(screen.getByRole("button", { name: "刷新" })).toBeDisabled();
      const overviewIndex = ADMIN_MODULES.findIndex((module) => module.path === "/admin");
      const overview = loads[overviewIndex];
      if (!overview) {
        throw new Error("overview module is not registered");
      }
      expect(overview).toHaveBeenCalledTimes(1);
      for (const load of loads) {
        if (load !== overview) expect(load).not.toHaveBeenCalled();
      }
      for (const call of fetchSpy.mock.calls) {
        expect(requestUrl(call[0])).toBe("/control/v1/identity/current");
      }
    } finally {
      view.unmount();
      for (const load of loads) {
        load.mockRestore();
      }
      vi.unstubAllGlobals();
    }
  });
});
