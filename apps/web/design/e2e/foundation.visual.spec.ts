import { mkdir, stat } from "node:fs/promises";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";

const evidence = process.env.FE01_EVIDENCE;
if (!evidence) throw new Error("FE01_EVIDENCE is required");
const shotDir = join(evidence, "screenshots");

const sizes = [
  { name: "1440x900", width: 1440, height: 900 },
  { name: "1280x800", width: 1280, height: 800 },
  { name: "768x1024", width: 768, height: 1024 },
  { name: "390x844", width: 390, height: 844 },
] as const;

const themes = [
  { name: "light", label: "浅色" },
  { name: "dark", label: "深色" },
] as const;

const densities = [
  { name: "comfortable", label: "舒适" },
  { name: "compact", label: "紧凑" },
] as const;

test.beforeAll(async () => {
  await mkdir(shotDir, { recursive: true });
});

async function shoot(page: Page, fileName: string) {
  await page.evaluate(async () => {
    await document.fonts.ready;
  });
  const path = join(shotDir, fileName);
  await page.screenshot({ path, fullPage: true });
  expect((await stat(path)).size).toBeGreaterThan(2000);
}

for (const size of sizes) {
  for (const theme of themes) {
    for (const density of densities) {
      test(`${size.name} ${theme.name} ${density.name}`, async ({ page }) => {
        await page.setViewportSize({ width: size.width, height: size.height });
        await page.emulateMedia({ colorScheme: theme.name, reducedMotion: "no-preference" });
        await page.goto("/");
        await page.getByRole("radio", { name: theme.label }).check();
        await page.getByRole("radio", { name: density.label }).check();
        await expect(page.locator("html")).toHaveAttribute("data-theme", theme.name);
        await expect(page.locator("html")).toHaveAttribute("data-density", density.name);
        await expect(page.getByRole("heading", { level: 1 })).toHaveText("设计基线");
        await shoot(page, `${size.name}-${theme.name}-${density.name}.png`);
      });
    }
  }
}

for (const theme of themes) {
  test(`1280x800 ${theme.name} roomy`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.emulateMedia({ colorScheme: theme.name, reducedMotion: "no-preference" });
    await page.goto("/");
    await page.getByRole("radio", { name: theme.label }).check();
    await page.getByRole("radio", { name: "宽松" }).check();
    await expect(page.locator("html")).toHaveAttribute("data-density", "roomy");
    await shoot(page, `1280x800-${theme.name}-roomy.png`);
  });
}

test("keyboard focus at 1280 light", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.emulateMedia({ colorScheme: "light", reducedMotion: "no-preference" });
  await page.goto("/");
  await page.keyboard.press("Tab");
  await expect(page.locator("input[name='theme']:focus")).toHaveCount(1);
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.keyboard.press("Space");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  expect(await page.evaluate(() => localStorage.length)).toBe(0);
  await shoot(page, "keyboard-focus-1280x800-dark.png");
});

test("reduced motion removes the sample transition", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
  await page.goto("/");
  const duration = await page.locator("[data-motion-sample]").evaluate((element) => {
    return getComputedStyle(element).transitionDuration;
  });
  expect(duration).toBe("0.01ms");
  await shoot(page, "reduced-motion-1280x800-light.png");
});
