import { render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it, vi } from "vitest";
import { bindingById, invokeOperation } from "@open-ontology/sdk-ts";
import { ADMIN_MODULES } from "../../app/module-registration";
import { ADMIN_FEATURE_MODULES, ADMIN_PAGE_OPERATIONS } from "./catalog";
import { ADMIN_NAV } from "./nav";

describe("admin registrations", () => {
  it("does not write G1's mount list", () => {
    expect(ADMIN_MODULES).toEqual([]);
  });

  it("matches navigation to the registrations", () => {
    expect(ADMIN_NAV.map((item) => item.id)).toEqual(ADMIN_FEATURE_MODULES.map((item) => item.routeId));
    expect(ADMIN_NAV.map((item) => item.label)).toEqual(ADMIN_FEATURE_MODULES.map((item) => item.title));
    expect(ADMIN_NAV.map((item) => item.to)).toEqual(ADMIN_FEATURE_MODULES.map((item) => item.path));
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
