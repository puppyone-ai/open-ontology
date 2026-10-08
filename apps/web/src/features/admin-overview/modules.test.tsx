import { render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it, vi } from "vitest";
import { bindingById, invokeOperation, SELFBUILD_HTTP } from "@open-ontology/sdk-ts";
import { ADMIN_MODULES } from "../../app/module-registration";
import { ADMIN_FEATURE_MODULES, ADMIN_PAGE_OPERATIONS } from "./catalog";
import { ADMIN_NAV } from "./nav";

describe("admin registrations", () => {
  it("registers forty-one feature modules for the guard", () => {
    expect(ADMIN_MODULES).toBe(ADMIN_FEATURE_MODULES);
    expect(ADMIN_MODULES).toHaveLength(41);
    expect(ADMIN_MODULES.map((item) => item.routeId)).toEqual(ADMIN_FEATURE_MODULES.map((item) => item.routeId));
    expect(ADMIN_MODULES.map((item) => item.path)).toEqual(ADMIN_FEATURE_MODULES.map((item) => item.path));
    expect(ADMIN_MODULES.some((item) => item.path === "/admin/login" || item.path === "/admin/bootstrap")).toBe(false);
  });

  it("matches navigation to the registrations", () => {
    const detailRouteIds = ["A06", "A08", "A10", "A12", "A14", "A18", "A20", "A23", "A24", "A30", "A38", "A40"];
    const linked = ADMIN_FEATURE_MODULES.filter((item) => !item.path.includes(":"));
    expect(ADMIN_NAV.map((item) => item.id)).toEqual(linked.map((item) => item.routeId));
    expect(ADMIN_NAV.map((item) => item.label)).toEqual(linked.map((item) => item.title));
    expect(ADMIN_NAV.map((item) => item.to)).toEqual(linked.map((item) => item.path));
    expect(ADMIN_NAV.some((item) => item.to.includes(":"))).toBe(false);
    expect(detailRouteIds).toHaveLength(12);
    for (const routeId of detailRouteIds) {
      expect(ADMIN_FEATURE_MODULES.some((item) => item.routeId === routeId && item.path.includes(":"))).toBe(true);
      expect(ADMIN_NAV.some((item) => item.id === routeId)).toBe(false);
    }
  });

  it("keeps placeholder operations unbound and frozen contract operations unsent", async () => {
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
    const frozenIds = new Set<string>(SELFBUILD_HTTP.map((item) => item[0]));
    const shown = new Set(Object.values(ADMIN_PAGE_OPERATIONS).flat());
    expect(SELFBUILD_HTTP).toHaveLength(15);
    for (const [operationId] of SELFBUILD_HTTP) {
      expect(shown.has(operationId)).toBe(true);
      expect(bindingById(operationId)?.state).toBe("schema_frozen");
      expect(bindingById(operationId)?.live).toBe(false);
    }
    for (const operationId of ["registerSource", "getSourceCatalog", "createRelease", "rollbackRelease", "getActiveRolePack"]) {
      expect(bindingById(operationId)).toBeNull();
      expect(shown.has(operationId)).toBe(false);
    }
    const seen = new Set<string>();
    for (const module of ADMIN_FEATURE_MODULES) {
      expect(seen.has(module.routeId)).toBe(false);
      seen.add(module.routeId);
      expect(module.layout).toBe("admin");
      expect(module.capability).toBe(`admin.${module.routeId}`);
      const operationIds = ADMIN_PAGE_OPERATIONS[module.routeId];
      expect(operationIds && operationIds.length > 0).toBe(true);
      for (const operationId of operationIds ?? []) {
        if (frozenIds.has(operationId)) {
          expect(invokeOperation(operationId)).toEqual({ ok: false, reason: "schema_only", operationId });
        } else {
          expect(bindingById(operationId)?.state).toBe("unbound");
          expect(invokeOperation(operationId)).toEqual({ ok: false, reason: "not_connected", operationId });
        }
      }
      const loaded = await module.load();
      const Page = loaded.default;
      const entry = module.path.replace(/:[A-Za-z]+/g, "secret-token");
      const router = createMemoryRouter([{ path: module.path, element: <Page /> }], { initialEntries: [entry] });
      const view = render(<RouterProvider router={router} />);
      expect(screen.getByRole("heading", { name: module.title })).toBeTruthy();
      expect(screen.queryByText("审核决定已保存；尚未发布。")).toBeNull();
      for (const operationId of operationIds ?? []) {
        const method = SELFBUILD_HTTP.find(([id]) => id === operationId)?.[1];
        const precheck = method === "POST" ? "csrf_required" : method === "GET" ? "schema_only" : "not_precise";
        expect(screen.getByText(`${operationId} 预检 ${precheck}`)).toBeTruthy();
        if (frozenIds.has(operationId)) {
          expect(screen.getByText(`${operationId} 状态 schema_frozen，调用 schema_only`)).toBeTruthy();
        }
      }
      if (module.routeId === "A18") {
        expect(screen.getByText("没有候选")).toBeTruthy();
        expect(screen.queryByRole("button", { name: "接受" })).toBeNull();
      }
      expect(fetchSpy).not.toHaveBeenCalled();
      view.unmount();
    }
    vi.unstubAllGlobals();
  });
});
