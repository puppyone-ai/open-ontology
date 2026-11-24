import { expect, test, type Page } from "@playwright/test";
import { stubBridgeSource } from "./stub-bridge.mjs";

// The CLI parity screens (D88): Add database, a database page, Query, ontology files, Diagnostics. Same stub bridge and tolerances as screens.spec.ts.
const app = "http://127.0.0.1:5276/";
type Scenario = { engine: "healthy"; databases?: boolean; gateway?: boolean };
const withDb: Scenario = { engine: "healthy", databases: true };

async function open(page: Page, scenario: Scenario) {
  await page.clock.install({ time: new Date("2026-11-12T10:00:00Z") });
  await page.addInitScript(stubBridgeSource(scenario));
  await page.goto(app);
  await page.waitForSelector("[data-testid=app-root]");
  await page.clock.resume();
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(500);
}
const go = async (page: Page, id: string) => { await page.locator(`[data-testid=${id}]`).click(); };

test.describe("CLI parity screens", () => {
  test("sources lists a database", async ({ page }) => {
    await open(page, withDb);
    await go(page, "nav-sources");
    await page.locator("[data-testid=source-row][data-kind=database]").waitFor();
    await expect(page).toHaveScreenshot("sources-with-database.png");
  });
  test("add database dialog, empty and filled with a passed check", async ({ page }) => {
    await open(page, withDb);
    await go(page, "nav-sources");
    await go(page, "add-source");
    await go(page, "add-source-database");
    await page.locator("[data-testid=add-database-name]").waitFor();
    await expect(page).toHaveScreenshot("add-database-empty.png");
    await page.locator("[data-testid=add-database-name]").fill("shop");
    await page.locator("[data-testid=add-database-host]").fill("db.example.test");
    await page.locator("[data-testid=add-database-database]").fill("shop");
    await page.locator("[data-testid=add-database-user]").fill("reader");
    await page.locator("[data-testid=add-database-password]").fill("not-a-real-password");
    await page.locator("[data-testid=add-database-ca-pem]").fill("-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----");
    await page.locator("[data-testid=add-database-include]").fill("public.customers\npublic.orders");
    await go(page, "add-database-test");
    await page.locator("[data-testid=add-database-check-ok]").waitFor();
    await expect(page).toHaveScreenshot("add-database-checked.png");
  });
  test("add database dialog shows what is missing", async ({ page }) => {
    await open(page, withDb);
    await go(page, "nav-sources");
    await go(page, "add-source");
    await go(page, "add-source-database");
    await go(page, "add-database-confirm");
    await expect(page.locator(".so-dialog .so-field-error").first()).toBeVisible();
    await expect(page).toHaveScreenshot("add-database-errors.png");
  });
  test("database page with the doctor result", async ({ page }) => {
    await open(page, withDb);
    await go(page, "nav-sources");
    await page.locator("[data-testid=source-row][data-kind=database]").click();
    await page.locator("[data-testid=screen-database-detail]").waitFor();
    await go(page, "database-doctor");
    await page.locator("[data-testid=database-doctor-result]").waitFor();
    await expect(page).toHaveScreenshot("database-detail.png", { fullPage: true });
  });
  test("query result", async ({ page }) => {
    await open(page, withDb);
    await go(page, "nav-ontology");
    await go(page, "ontology-tab-query");
    await page.locator("[data-testid=query-sql-input]").fill("select * from public.customers");
    await go(page, "query-run");
    await page.locator("[data-testid=query-result]").waitFor();
    await expect(page).toHaveScreenshot("query-result.png");
  });
  test("query without a Gateway asks for one", async ({ page }) => {
    await open(page, { ...withDb, gateway: false });
    await go(page, "nav-ontology");
    await go(page, "ontology-tab-query");
    await page.locator("[data-testid=query-gateway][data-state=disconnected]").waitFor();
    await expect(page).toHaveScreenshot("query-no-gateway.png");
  });
  test("tables and describe", async ({ page }) => {
    await open(page, withDb);
    await go(page, "nav-ontology");
    await go(page, "ontology-tab-query");
    await go(page, "query-mode-tables");
    await page.locator("[data-testid=query-table-row]").first().click();
    await page.locator("[data-testid=query-describe] table").waitFor();
    await expect(page).toHaveScreenshot("query-tables.png");
  });
  test("ontology files: YAML view and version diff", async ({ page }) => {
    await open(page, withDb);
    await go(page, "nav-ontology");
    await go(page, "ontology-tab-files");
    await page.locator("[data-testid=ontology-yaml]").waitFor();
    await expect(page).toHaveScreenshot("ontology-files.png");
    await page.locator("[data-testid=ontology-file-compare]").selectOption({ index: 1 });
    await page.locator("[data-testid=ontology-diff]").waitFor();
    await expect(page).toHaveScreenshot("ontology-files-diff.png");
  });
  test("diagnostics", async ({ page }) => {
    await open(page, withDb);
    await go(page, "nav-settings");
    await go(page, "settings-section-diagnostics");
    await page.locator("[data-testid=diag-oo]").waitFor();
    await expect(page).toHaveScreenshot("diagnostics.png");
  });
  test("an equivalent command opens and is copyable text", async ({ page }) => {
    await open(page, withDb);
    await go(page, "nav-ontology");
    await go(page, "ontology-tab-query");
    await page.locator("[data-testid=query-sql-input]").fill("select 1");
    await go(page, "query-run");
    await page.locator("[data-testid=query-cli-toggle]").click();
    await expect(page.locator("[data-testid=query-cli-value]")).toContainText("oo query");
  });
});
