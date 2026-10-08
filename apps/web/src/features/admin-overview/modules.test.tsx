import { render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it, vi } from "vitest";
import { bindingById, invokeOperation } from "@open-ontology/sdk-ts";
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

  it("keeps every consumed operation unbound", async () => {
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
    const seen = new Set<string>();
    for (const module of ADMIN_FEATURE_MODULES) {
      expect(seen.has(module.routeId)).toBe(false);
      seen.add(module.routeId);
      expect(module.layout).toBe("admin");
      expect(module.capability).toBe(`admin.${module.routeId}`);
      const operationIds = ADMIN_PAGE_OPERATIONS[module.routeId];
      expect(operationIds && operationIds.length > 0).toBe(true);
      for (const operationId of operationIds ?? []) {
        expect(bindingById(operationId)?.state).toBe("unbound");
        expect(invokeOperation(operationId)).toEqual({ ok: false, reason: "not_connected", operationId });
      }
      const loaded = await module.load();
      const Page = loaded.default;
      const entry = module.path.replace(/:[A-Za-z]+/g, "secret-token");
      const router = createMemoryRouter([{ path: module.path, element: <Page /> }], { initialEntries: [entry] });
      const view = render(<RouterProvider router={router} />);
      expect(screen.getByRole("heading", { name: module.title })).toBeTruthy();
      expect(screen.queryByText("审核决定已保存；尚未发布。")).toBeNull();
      expect(fetchSpy).not.toHaveBeenCalled();
      view.unmount();
    }
    vi.unstubAllGlobals();
  });
});
