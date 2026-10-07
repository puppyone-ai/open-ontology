import { render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it } from "vitest";
import { appRoutes } from "../../app/router";
import { ChatHomePage } from "./chat-home-page";

function renderAt(path: string) {
  const router = createMemoryRouter(appRoutes, { initialEntries: [path] });
  return render(<RouterProvider router={router} />);
}

describe("user pages without a backend", () => {
  it("keeps the chat entry on the login redirect", async () => {
    renderAt("/");
    expect(await screen.findByRole("heading", { name: "登录" })).toBeTruthy();
    expect(screen.queryByLabelText("问题")).toBeNull();
  });

  it("renders the unavailable chat surface without history or a send action", () => {
    const router = createMemoryRouter([{ path: "/", element: <ChatHomePage /> }], { initialEntries: ["/"] });
    render(<RouterProvider router={router} />);
    expect(screen.getByRole("heading", { name: "对话" })).toBeTruthy();
    expect(screen.getByText("依赖 B01、B08。")).toBeTruthy();
    expect(screen.getByText("这里没有历史记录，也没有可发送的问题。")).toBeTruthy();
    expect(screen.getByRole("button", { name: "发送" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "停止本轮" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "结束会话" })).toBeDisabled();
    expect(screen.queryByText("数据源")).toBeNull();
  });

  it("does not treat a conversation address as an existing thread", async () => {
    renderAt("/conversations/secret-token");
    expect(await screen.findByRole("heading", { name: "对话" })).toBeTruthy();
    expect(screen.getByText("这个地址不代表已经存在的对话。")).toBeTruthy();
    expect(screen.getByText("依赖 B08、B09。")).toBeTruthy();
    expect(screen.queryByText("secret-token")).toBeNull();
    expect(screen.getByRole("button", { name: "发送" })).toBeDisabled();
    expect(screen.getByRole("link", { name: "文件" })).toBeTruthy();
  });

  it("shows files as not connected and offers no download", async () => {
    renderAt("/conversations/secret-token/files");
    expect(await screen.findByRole("heading", { name: "文件" })).toBeTruthy();
    expect(screen.getByText("依赖 B07、B08。")).toBeTruthy();
    expect(screen.getByText("这里不提供下载，也没有大小、哈希或有效期。")).toBeTruthy();
    expect(screen.queryByRole("link", { name: "下载" })).toBeNull();
    expect(screen.queryByText("secret-token")).toBeNull();
  });

  it("keeps knowledge free of catalog names", async () => {
    renderAt("/knowledge");
    expect(await screen.findByRole("heading", { name: "知识" })).toBeTruthy();
    expect(screen.getByText("依赖 B05、B09。")).toBeTruthy();
    expect(screen.getByText("这里不列出目录、表、术语、指标、示例或关系。")).toBeTruthy();
    expect(screen.queryByText("数据源")).toBeNull();
    expect(screen.queryByText("待办")).toBeNull();
  });

  it("shows account without credentials or a sign-out action", async () => {
    renderAt("/account");
    expect(await screen.findByRole("heading", { name: "账户" })).toBeTruthy();
    expect(screen.getByText("依赖 B01。")).toBeTruthy();
    expect(screen.getByText("这里不回读凭证。")).toBeTruthy();
    expect(screen.queryByLabelText(/密码/)).toBeNull();
    expect(screen.queryByRole("button", { name: "退出" })).toBeNull();
  });
});
