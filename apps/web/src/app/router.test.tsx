import { render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it } from "vitest";
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
    expect(ADMIN_MODULES).toEqual([]);
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
