import { expect, test, type Page } from "@playwright/test";
import { stubBridgeSource } from "./stub-bridge.mjs";

const app = "http://127.0.0.1:5276/";
type Scenario = { engine: "healthy" | "starting" | "crashed"; firstRun?: boolean; filled?: boolean; language?: string };

async function open(page: Page, scenario: Scenario) {
  await page.clock.install({ time: new Date("2026-11-12T10:00:00Z") });
  await page.addInitScript(stubBridgeSource(scenario));
  await page.goto(app);
  await page.waitForSelector("[data-testid=app-root]");
  await page.clock.resume();
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(600);
}

test.describe("screens", () => {
  test("home", async ({ page }) => {
    await open(page, { engine: "healthy" });
    await page.locator("[data-testid=home-count-entities][data-count]").waitFor();
    await expect(page).toHaveScreenshot("home.png");
  });
  test("sources", async ({ page }) => {
    await open(page, { engine: "healthy" });
    await page.locator("[data-testid=nav-sources]").click();
    await page.locator("[data-testid=screen-sources]").waitFor();
    await expect(page).toHaveScreenshot("sources.png");
  });
  test("ontology entities", async ({ page }) => {
    await open(page, { engine: "healthy" });
    await page.locator("[data-testid=nav-ontology]").click();
    await page.locator("[data-testid=entity-list]").waitFor();
    await expect(page).toHaveScreenshot("entities.png");
  });
  for (const language of ["en", "zh-Hans"] as const) {
    test(`entity page: normalised values with the text as written (${language})`, async ({ page }) => {
      await open(page, { engine: "healthy", filled: true, language });
      await page.locator("[data-testid=nav-ontology]").click();
      await page.locator("[data-testid=entity-row][data-name=Atlas]").click();
      await page.locator("[data-testid=screen-entity][data-name=Atlas]").waitFor();
      const budget = page.locator("[data-testid=fact-row][data-predicate='attr.budget']");
      await expect(budget.locator("[data-testid=fact-value]")).toHaveText("6,850,000 CNY");
      await expect(budget.locator("[data-testid=fact-value-original]")).toContainText("六百八十五万元");
      await expect(page.locator("[data-testid=fact-row][data-predicate='attr.yield'] [data-testid=fact-value]")).toHaveText("88%");
      // A value key this app does not know still shows its text.
      await expect(page.locator("[data-testid=fact-row][data-predicate='attr.review_window'] [data-testid=fact-value]")).toHaveText("P2W");
      await expect(page).toHaveScreenshot(`entity-values-${language}.png`);
    });
  }
  test("entity page: an open Ratio thing is named by its percentage and lists how files wrote it", async ({ page }) => {
    await open(page, { engine: "healthy", filled: true });
    await page.locator("[data-testid=nav-ontology]").click();
    await page.locator("[data-testid=entity-row][data-name='88%']").click();
    await page.locator("[data-testid=screen-entity]").waitFor();
    await expect(page.locator("h1")).toHaveText("88%");
    await expect(page.locator(".so-chip", { hasText: "百分之八十八" })).toBeVisible();
    await expect(page).toHaveScreenshot("entity-ratio.png");
  });
  test("first run: engine starting keeps Get started disabled, ready enables it", async ({ page }) => {
    await open(page, { engine: "starting", firstRun: true });
    await expect(page.locator("[data-testid=firstrun-start]")).toBeDisabled();
    await expect(page).toHaveScreenshot("first-run-starting.png");
  });
  test("first run: a failed engine start offers Try again and names the log file", async ({ page }) => {
    await open(page, { engine: "crashed", firstRun: true });
    await expect(page.locator("[data-testid=firstrun-engine-retry]")).toBeVisible();
    await expect(page.locator("[data-testid=firstrun-engine-log]")).toContainText("super-ontology-serve.log");
    await expect(page.locator("[data-testid=firstrun-start]")).toBeDisabled();
    await expect(page).toHaveScreenshot("first-run-engine-failed.png");
  });
  test("first run: ready", async ({ page }) => {
    await open(page, { engine: "healthy", firstRun: true });
    await expect(page.locator("[data-testid=firstrun-start]")).toBeEnabled();
    await expect(page).toHaveScreenshot("first-run-ready.png");
  });
});
