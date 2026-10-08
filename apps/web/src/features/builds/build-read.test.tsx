import { fireEvent, render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearSameOriginTransport, registerSameOriginTransport } from "@open-ontology/sdk-ts";
import { IdentityProvider } from "../../app/identity-context";
import { appRoutes } from "../../app/router";

const CSRF = "a".repeat(43);
const ORIGIN = "https://admin.example";
const MISSING_BUILD = `b_${"0".repeat(32)}`;
const UNCALLED = [
  "/control/v1/builds",
  "/cancel",
  "/cards",
  "/candidates",
  "/pack",
  "/control/v1/reviews",
  "/control/v1/classifications",
];

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

function emptyList() {
  return {
    active: false,
    builds: [],
    compiled_pack: "not_published",
    contract: "oo.selfbuild-control-api/v1",
    data_grants: "none",
    management_identity: "verified",
    operation: "listBuilds",
    published: false,
  };
}

function forbiddenBuild() {
  return {
    active: false,
    compiled_pack: "not_published",
    contract: "oo.selfbuild-control-api/v1",
    data_grants: "none",
    error: { code: "forbidden", message: "missing build", retryable: false },
    management_identity: "verified",
    published: false,
  };
}

function requestUrl(input: RequestInfo | URL): string {
  if (typeof input === "string") return input;
  if (input instanceof URL) return input.toString();
  return input.url;
}

