import fs from "node:fs";
import path from "node:path";
import { defineConfig } from "@playwright/test";

// Playwright wants its own pinned browser build; any installed headless Chromium renders these pages the same on one machine.
function findBrowser(): string | undefined {
  if (process.env.SO_CHROMIUM) return process.env.SO_CHROMIUM;
  const cache = path.join(process.env.HOME ?? "", "Library/Caches/ms-playwright");
  const shells = fs.existsSync(cache) ? fs.readdirSync(cache).filter((name) => name.startsWith("chromium_headless_shell-")).sort().reverse() : [];
  for (const shell of shells) {
    const candidate = path.join(cache, shell, "chrome-headless-shell-mac-arm64", "chrome-headless-shell");
    if (fs.existsSync(candidate)) return candidate;
  }
  return undefined;
}

// Light theme only (D87). Baselines are per platform (the snapshot name carries it); `pnpm test:visual -- --update-snapshots` rewrites them.
export default defineConfig({
  testDir: path.resolve(import.meta.dirname, "../tests/visual"),
  testMatch: "*.spec.ts",
  timeout: 60_000,
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
  snapshotPathTemplate: "{testDir}/__screenshots__/{projectName}/{testFileName}/{arg}{ext}",
  expect: { toHaveScreenshot: { animations: "disabled", caret: "hide", maxDiffPixelRatio: 0.002, scale: "css" } },
  use: { viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2, colorScheme: "light", locale: "en-US", launchOptions: { executablePath: findBrowser() } },
  projects: [{ name: "light" }],
  webServer: [
    { command: "pnpm exec vite --config config/vite.visual.config.ts", url: "http://127.0.0.1:5275/", reuseExistingServer: true, timeout: 60_000, cwd: path.resolve(import.meta.dirname, "..") },
    // The production build of the renderer, served as static files (screens.spec.ts installs a stub bridge before the page loads).
    { command: "pnpm run build && pnpm exec vite preview --host 127.0.0.1 --port 5276 --strictPort", url: "http://127.0.0.1:5276/", reuseExistingServer: true, timeout: 120_000, cwd: path.resolve(import.meta.dirname, "..") },
  ],
});
