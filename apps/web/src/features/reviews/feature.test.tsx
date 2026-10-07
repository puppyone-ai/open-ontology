import type { ReactElement } from "react";
import { render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it, vi } from "vitest";
import * as pages from "./pages";

const cases = [
    {
      name: "ReviewsPage",
      path: "/admin/reviews",
      title: "审核",
      dependency: "依赖 B04。",
      action: "刷新",
      extra: null,
    },
    {
      name: "ReviewDetailPage",
      path: "/admin/reviews/:id",
      title: "审核决定",
      dependency: "依赖 B04。",
      action: "转交",
      extra: "ReviewDesk",
    },
] as const;

function renderPage(element: ReactElement, path: string) {
  const entry = path.replace(/:[A-Za-z]+/g, "secret-token");
  const router = createMemoryRouter([{ path, element }], { initialEntries: [entry] });
  return render(<RouterProvider router={router} />);
}

describe("reviews", () => {
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
      if (item.extra === "ReviewDesk") {
        expect(screen.getByText("没有候选")).toBeTruthy();
        expect(screen.getByText("没有可显示的证据。")).toBeTruthy();
        expect(screen.queryByRole("button", { name: "提交决定" })).toBeNull();
        expect(screen.queryByLabelText("理由")).toBeNull();
      }
      expect(fetchSpy).not.toHaveBeenCalled();
      view.unmount();
    }
    vi.unstubAllGlobals();
  });
});