describe("build list read", () => {
  afterEach(() => {
    clearSameOriginTransport();
    vi.unstubAllGlobals();
  });

  it("reads an empty listBuilds and does not call the other build operations", async () => {
    const seen: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const url = requestUrl(input);
        if (url !== "/control/v1/identity/current") {
          throw new Error(`unexpected identity ${url}`);
        }
        return new Response(JSON.stringify(envelope()), { status: 200, headers: { "content-type": "application/json" } });
      }),
    );
    const registered = registerSameOriginTransport({
      origin: ORIGIN,
      pageOrigin: ORIGIN,
      fetchImpl: async (input) => {
        const url = requestUrl(input);
        seen.push(url);
        if (url !== `${ORIGIN}/control/v1/builds?project_id=alpha`) {
          throw new Error(`unexpected ${url}`);
        }
        return new Response(JSON.stringify(emptyList()), { status: 200, headers: { "content-type": "application/json" } });
      },
    });
    expect(registered.ok).toBe(true);
    const router = createMemoryRouter(appRoutes, { initialEntries: ["/admin/builds"] });
    render(
      <IdentityProvider>
        <RouterProvider router={router} />
      </IdentityProvider>,
    );
    expect(await screen.findByText("listBuilds 0")).toBeTruthy();
    expect(screen.getByText("published false")).toBeTruthy();
    expect(screen.getByText("active false")).toBeTruthy();
    expect(screen.getByText("compiled_pack not_published")).toBeTruthy();
    expect(screen.getByText("没有构建行。")).toBeTruthy();
    expect(screen.getByText("空列表不是完成的构建。")).toBeTruthy();
    expect(screen.getByText("审核和分类还没有读取。")).toBeTruthy();
    expect(screen.getByRole("button", { name: "运行构建" })).toBeDisabled();
    expect(screen.queryByText("构建完成")).toBeNull();
    expect(seen).toEqual([`${ORIGIN}/control/v1/builds?project_id=alpha`]);
  });

  it("shows getBuild forbidden for a missing id and leaves review unread", async () => {
    const seen: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const url = requestUrl(input);
        if (url !== "/control/v1/identity/current") {
          throw new Error(`unexpected identity ${url}`);
        }
        return new Response(JSON.stringify(envelope()), { status: 200, headers: { "content-type": "application/json" } });
      }),
    );
    registerSameOriginTransport({
      origin: ORIGIN,
      pageOrigin: ORIGIN,
      fetchImpl: async (input) => {
        const url = requestUrl(input);
        seen.push(url);
        if (url !== `${ORIGIN}/control/v1/builds/${MISSING_BUILD}`) {
          throw new Error(`unexpected ${url}`);
        }
        return new Response(JSON.stringify(forbiddenBuild()), { status: 403, headers: { "content-type": "application/json" } });
      },
    });
    const router = createMemoryRouter(appRoutes, { initialEntries: [`/admin/builds/${MISSING_BUILD}`] });
    render(
      <IdentityProvider>
        <RouterProvider router={router} />
      </IdentityProvider>,
    );
    expect(await screen.findByText("getBuild http_error forbidden")).toBeTruthy();
    expect(screen.getByText("审核和分类还没有读取。")).toBeTruthy();
    expect(screen.getByRole("button", { name: "重试" })).toBeDisabled();
    expect(screen.getByText("S0 没有运行记录")).toBeTruthy();
    expect(seen).toEqual([`${ORIGIN}/control/v1/builds/${MISSING_BUILD}`]);
    for (const path of ["/cards", "/candidates", "/pack", "/cancel", "/control/v1/reviews", "/control/v1/classifications"]) {
      expect(seen.some((url) => url.includes(path))).toBe(false);
    }
  });

  it("does not send getBuild for a placeholder id or a plain 200 as a build", async () => {
    const seen: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const url = requestUrl(input);
        if (url !== "/control/v1/identity/current") {
          throw new Error(`unexpected identity ${url}`);
        }
        return new Response(JSON.stringify(envelope()), { status: 200, headers: { "content-type": "application/json" } });
      }),
    );
    registerSameOriginTransport({
      origin: ORIGIN,
      pageOrigin: ORIGIN,
      fetchImpl: async (input) => {
        const url = requestUrl(input);
        seen.push(url);
        return new Response("no-build", { status: 200, headers: { "content-type": "text/plain" } });
      },
    });
    const placeholder = createMemoryRouter(appRoutes, { initialEntries: ["/admin/builds/secret-token"] });
    const first = render(
      <IdentityProvider>
        <RouterProvider router={placeholder} />
      </IdentityProvider>,
    );
    expect(await screen.findByRole("heading", { name: "构建详情" })).toBeTruthy();
    expect(screen.getByText("审核和分类还没有读取。")).toBeTruthy();
    expect(seen).toEqual([]);
    first.unmount();
    const plain = createMemoryRouter(appRoutes, { initialEntries: [`/admin/builds/${MISSING_BUILD}`] });
    render(
      <IdentityProvider>
        <RouterProvider router={plain} />
      </IdentityProvider>,
    );
    expect(await screen.findByText("getBuild invalid_schema")).toBeTruthy();
    expect(screen.queryByText("构建完成")).toBeNull();
    expect(seen).toEqual([`${ORIGIN}/control/v1/builds/${MISSING_BUILD}`]);
    expect(UNCALLED.filter((path) => path !== "/control/v1/builds").every((path) => !seen[0]?.includes(path))).toBe(true);
  });

  it("does not invent a list when the same-origin transport is absent", async () => {
    const seen: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const url = requestUrl(input);
        seen.push(url);
        if (url !== "/control/v1/identity/current") {
          throw new Error(`unexpected ${url}`);
        }
        return new Response(JSON.stringify(envelope()), { status: 200, headers: { "content-type": "application/json" } });
      }),
    );
    const router = createMemoryRouter(appRoutes, { initialEntries: ["/admin/builds"] });
    render(
      <IdentityProvider>
        <RouterProvider router={router} />
      </IdentityProvider>,
    );
    expect(await screen.findByText("listBuilds origin_rejected")).toBeTruthy();
    expect(screen.queryByText("listBuilds 0")).toBeNull();
    expect(seen).toEqual(["/control/v1/identity/current"]);
    fireEvent.click(screen.getByRole("button", { name: "运行构建" }));
    expect(seen).toEqual(["/control/v1/identity/current"]);
  });
});
