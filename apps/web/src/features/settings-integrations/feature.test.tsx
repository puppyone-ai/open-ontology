import type { ReactElement } from "react";
import { render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it, vi } from "vitest";
import * as pages from "./pages";

const cases = [
    {
      name: "AgentSettingsPage",
      path: "/admin/settings/agent",
      title: "Agent",
      dependency: "依赖 B11。",
      action: "保存",
      extra: null,
    },
    {
      name: "DeploymentPage",
      path: "/admin/settings/deployment",
      title: "部署",
      dependency: "依赖 B11。",
      action: "应用",
      extra: null,
    },
    {
      name: "ClientsPage",
      path: "/admin/clients",
      title: "外部客户端",
      dependency: "依赖 B06、B11。",
      action: "撤销",
      extra: null,
    },
    {
      name: "ChannelsPage",
      path: "/admin/channels",
      title: "渠道",
      dependency: "依赖 B11。",
      action: "测试",
      extra: null,
    },
] as const;

function renderPage(element: ReactElement, path: string) {
  const entry = path.replace(/:[A-Za-z]+/g, "secret-token");
  const router = createMemoryRouter([{ path, element }], { initialEntries: [entry] });
  return render(<RouterProvider router={router} />);
}

describe("settings-integrations", () => {
  it("renders every page unbound and does not call fetch", async () => {
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
    for (const item of cases) {
      const Page = pages[item.name];
      const view = renderPage(<Page />, item.path);
      expect(screen.getByRole("heading", { name: item.title })).toBeTruthy();
      expect(screen.getByText(item.dependency)).toBeTruthy();
      expect(screen.getByRole("button", { name: item.action })).toBeDisabled();
      if (item.path.includes(":")) {
        expect(screen.queryByText("secret-token")).toBeNull();
      }

      expect(fetchSpy).not.toHaveBeenCalled();
      view.unmount();
    }
    vi.unstubAllGlobals();
  });
});
