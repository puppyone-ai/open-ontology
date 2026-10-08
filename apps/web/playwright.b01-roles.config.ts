import { defineConfig } from "@playwright/test";

const evidence = process.env.FE01_EVIDENCE;
const origin = process.env.FE01_B01_ORIGIN;
if (!evidence || !origin) {
  throw new Error("FE01_B01_ORIGIN and FE01_EVIDENCE are required");
}

export default defineConfig({
  testDir: "./design/e2e",
  testMatch: "b01.roles.spec.ts",
  workers: 1,
  retries: 0,
  timeout: 180_000,
  use: {
    baseURL: origin,
    locale: "zh-CN",
    timezoneId: "Asia/Shanghai",
    ignoreHTTPSErrors: false,
    launchOptions: {
      args: ["--no-sandbox", "--disable-dev-shm-usage"],
    },
  },
});
