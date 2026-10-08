import { render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it, vi } from "vitest";
import { bindingById } from "@open-ontology/sdk-ts";
import { adminSessionReady } from "../../app/admin-gate";
import { ADMIN_MODULES } from "../../app/module-registration";
import { appRoutes } from "../../app/router";
import { ADMIN_FEATURE_MODULES } from "./catalog";

describe("guarded admin routes", () => {
  it("shows login for every registered module and does not load it", async () => {
    expect(adminSessionReady()).toBe(false);
    expect(bindingById("getIdentityCurrent")?.state).toBe("schema_frozen");
    expect(bindingById("getIdentityCurrent")?.live).toBe(false);
    expect(ADMIN_MODULES).toBe(ADMIN_FEATURE_MODULES);
    expect(ADMIN_MODULES).toHaveLength(41);
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
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
});
