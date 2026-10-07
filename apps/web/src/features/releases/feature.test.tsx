import type { ReactElement } from "react";
import { render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it, vi } from "vitest";
import * as pages from "./pages";

const cases = [
    {
      name: "ReleasesPage",
      path: "/admin/releases",
      title: "发布",
      dependency: "依赖 B05。",
      action: "刷新",
      extra: null,
    },
    {
      name: "ReleaseDetailPage",
      path: "/admin/releases/:id",
      title: "发布详情",
      dependency: "依赖 B05、B03。",
      action: "发布",
      extra: "ReleaseGates",
    },
] as const;

function renderPage(element: ReactElement, path: string) {
  const entry = path.replace(/:[A-Za-z]+/g, "secret-token");
  const router = createMemoryRouter([{ path, element }], { initialEntries: [entry] });
  return render(<RouterProvider router={router} />);
}

describe("releases", () => {
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
      if (item.extra === "ReleaseGates") {
        expect(document.querySelectorAll("[data-gate='pass']")).toHaveLength(0);
        expect(document.querySelectorAll("[data-gate='not_run']")).toHaveLength(15);
      }
      expect(fetchSpy).not.toHaveBeenCalled();
      view.unmount();
    }
    vi.unstubAllGlobals();
  });
});
