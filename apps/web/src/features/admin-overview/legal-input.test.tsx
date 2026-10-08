import { render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it, vi } from "vitest";
import { csrfFromCurrent, GROUP_PUBLICATION } from "@open-ontology/sdk-ts";
import { IdentityProvider } from "../../app/identity-context";
import { appRoutes } from "../../app/router";
import { BuildsPage } from "../builds/pages";
import { legalAdminCall } from "./legal-call";

const CSRF = "a".repeat(43);

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

describe("legal admin input", () => {
  it("accepts a current envelope and still does not send B04", () => {
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
    const current = envelope();
    expect(csrfFromCurrent(current)).toEqual({ ok: true, headerName: "X-OO-CSRF", token: CSRF, revision: 4 });
    expect(csrfFromCurrent({ authorized: true, csrf_token: CSRF })).toEqual({ ok: false, reason: "not_current" });
    expect(GROUP_PUBLICATION.B04.ready).toBe(false);
    expect(GROUP_PUBLICATION.B04.contractCommit).toBe("7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9");
    const built = legalAdminCall("createBuild", current);
    expect(built).toEqual({
      ok: false,
      operationId: "createBuild",
      reason: "schema_only",
      headerName: "X-OO-CSRF",
      csrfBound: true,
    });
    expect(JSON.stringify(built).includes(CSRF)).toBe(false);
    expect(legalAdminCall("createBuild", { authorized: true }).reason).toBe("csrf_required");
    expect(legalAdminCall("createBuild", null).reason).toBe("csrf_required");
    expect(legalAdminCall("listBuilds", null)).toEqual({
      ok: false,
      operationId: "listBuilds",
      reason: "schema_only",
      headerName: null,
      csrfBound: false,
    });
    expect(legalAdminCall("B04.builder", current).reason).toBe("not_precise");
    expect(legalAdminCall("B01.projectRoleBinding", current).reason).toBe("not_precise");
    expect(legalAdminCall("registerSource", current).reason).toBe("not_precise");
    expect(legalAdminCall("getSourceCatalog", current).reason).toBe("not_precise");
    expect(legalAdminCall("createRelease", current).reason).toBe("not_precise");
    expect(legalAdminCall("rollbackRelease", current).reason).toBe("not_precise");
    expect(legalAdminCall("getActiveRolePack", current).reason).toBe("not_precise");
    expect(legalAdminCall("logoutManagementSession", current)).toEqual({
      ok: false,
      operationId: "logoutManagementSession",
      reason: "not_connected",
      headerName: "X-OO-CSRF",
      csrfBound: true,
    });
    expect(legalAdminCall("consumeBootstrapToken", current).reason).toBe("not_connected");
    expect(legalAdminCall("getIdentityCurrent", null).reason).toBe("not_connected");
    expect(fetchSpy).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });

  it("shows frozen prechecks on the build page without enabling the action", () => {
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
    const router = createMemoryRouter([{ path: "/admin/builds", element: <BuildsPage /> }], {
      initialEntries: ["/admin/builds"],
    });
    render(<RouterProvider router={router} />);
    expect(screen.getByRole("heading", { name: "构建" })).toBeTruthy();
    expect(screen.getByText("createBuild 状态 schema_frozen，调用 schema_only")).toBeTruthy();
    expect(screen.getByText("createBuild 预检 csrf_required")).toBeTruthy();
    expect(screen.getByText("listBuilds 预检 schema_only")).toBeTruthy();
    expect(screen.getByText("B04.builder 预检 not_precise")).toBeTruthy();
    expect(screen.getByRole("button", { name: "运行构建" })).toBeDisabled();
    expect(screen.queryByText("审核决定已保存；尚未发布。")).toBeNull();
    expect(fetchSpy).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });

  it("opens the real bootstrap page and does not consume a token", async () => {
    const seen: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = requestUrl(input);
        seen.push(`${init?.method ?? "GET"} ${url}`);
        if (url !== "/control/v1/identity/current") {
          throw new Error(`unexpected ${url}`);
        }
        return new Response(JSON.stringify(envelope({ install_state: "uninitialized", projects: [] })), {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      }),
    );
    const router = createMemoryRouter(appRoutes, { initialEntries: ["/admin/builds"] });
    render(
      <IdentityProvider>
        <RouterProvider router={router} />
      </IdentityProvider>,
    );
    expect(await screen.findByRole("heading", { name: "初始化" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "提交初始化" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "构建" })).toBeNull();
    expect(screen.queryByRole("button", { name: "运行构建" })).toBeNull();
    expect(seen).toEqual(["GET /control/v1/identity/current"]);
    vi.unstubAllGlobals();
  });
});
