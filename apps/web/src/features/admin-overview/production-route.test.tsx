import { render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it } from "vitest";
import { ADMIN_MODULES } from "../../app/module-registration";
import { appRoutes } from "../../app/router";

describe("G1 still owns the admin mount", () => {
  it("keeps /admin on the login redirect and an empty module list", async () => {
    const router = createMemoryRouter(appRoutes, { initialEntries: ["/admin"] });
    render(<RouterProvider router={router} />);
    expect(await screen.findByRole("heading", { name: "管理登录" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "总览" })).toBeNull();
    expect(ADMIN_MODULES).toEqual([]);
  });
});
