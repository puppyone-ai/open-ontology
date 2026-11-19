#!/usr/bin/env node
// Review screenshots at 1x and 2x of the running dev renderer (vite dev server + dev bridge).
//   node scripts/ui-shots.mjs <out-dir> <name>=<steps> ...
// A step list is `;`-separated: `click:<css or text=...>`, `fill:<selector>|<text>`, `wait:<ms>`, `press:<key>`, `shot` (default at the end).
import { chromium } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

const [outDir, ...specs] = process.argv.slice(2);
if (!outDir || specs.length === 0) { console.error("usage: ui-shots.mjs <out-dir> name=steps ..."); process.exit(2); }
fs.mkdirSync(outDir, { recursive: true });
const base = process.env.SO_URL ?? "http://127.0.0.1:5273/";
// Playwright wants its own pinned browser build; any installed headless shell renders these pages the same.
function findBrowser() {
  if (process.env.SO_CHROMIUM) return process.env.SO_CHROMIUM;
  const cache = path.join(process.env.HOME ?? "", "Library/Caches/ms-playwright");
  const shells = fs.existsSync(cache) ? fs.readdirSync(cache).filter((name) => name.startsWith("chromium_headless_shell-")).sort().reverse() : [];
  for (const shell of shells) {
    const candidate = path.join(cache, shell, "chrome-headless-shell-mac-arm64", "chrome-headless-shell");
    if (fs.existsSync(candidate)) return candidate;
  }
  return undefined;
}
const browser = await chromium.launch({ executablePath: findBrowser() });
try {
  for (const scale of [1, 2]) {
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: scale, colorScheme: "light", locale: process.env.SO_LOCALE ?? "en-US" });
    const page = await context.newPage();
    await page.addInitScript(() => { if (!window.localStorage.getItem("so.firstRun.done") && !window.sessionStorage.getItem("so.shot.firstRun")) window.localStorage.setItem("so.firstRun.done", "1"); });
    for (const spec of specs) {
      const [name, steps = ""] = spec.split("=");
      await page.goto(base);
      await page.waitForSelector("[data-testid=app-root]");
      await page.waitForTimeout(800);
      for (const step of steps.split(";").filter(Boolean)) {
        const [kind, ...rest] = step.split(":");
        const arg = rest.join(":");
        if (kind === "click") await page.locator(arg).first().click();
        else if (kind === "fill") { const [selector, text] = arg.split("|"); await page.locator(selector).first().fill(text); }
        else if (kind === "press") await page.keyboard.press(arg);
        else if (kind === "wait") await page.waitForTimeout(Number(arg));
        else if (kind === "hover") await page.locator(arg).first().hover();
        await page.waitForTimeout(250);
      }
      await page.screenshot({ path: path.join(outDir, `${name}@${scale}x.png`) });
      console.log(`${name}@${scale}x`);
    }
    await context.close();
  }
} finally { await browser.close(); }
