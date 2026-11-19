import { expect, test, type Page } from "@playwright/test";

const gallery = "http://127.0.0.1:5275/";
const SECTIONS = ["buttons", "progress", "fields", "controls", "feedback", "rows"] as const;
const box = async (page: Page, selector: string) => page.locator(selector).first().evaluate((element) => { const r = element.getBoundingClientRect(); return { width: Math.round(r.width * 100) / 100, height: Math.round(r.height * 100) / 100 }; });

test.beforeEach(async ({ page }) => { await page.goto(gallery); await page.waitForSelector("[data-section=buttons]"); await page.evaluate(() => document.fonts.ready); });

test.describe("component screenshots", () => {
  for (const id of SECTIONS) {
    test(`${id}`, async ({ page }) => { await expect(page.locator(`[data-section=${id}]`)).toHaveScreenshot(`${id}.png`); });
  }
  test("hover and keyboard focus on a button", async ({ page }) => {
    const section = page.locator("[data-section=buttons]");
    await section.getByRole("button", { name: "Primary" }).hover();
    await expect(section).toHaveScreenshot("buttons-hover-primary.png");
    await page.mouse.move(0, 0);
    await page.keyboard.press("Tab");
    await expect(section).toHaveScreenshot("buttons-focus-first.png");
  });
  test("a field with an error and its focus ring", async ({ page }) => {
    const field = page.locator("[data-section=fields] .so-field").nth(1);
    await field.locator("input").focus();
    await expect(field).toHaveScreenshot("field-error-focus.png");
  });
});

// D86: what replaces an icon, a label or a row has the same size, so nothing jumps when the work starts or ends.
test.describe("loading states keep their box", () => {
  test("a button that loads keeps its width and height (icon replaced by the spinner, or the label overlaid)", async ({ page }) => {
    expect(await box(page, "[data-testid=loading-icon]")).toEqual(await box(page, "[data-testid=idle-icon]"));
    expect(await box(page, "[data-testid=loading-text]")).toEqual(await box(page, "[data-testid=idle-text]"));
    expect(await box(page, "[data-testid=loading-secondary]")).toEqual(await box(page, "[data-testid=idle-text]"));
  });
  test("an icon button that loads keeps its size", async ({ page }) => {
    expect(await box(page, "[data-testid=loading-icon-button]")).toEqual(await box(page, "[data-testid=idle-icon-button]"));
  });
  test("the spinner has the box of the 16px icon it replaces", async ({ page }) => {
    const spinner = await page.locator("[data-testid=loading-icon] svg").first().evaluate((e) => [(e as unknown as SVGElement & { clientWidth: number }).clientWidth, (e as unknown as SVGElement & { clientHeight: number }).clientHeight]);
    const icon = await page.locator("[data-testid=idle-icon] svg").first().evaluate((e) => [(e as unknown as SVGElement & { clientWidth: number }).clientWidth, (e as unknown as SVGElement & { clientHeight: number }).clientHeight]);
    expect(spinner).toEqual(icon);
  });
  test("a skeleton row is as tall as the row it stands in for, and a skeleton metric as the number", async ({ page }) => {
    expect((await box(page, "[data-testid=row-skeleton]")).height).toBe((await box(page, "[data-testid=row-real]")).height);
    expect((await box(page, "[data-testid=metric-skeleton]")).height).toBe((await box(page, "[data-testid=metric-real]")).height);
  });
});

// D86: one set of timings. Every transition on an interactive element is 120 ms (state) or 200 ms (movement), never an ad-hoc value.
test("transitions use the two timing tokens only", async ({ page }) => {
  const durations = await page.evaluate(() => {
    const found = new Set<string>();
    for (const element of document.querySelectorAll("button, input, select, textarea, .so-switch-thumb, .so-progress-fill, a")) {
      for (const part of getComputedStyle(element).transitionDuration.split(",")) found.add(part.trim());
    }
    return [...found];
  });
  expect(durations.filter((value) => !["0s", "0.12s", "0.2s"].includes(value))).toEqual([]);
});

// D86: every input has a label, a helper or error text wired with aria-describedby, and an error is announced.
test("fields wire their helper and error text", async ({ page }) => {
  const problems = await page.evaluate(() => [...document.querySelectorAll("[data-section=fields] .so-field")].flatMap((field) => {
    const control = field.querySelector("input, textarea, select");
    const note = field.querySelector(".so-field-helper, .so-field-error");
    const out: string[] = [];
    if (!control?.id || !field.querySelector(`label[for="${control.id}"]`)) out.push("label");
    if (note && !(control?.getAttribute("aria-describedby") ?? "").split(" ").includes(note.id)) out.push(`describedby ${note.textContent}`);
    if (field.querySelector(".so-field-error") && field.querySelector(".so-field-error")?.getAttribute("role") !== "alert") out.push("alert");
    return out;
  }));
  expect(problems).toEqual([]);
});
