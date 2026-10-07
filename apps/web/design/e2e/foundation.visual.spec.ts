import { mkdir, stat, writeFile } from "node:fs/promises";
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
  await mkdir(join(evidence, "raw"), { recursive: true });
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

function toMilliseconds(duration: string) {
  return duration.endsWith("ms") ? Number.parseFloat(duration) : Number.parseFloat(duration) * 1000;
}

test("production reduced motion clears panel and button transitions", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
  await page.goto("/");
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("登录");
  await expect(page.locator("[data-motion-sample]")).toHaveCount(0);
  const panel = await page.locator(".oo-panel").first().evaluate((element) => {
    const style = getComputedStyle(element);
    return { property: style.transitionProperty, duration: style.transitionDuration };
  });
  expect(panel.property).toBe("none");
  expect(toMilliseconds(panel.duration)).toBeCloseTo(0, 5);
  const ruleDuration = await page.evaluate(() => {
    for (const sheet of document.styleSheets) {
      let rules: CSSRuleList;
      try {
        rules = sheet.cssRules;
      } catch {
        continue;
      }
      for (const rule of rules) {
        if (!(rule instanceof CSSMediaRule) || !rule.conditionText.includes("prefers-reduced-motion")) continue;
        for (const inner of rule.cssRules) {
          if (inner instanceof CSSStyleRule && inner.selectorText === ".oo-motion") return inner.style.transitionDuration;
        }
      }
    }
    return "";
  });
  expect(toMilliseconds(ruleDuration)).toBeCloseTo(0.01, 5);
  await shoot(page, "production-reduced-motion-1280x800-light.png");

  await page.goto("/account");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("账户");
  const menu = await page.getByRole("button", { name: "菜单" }).evaluate((element) => {
    const style = getComputedStyle(element);
    return { property: style.transitionProperty, duration: style.transitionDuration };
  });
  expect(menu.property).toBe("none");
  expect(toMilliseconds(menu.duration)).toBeCloseTo(0, 5);
  await shoot(page, "production-account-1280x800-light.png");
});

test("production routes use the subset and keep send disabled", async ({ page }) => {
  const hits: { url: string; status: number }[] = [];
  page.on("response", (response) => {
    const url = response.url();
    if (url.includes("/fonts/")) hits.push({ url, status: response.status() });
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.emulateMedia({ colorScheme: "light", reducedMotion: "no-preference" });
  const routes = [
    "/login",
    "/account",
    "/knowledge",
    "/conversations/route-sample",
    "/conversations/route-sample/files",
    "/admin/login",
    "/admin/bootstrap",
    "/forbidden",
    "/unsupported",
    "/missing-page",
  ];
  for (const path of routes) {
    await page.goto(path);
    await page.evaluate(async () => {
      await document.fonts.ready;
    });
    if (path === "/conversations/route-sample") {
      await expect(page.getByRole("button", { name: "发送" })).toBeDisabled();
      await expect(page.getByRole("textbox", { name: "问题" })).toBeDisabled();
      await shoot(page, "production-conversation-disabled-1280x800-light.png");
    }
  }
  await writeFile(join(evidence, "raw", "font-requests.json"), `${JSON.stringify(hits, null, 2)}\n`);
  expect(hits.length).toBeGreaterThan(0);
  expect(hits.every((hit) => hit.status === 200 || hit.status === 304)).toBe(true);
  expect(hits.some((hit) => hit.status === 200 && new URL(hit.url).pathname.endsWith("/NotoSansSC-400-subset.woff2"))).toBe(true);
  const paths = hits.map((hit) => new URL(hit.url).pathname);
  expect(paths.every((path) => path.startsWith("/fonts/"))).toBe(true);
  expect(hits.every((hit) => new URL(hit.url).origin === "http://127.0.0.1:4173")).toBe(true);
  expect(paths.some((path) => path.endsWith("/NotoSansSC-400-subset.woff2"))).toBe(true);
  expect(paths.some((path) => path.endsWith("/NotoSansSC-full.woff2"))).toBe(false);
  const keys = await page.evaluate(() => Object.keys(localStorage));
  expect(keys.every((key) => key === "oo.pref.theme" || key === "oo.pref.density" || key === "oo.pref.lang")).toBe(true);
});
