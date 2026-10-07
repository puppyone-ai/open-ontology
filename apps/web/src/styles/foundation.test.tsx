import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { foundationCopy as copy } from "./copy";
import { Foundation } from "./foundation";

describe("foundation", () => {
  it("shows the public foundation copy and no business action", async () => {
    render(<Foundation />);
    expect(screen.getByRole("heading", { name: copy.title, level: 1 })).toBeTruthy();
    expect(screen.getByText(copy.lede)).toBeTruthy();
    expect(screen.getByText(copy.stateUnbound)).toBeTruthy();
    expect(screen.getByText(copy.stateForbidden)).toBeTruthy();
    expect(screen.getByText(copy.rareGlyph)).toBeTruthy();
    const button = screen.getByRole("button", { name: copy.buttonLabel });
    expect(button.hasAttribute("disabled")).toBe(true);
    expect(window.localStorage.length).toBe(0);
    await userEvent.click(screen.getByRole("radio", { name: copy.themeDark }));
    expect(document.documentElement.dataset.theme).toBe("dark");
    await userEvent.click(screen.getByRole("radio", { name: copy.densityCompact }));
    expect(document.documentElement.dataset.density).toBe("compact");
    expect(window.localStorage.length).toBe(0);
  });
});
