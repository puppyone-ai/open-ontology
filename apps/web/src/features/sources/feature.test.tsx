import type { ReactElement } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createMemoryRouter, RouterProvider } from "react-router";
import { describe, expect, it, vi } from "vitest";
import * as pages from "./pages";

const cases = [
    {
      name: "SourcesPage",
      path: "/admin/sources",
      title: "数据源",
      dependency: "依赖 B02。",
      action: "添加数据源",
      extra: null,
    },
    {
      name: "SourceWizardPage",
      path: "/admin/sources/new",
      title: "新建数据源",
      dependency: "依赖 B02、B07。",
      action: "应用",
      extra: "SourceWizard",
    },
    {
      name: "SourceDetailPage",
      path: "/admin/sources/:id",
      title: "数据源详情",
      dependency: "依赖 B02、B03。",
      action: "暂停",
      extra: null,
    },
] as const;

function renderPage(element: ReactElement, path: string) {
  const entry = path.replace(/:[A-Za-z]+/g, "secret-token");
  const router = createMemoryRouter([{ path, element }], { initialEntries: [entry] });
  return render(<RouterProvider router={router} />);
}

describe("sources", () => {
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
      if (item.extra === "SourceWizard") {
        const user = userEvent.setup();
        const secret = screen.getByLabelText("Secret");
        await user.type(secret, "super-secret");
        await user.click(screen.getByRole("button", { name: "离开" }));
        await user.click(screen.getByRole("button", { name: "放弃输入" }));
        expect(screen.getByLabelText("Secret")).toHaveValue("");
        expect(screen.queryByDisplayValue("super-secret")).toBeNull();
      }
      expect(fetchSpy).not.toHaveBeenCalled();
      view.unmount();
    }
    vi.unstubAllGlobals();
  });
});
