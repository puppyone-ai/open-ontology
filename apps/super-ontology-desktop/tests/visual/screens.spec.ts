import { expect, test, type Page } from "@playwright/test";
import { stubBridgeSource } from "./stub-bridge.mjs";

const app = "http://127.0.0.1:5276/";
type Scenario = { engine: "healthy" | "starting"; firstRun?: boolean; databases?: boolean };

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
  test("sources with a database", async ({ page }) => {
    await open(page, { engine: "healthy", databases: true });
    await page.locator("[data-testid=nav-sources]").click();
    await page.locator("[data-testid=db-source-row]").waitFor();
    await expect(page).toHaveScreenshot("sources.png");
  });
  test("database dialog: empty, then what is missing", async ({ page }) => {
    await open(page, { engine: "healthy" });
    await page.locator("[data-testid=nav-sources]").click();
    await page.locator("[data-testid=add-source]").first().click();
    await page.locator("[data-testid=add-source-database]").click();
    await page.locator("[data-testid=database-dialog]").waitFor();
    await expect(page).toHaveScreenshot("database-dialog.png");
    await page.locator("[data-testid=add-source-confirm]").click();
    await expect(page).toHaveScreenshot("database-dialog-required.png");
  });
  test("ontology entities", async ({ page }) => {
    await open(page, { engine: "healthy" });
    await page.locator("[data-testid=nav-ontology]").click();
    await page.locator("[data-testid=entity-list]").waitFor();
    await expect(page).toHaveScreenshot("entities.png");
  });
  test("first run: engine starting keeps Get started disabled, ready enables it", async ({ page }) => {
    await open(page, { engine: "starting", firstRun: true });
    await expect(page.locator("[data-testid=firstrun-start]")).toBeDisabled();
    await expect(page).toHaveScreenshot("first-run-starting.png");
  });
  test("first run: ready", async ({ page }) => {
    await open(page, { engine: "healthy", firstRun: true });
    await expect(page.locator("[data-testid=firstrun-start]")).toBeEnabled();
    await expect(page).toHaveScreenshot("first-run-ready.png");
  });
});
