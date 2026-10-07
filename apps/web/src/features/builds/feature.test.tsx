import type { ReactElement } from "react";
import { render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it, vi } from "vitest";
import * as pages from "./pages";

const cases = [
    {
      name: "BuildsPage",
      path: "/admin/builds",
      title: "构建",
      dependency: "依赖 B04。",
      action: "运行构建",
      extra: null,
    },
    {
      name: "BuildDetailPage",
      path: "/admin/builds/:id",
      title: "构建详情",
      dependency: "依赖 B04、B03。",
      action: "重试",
      extra: "BuildStages",
    },
] as const;

function renderPage(element: ReactElement, path: string) {
  const entry = path.replace(/:[A-Za-z]+/g, "secret-token");
  const router = createMemoryRouter([{ path, element }], { initialEntries: [entry] });
  return render(<RouterProvider router={router} />);
}

describe("builds", () => {
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
      if (item.extra === "BuildStages") {
        expect(screen.getByText("S0 没有运行记录")).toBeTruthy();
        expect(screen.getByText("S10 没有运行记录")).toBeTruthy();
      }
      expect(fetchSpy).not.toHaveBeenCalled();
      view.unmount();
    }
    vi.unstubAllGlobals();
  });
});
