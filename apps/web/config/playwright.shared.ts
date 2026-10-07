import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "@playwright/test";

const webRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
export const evidenceDir = process.env.FE01_EVIDENCE;
if (!evidenceDir) {
  throw new Error("FE01_EVIDENCE is required");
}

export function playwrightConfig(testMatch: string) {
  return defineConfig({
    testDir: "../design/e2e",
    testMatch,
    fullyParallel: false,
    workers: 1,
    retries: 0,
    timeout: 60_000,
    use: {
      baseURL: "http://127.0.0.1:4173",
      locale: "zh-CN",
      timezoneId: "Asia/Shanghai",
      launchOptions: {
        args: ["--no-sandbox", "--disable-dev-shm-usage"],
      },
    },
    webServer: {
      command: "pnpm exec vite preview --host 127.0.0.1 --port 4173 --strictPort",
      cwd: webRoot,
      port: 4173,
      reuseExistingServer: false,
      timeout: 120_000,
    },
  });
}
