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
  await page.goto("/");
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

test("rendered foundation colors meet text and control contrast", async ({ page }) => {
  const rows: Array<{
    theme: "light" | "dark";
    name: string;
    minimum: number | null;
    ratio: number;
    pass: boolean | null;
    required?: false;
  }> = [];
  for (const theme of ["light", "dark"] as const) {
    await paint(page, theme);
    const measured = await page.evaluate(() => {
      const input = document.querySelector(".oo-input");
      const row = document.querySelector(".oo-row");
      const button = document.querySelector(".oo-button");
      if (!input || !row || !button) throw new Error("missing sample");
      const inputStyle = getComputedStyle(input);
      const rowStyle = getComputedStyle(row);
      const buttonStyle = getComputedStyle(button);
      return {
        swatches: [...document.querySelectorAll<HTMLElement>("[data-pair]")].map((element) => ({
          name: element.dataset.pair ?? "",
          color: getComputedStyle(element).color,
          background: getComputedStyle(element).backgroundColor,
        })),
        input: { color: inputStyle.borderTopColor, background: inputStyle.backgroundColor },
        decorative: { color: rowStyle.borderBottomColor, background: rowStyle.backgroundColor },
        button: { color: buttonStyle.color, background: buttonStyle.backgroundColor },
      };
    });
    for (const swatch of measured.swatches) {
      const ratio = contrastRatio(swatch.color, swatch.background);
      rows.push({ theme, name: swatch.name, minimum: 4.5, ratio: Number(ratio.toFixed(3)), pass: ratio >= 4.5 });
      expect(ratio, `${swatch.name} ${theme}`).toBeGreaterThanOrEqual(4.5);
    }
    const inputRatio = contrastRatio(measured.input.color, measured.input.background);
    rows.push({ theme, name: "input-border", minimum: 3, ratio: Number(inputRatio.toFixed(3)), pass: inputRatio >= 3 });
    expect(inputRatio, `input ${theme}`).toBeGreaterThanOrEqual(3);
    const buttonRatio = contrastRatio(measured.button.color, measured.button.background);
    rows.push({ theme, name: "button", minimum: 4.5, ratio: Number(buttonRatio.toFixed(3)), pass: buttonRatio >= 4.5 });
    expect(buttonRatio, `button ${theme}`).toBeGreaterThanOrEqual(4.5);
    const decorativeRatio = contrastRatio(measured.decorative.color, measured.decorative.background);
    rows.push({
      theme,
      name: "decorative-row-border",
      minimum: null,
      ratio: Number(decorativeRatio.toFixed(3)),
      pass: null,
      required: false,
    });
  }
  await writeFile(join(evidence, "contrast-browser.json"), `${JSON.stringify(rows, null, 2)}\n`);
});
