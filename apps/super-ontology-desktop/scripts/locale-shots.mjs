#!/usr/bin/env node
/* global document */
// Review screenshots of the main screens in other languages (1x), from the production build served by `vite preview` (port 5276) with the
// stub bridge the visual tests use. Look for truncation, overflow and alignment: German and Japanese are the long and the tall cases.
//   pnpm run build && pnpm exec vite preview --host 127.0.0.1 --port 5276 --strictPort &
//   node scripts/locale-shots.mjs <out-dir> [zh-Hans ja de ...]
import { chromium } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { stubBridgeSource } from "../tests/visual/stub-bridge.mjs";

const [outDir, ...languages] = process.argv.slice(2);
if (!outDir) { console.error("usage: locale-shots.mjs <out-dir> [locale ...]"); process.exit(2); }
fs.mkdirSync(outDir, { recursive: true });
const app = process.env.SO_URL ?? "http://127.0.0.1:5276/";
function findBrowser() {
  if (process.env.SO_CHROMIUM) return process.env.SO_CHROMIUM;
  const cache = path.join(process.env.HOME ?? "", "Library/Caches/ms-playwright");
  const shells = fs.existsSync(cache) ? fs.readdirSync(cache).filter((name) => name.startsWith("chromium_headless_shell-")).sort().reverse() : [];
  for (const shell of shells) { const candidate = path.join(cache, shell, "chrome-headless-shell-mac-arm64", "chrome-headless-shell"); if (fs.existsSync(candidate)) return candidate; }
  return undefined;
}
const click = async (page, id) => { await page.locator(`[data-testid=${id}]`).first().click(); await page.waitForTimeout(500); };
const screens = [
  ["home", async () => {}, "home-count-entities"],
  ["sources", async (page) => click(page, "nav-sources"), "screen-sources"],
  ["ontology-atlas", async (page) => { await click(page, "nav-ontology"); await click(page, "ontology-tab-atlas"); await page.waitForTimeout(1500); }, "atlas"],
  ["ontology-atlas-list", async (page) => { await click(page, "nav-ontology"); await click(page, "ontology-tab-atlas"); await click(page, "atlas-view-list"); }, "atlas-list"],
  ["ontology-entities", async (page) => click(page, "nav-ontology"), "entity-list"],
  ["agents", async (page) => click(page, "nav-agents"), null],
  ["settings", async (page) => click(page, "nav-settings"), null],
];
const browser = await chromium.launch({ executablePath: findBrowser() });
try {
  for (const language of languages.length ? languages : ["zh-Hans", "ja", "de"]) {
    for (const [name, go, waitFor] of screens) {
      const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1, colorScheme: "light", locale: language === "zh-Hans" ? "zh-CN" : language });
      const page = await context.newPage();
      await page.clock.install({ time: new Date("2026-11-12T10:00:00Z") });
      await page.addInitScript(stubBridgeSource({ engine: "healthy", language }));
      await page.goto(app);
      await page.waitForSelector("[data-testid=app-root]");
      await page.clock.resume();
      await page.evaluate(() => document.fonts.ready);
      await page.waitForTimeout(600);
      await go(page);
      if (waitFor) await page.locator(`[data-testid=${waitFor}]`).first().waitFor({ timeout: 8000 }).catch(() => console.warn(`${language}/${name}: ${waitFor} not found`));
      await page.waitForTimeout(500);
      await page.screenshot({ path: path.join(outDir, `${name}-${language}@1x.png`) });
      console.log(`${name}-${language}@1x`);
      await context.close();
    }
  }
} finally { await browser.close(); }
