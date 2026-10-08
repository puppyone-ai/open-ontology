import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

const evidence = process.env.FE01_EVIDENCE;
if (!evidence) throw new Error("FE01_EVIDENCE is required");

function channel(value: number): number {
  const normalized = value / 255;
  return normalized <= 0.04045 ? normalized / 12.92 : ((normalized + 0.055) / 1.055) ** 2.4;
}

function contrastRatio(foreground: string, background: string): number {
  const lum = (input: string) => {
    const match = input.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
    if (!match) throw new Error(`unparsed color ${input}`);
    const red = channel(Number(match[1]));
    const green = channel(Number(match[2]));
    const blue = channel(Number(match[3]));
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
  };
  const lighter = Math.max(lum(foreground), lum(background));
  const darker = Math.min(lum(foreground), lum(background));
  return (lighter + 0.05) / (darker + 0.05);
}

async function paint(page: Page, theme: "light" | "dark") {
  await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
  await page.goto("/login");
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("登录");
  await page.getByRole("radio", { name: theme === "dark" ? "深色" : "浅色" }).check();
  await page.evaluate(async () => {
    await document.fonts.ready;
  });
}

test("axe keeps serious and critical violations empty", async ({ page }) => {
  await mkdir(evidence, { recursive: true });
  await paint(page, "light");
  const light = await new AxeBuilder({ page }).analyze();
  await paint(page, "dark");
  const dark = await new AxeBuilder({ page }).analyze();
  await writeFile(join(evidence, "axe.json"), `${JSON.stringify({ light, dark }, null, 2)}\n`);
  for (const result of [light, dark]) {
    const blocking = result.violations.filter((item) => item.impact === "serious" || item.impact === "critical");
    expect(blocking).toEqual([]);
  }
});

test("rendered login text meets contrast", async ({ page }) => {
  const rows: Array<{ theme: "light" | "dark"; name: string; minimum: number; ratio: number; pass: boolean }> = [];
  for (const theme of ["light", "dark"] as const) {
    await paint(page, theme);
    const measured = await page.evaluate(() => {
      const heading = document.querySelector("h1");
      if (!heading) throw new Error("missing login heading");
      const opaque = (element: Element): string => {
        let current: Element | null = element;
        while (current) {
          const background = getComputedStyle(current).backgroundColor;
          const match = background.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/);
          if (match && Number(match[4] ?? "1") > 0) return background;
          current = current.parentElement;
        }
        return getComputedStyle(document.body).backgroundColor;
      };
      return {
        color: getComputedStyle(heading).color,
        background: opaque(heading),
      };
    });
    const ratio = contrastRatio(measured.color, measured.background);
    rows.push({ theme, name: "login-heading", minimum: 4.5, ratio: Number(ratio.toFixed(3)), pass: ratio >= 4.5 });
    expect(ratio, `login heading ${theme}`).toBeGreaterThanOrEqual(4.5);
  }
  await writeFile(join(evidence, "contrast-browser.json"), `${JSON.stringify(rows, null, 2)}\n`);
});
