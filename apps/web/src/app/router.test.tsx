import { cleanup, render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it, vi } from "vitest";
import { ADMIN_FEATURE_MODULES } from "../features/admin-overview/catalog";
import { GuardedAdminModule, adminSessionReady } from "./admin-gate";
import { ADMIN_MODULES } from "./module-registration";
import { appRoutes } from "./router";

function renderAt(path: string) {
  const router = createMemoryRouter(appRoutes, { initialEntries: [path] });
  return render(<RouterProvider router={router} />);
}

describe("auth and error routes", () => {
  it("sends an unauthenticated visit to login without a password", async () => {
    renderAt("/");
    expect(await screen.findByRole("heading", { name: "登录" })).toBeTruthy();
    expect(screen.getByText("此能力尚未接入。")).toBeTruthy();
    expect(screen.getByText("依赖 B01。")).toBeTruthy();
    expect(screen.queryByLabelText(/密码/)).toBeNull();
  });

  it("registers admin modules and still redirects them to admin login", async () => {
    expect(adminSessionReady()).toBe(false);
    expect(ADMIN_MODULES).toBe(ADMIN_FEATURE_MODULES);
    expect(ADMIN_MODULES.some((item) => item.routeId === "A04" && item.path === "/admin/sources")).toBe(true);
    renderAt("/admin");
    expect(await screen.findByRole("heading", { name: "管理登录" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "总览" })).toBeNull();
    expect(screen.queryByLabelText(/密码/)).toBeNull();
    cleanup();
    renderAt("/admin/sources");
    expect(await screen.findByRole("heading", { name: "管理登录" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "数据源" })).toBeNull();
    cleanup();
    renderAt("/admin/sources/route-sample");
    expect(await screen.findByRole("heading", { name: "管理登录" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "数据源详情" })).toBeNull();
  });

  it("does not call module.load while the session is not live", async () => {
    const load = vi.fn(async () => ({ default: () => <h1>数据源</h1> }));
    const router = createMemoryRouter(
      [
        { path: "/admin/login", element: <h1>管理登录</h1> },
        {
          path: "/admin/sources",
          element: (
            <GuardedAdminModule
              module={{
                routeId: "A04",
                path: "/admin/sources",
                capability: "admin.A04",
                layout: "admin",
                title: "数据源",
                load,
              }}
            />
          ),
        },
      ],
      { initialEntries: ["/admin/sources"] },
    );
    render(<RouterProvider router={router} />);
    expect(await screen.findByRole("heading", { name: "管理登录" })).toBeTruthy();
    expect(load).not.toHaveBeenCalled();
    expect(screen.queryByRole("heading", { name: "数据源" })).toBeNull();
  });

  it("keeps admin entry free of managed object names", async () => {
    renderAt("/admin/login");
    expect(await screen.findByRole("heading", { name: "管理登录" })).toBeTruthy();
    expect(screen.queryByText("数据源")).toBeNull();
    expect(screen.queryByText("待办")).toBeNull();
    renderAt("/admin/bootstrap");
    expect(await screen.findByRole("heading", { name: "初始化" })).toBeTruthy();
    expect(screen.getByText("浏览器不能在这里设置密码。")).toBeTruthy();
    expect(screen.queryByLabelText(/密码/)).toBeNull();
  });

  it("drops a token from returnTo and distinguishes not-found from empty", async () => {
    renderAt("/login?returnTo=/account%3Ftoken%3Dabc");
    expect(await screen.findByText("登录后的返回尚未接入。目标路径 /account")).toBeTruthy();
    expect(screen.queryByText(/token/)).toBeNull();
    renderAt("/missing");
    expect(await screen.findByRole("heading", { name: "找不到页面" })).toBeTruthy();
    expect(screen.queryByText("没有可显示的记录。")).toBeNull();
    renderAt("/forbidden");
    expect(await screen.findByText("当前身份没有执行此操作的权限。")).toBeTruthy();
  });
});
