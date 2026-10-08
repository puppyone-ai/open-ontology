import { fireEvent, render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it, vi } from "vitest";
import { IdentityProvider } from "../../app/identity-context";
import { appRoutes } from "../../app/router";
import { RolesPage } from "./pages";

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

describe("permission current session", () => {
  it("does not invent a role when current is absent", () => {
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
    const router = createMemoryRouter([{ path: "/admin/permissions/roles", element: <RolesPage /> }], {
      initialEntries: ["/admin/permissions/roles"],
    });
    render(<RouterProvider router={router} />);
    expect(screen.getByRole("heading", { name: "角色" })).toBeTruthy();
    expect(screen.getByText("当前身份尚未接入。")).toBeTruthy();
    expect(screen.queryByText(/fixture-admin/)).toBeNull();
    expect(screen.getByText("B06.roles 状态 unbound，调用 not_connected")).toBeTruthy();
    expect(screen.getByRole("button", { name: "应用" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "退出" })).toBeNull();
    expect(fetchSpy).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });

  it("shows the current admin capability and logs out through the service", async () => {
    const seen: string[] = [];
    let currentCalls = 0;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = requestUrl(input);
        seen.push(`${init?.method ?? "GET"} ${url}`);
        if (url === "/control/v1/identity/current") {
          currentCalls += 1;
          if (currentCalls === 1) {
            return new Response(JSON.stringify(envelope()), { status: 200, headers: { "content-type": "application/json" } });
          }
          return new Response(null, { status: 401 });
        }
        if (url === "/control/v1/auth/logout") {
          expect(init?.method).toBe("POST");
          expect(new Headers(init?.headers).get("x-oo-csrf")).toBe(CSRF);
          return new Response(JSON.stringify({ outcome: "revoked", customer_integration: "not_claimed" }), {
            status: 200,
            headers: { "content-type": "application/json" },
          });
        }
        throw new Error(`unexpected ${url}`);
      }),
    );
    const router = createMemoryRouter(appRoutes, { initialEntries: ["/admin/permissions/roles"] });
    render(
      <IdentityProvider>
        <RouterProvider router={router} />
      </IdentityProvider>,
    );
    expect(await screen.findByRole("heading", { name: "角色" })).toBeTruthy();
    expect(screen.getByText("主体 fixture-admin")).toBeTruthy();
    expect(screen.getByText("初始化状态 已初始化")).toBeTruthy();
    expect(screen.getByText("项目 alpha 角色 admin 能力 binding.read identity.current project.admin session.revoke")).toBeTruthy();
    expect(screen.getByText("fixture-admin · alpha · admin")).toBeTruthy();
    expect(screen.getByText("B06.roles 状态 unbound，调用 not_connected")).toBeTruthy();
    expect(screen.queryByText("claims")).toBeNull();
    expect(screen.getByRole("button", { name: "应用" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "退出" }));
    expect(await screen.findByRole("heading", { name: "管理登录" })).toBeTruthy();
    expect(screen.queryByText("主体 fixture-admin")).toBeNull();
    expect(screen.queryByRole("heading", { name: "角色" })).toBeNull();
    expect(seen).toEqual([
      "GET /control/v1/identity/current",
      "POST /control/v1/auth/logout",
      "GET /control/v1/identity/current",
    ]);
    vi.unstubAllGlobals();
  });

  it("keeps the current role when logout is rejected", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = requestUrl(input);
        if (url === "/control/v1/identity/current") {
          return new Response(JSON.stringify(envelope()), { status: 200, headers: { "content-type": "application/json" } });
        }
        if (url === "/control/v1/auth/logout") {
          expect(init?.method).toBe("POST");
          return new Response(JSON.stringify({ outcome: "rejected" }), { status: 403, headers: { "content-type": "application/json" } });
        }
        throw new Error(`unexpected ${url}`);
      }),
    );
    const router = createMemoryRouter(appRoutes, { initialEntries: ["/admin/permissions/bindings"] });
    render(
      <IdentityProvider>
        <RouterProvider router={router} />
      </IdentityProvider>,
    );
    expect(await screen.findByRole("heading", { name: "身份绑定" })).toBeTruthy();
    expect(screen.getByText("B01.projectRoleBinding 状态 unbound，调用 not_connected")).toBeTruthy();
    expect(screen.getByText("这里不列出人、组或 claims。")).toBeTruthy();
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "退出" }));
    expect(await screen.findByText("这一步没有完成。")).toBeTruthy();
    expect(screen.getByText("主体 fixture-admin")).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "管理登录" })).toBeNull();
    vi.unstubAllGlobals();
  });
});
