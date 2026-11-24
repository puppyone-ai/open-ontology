import fs from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { digestReview } from "../../src/shared/super-ontology/review-digest.mjs";
import { stubBridgeSource } from "./stub-bridge.mjs";

// The build review and the import stages (D88): what the Builder proposed, accept or reject each relationship, then the build goes on.
// The draft is a real Builder answer for a synthetic four-table shop (4 tables, 3 foreign keys), recorded from `oo build`.
const app = "http://127.0.0.1:5276/";
// Tall enough to hold the whole review: a page screenshot then shows every row. SO_AUDIT_DIR / SO_AUDIT_SCALE write review screenshots at 1x and 2x instead of comparing.
const scale = Number(process.env.SO_AUDIT_SCALE ?? 2);
test.use({ viewport: { width: 1280, height: 1500 }, deviceScaleFactor: scale });
async function snap(page: Page, name: string, options: { fullPage?: boolean } = {}) {
  const dir = process.env.SO_AUDIT_DIR;
  if (!dir) { await expect(page).toHaveScreenshot(name, options); return; }
  fs.mkdirSync(dir, { recursive: true });
  await page.screenshot({ path: path.join(dir, `${name.replace(/\.png$/u, "")}@${scale}x.png`), ...options });
}
const recorded = path.resolve(import.meta.dirname, "../support/recorded");
const digest = digestReview(JSON.parse(fs.readFileSync(path.join(recorded, "adjudication.json"), "utf8")), JSON.parse(fs.readFileSync(path.join(recorded, "review-input.json"), "utf8")));

type Scenario = { engine: "healthy"; databases: true; review?: "pending" | "done" | "none"; job?: "review" | "failed" | "running" | "up-running"; digest?: unknown; submitFails?: boolean; resume?: "running" };
const base: Scenario = { engine: "healthy", databases: true, review: "none", job: "review", digest };

async function open(page: Page, scenario: Scenario) {
  await page.clock.install({ time: new Date("2026-11-12T10:00:00Z") });
  await page.addInitScript(stubBridgeSource(scenario));
  await page.goto(app);
  await page.waitForSelector("[data-testid=app-root]");
  await page.clock.resume();
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(400);
}
const go = async (page: Page, id: string) => { await page.locator(`[data-testid=${id}]`).click(); };
async function openDatabase(page: Page) {
  await go(page, "nav-sources");
  await page.locator("[data-testid=source-row][data-kind=database]").click();
  await page.locator("[data-testid=screen-database-detail]").waitFor();
}
/** Import data: services start, the build runs, and it stops for the owner's review. */
async function importUntilReview(page: Page) {
  await go(page, "database-import");
  await page.locator("[data-testid=build-review]").waitFor({ timeout: 15_000 });
}

test.describe("build review", () => {
  test("shows what the Builder proposed", async ({ page }) => {
    await open(page, base);
    await openDatabase(page);
    await importUntilReview(page);
    await expect(page.locator("[data-testid=review-relation]")).toHaveCount(9);
    await expect(page.locator("[data-testid=review-stat-tables]")).toContainText("4");
    await snap(page, "build-review.png", { fullPage: true });
  });

  test("a changed decision is marked and counted", async ({ page }) => {
    await open(page, base);
    await openDatabase(page);
    await importUntilReview(page);
    const found = page.locator("[data-testid=review-relation][data-origin=discovered]").first();
    await found.locator("[data-testid=review-accept]").click();
    await expect(found).toHaveAttribute("data-decision", "adopted");
    await expect(page.locator("[data-testid=review-changes]")).toHaveAttribute("data-count", "1");
    const declared = page.locator("[data-testid=review-relation][data-origin=declared]").first();
    await declared.locator("[data-testid=review-reject]").click();
    await expect(page.locator("[data-testid=review-changes]")).toHaveAttribute("data-count", "2");
    await page.locator("[data-testid=review-confirm]").scrollIntoViewIfNeeded();
    await snap(page, "build-review-changed.png", { fullPage: true });
  });

  test("an empty reason is refused before anything is written", async ({ page }) => {
    await open(page, base);
    await openDatabase(page);
    await importUntilReview(page);
    await page.locator("[data-testid=review-reason]").fill("");
    await go(page, "review-confirm");
    await expect(page.locator("[data-testid=build-review] .so-field-error")).toBeVisible();
    await expect(page.locator("[data-testid=build-review]")).toHaveAttribute("data-state", "ready");
  });

  test("a refused decision says what to do next", async ({ page }) => {
    await open(page, { ...base, submitFails: true });
    await openDatabase(page);
    await importUntilReview(page);
    await go(page, "review-confirm");
    await page.locator("[data-testid=review-error]").waitFor();
    await expect(page.locator("[data-testid=review-error-next]")).toBeVisible();
    await page.locator("[data-testid=review-error]").scrollIntoViewIfNeeded();
    await snap(page, "build-review-error.png", { fullPage: true });
  });

  test("leaving for later keeps the build waiting on the database page", async ({ page }) => {
    await open(page, base);
    await openDatabase(page);
    await importUntilReview(page);
    await go(page, "review-later");
    await page.locator("[data-testid=database-review-waiting]").waitFor();
    await expect(page.locator("[data-testid=build-stage][data-stage=self_review]")).toHaveAttribute("data-state", "review");
    await snap(page, "database-review-waiting.png", { fullPage: true });
  });

  test("a build left waiting is found again when the page opens", async ({ page }) => {
    await open(page, { ...base, review: "pending" });
    await openDatabase(page);
    await page.locator("[data-testid=database-review-waiting]").waitFor();
    await go(page, "database-review-open");
    await page.locator("[data-testid=build-review]").waitFor();
  });

  test("confirming resumes the build and ends with links to the atlas and the files", async ({ page }) => {
    await open(page, base);
    await openDatabase(page);
    await importUntilReview(page);
    await go(page, "review-confirm");
    await page.locator("[data-testid=database-import-done]").waitFor({ timeout: 15_000 });
    for (const stage of ["load", "candidate", "builder", "self_review", "publish", "gateway", "first_token", "share_prompt"]) await expect(page.locator(`[data-testid=build-stage][data-stage=${stage}]`)).toHaveAttribute("data-state", "complete");
    await expect(page.locator("[data-testid=database-open-atlas]")).toBeVisible();
    await expect(page.locator("[data-testid=database-open-files]")).toBeVisible();
    await snap(page, "database-import-done.png", { fullPage: true });
  });

  test("progress shows each stage while the build runs", async ({ page }) => {
    await open(page, { ...base, job: "running" });
    await openDatabase(page);
    await go(page, "database-import");
    await page.locator("[data-testid=build-stages]").waitFor();
    await page.locator("[data-testid=build-stage][data-stage=builder]").waitFor();
    await expect(page.locator("[data-testid=build-stage][data-stage=builder]")).toHaveAttribute("data-state", "running");
    await snap(page, "database-import-running.png", { fullPage: true });
  });

  test("a failed stage names its cause and the next step", async ({ page }) => {
    await open(page, { ...base, job: "failed" });
    await openDatabase(page);
    await go(page, "database-import");
    await page.locator("[data-testid=database-import-error]").waitFor({ timeout: 15_000 });
    await expect(page.locator("[data-testid=database-import-hint]")).toContainText("host.docker.internal");
    await expect(page.locator("[data-testid=build-stage][data-stage=load]")).toHaveAttribute("data-state", "blocked");
    await snap(page, "database-import-failed.png", { fullPage: true });
  });
});

test.describe("add database, kinds that were not wired", () => {
  test.use({ viewport: { width: 1280, height: 800 } });
  const pick = async (page: Page, kind: string) => {
    await go(page, "nav-sources");
    await go(page, "add-source");
    await go(page, "add-source-database");
    await page.locator("[data-testid=add-database-name]").waitFor();
    await page.locator("[data-testid=add-database-kind]").selectOption(kind);
  };
  test("MongoDB asks for a certificate unless the host is this machine", async ({ page }) => {
    await open(page, base);
    await pick(page, "mongodb");
    await expect(page.locator("[data-testid=add-database-ca]")).toHaveAttribute("data-need", "required");
    await page.locator("[data-testid=add-database-host]").fill("127.0.0.1");
    await expect(page.locator("[data-testid=add-database-ca]")).toHaveAttribute("data-need", "optional");
    await page.locator("[data-testid=add-database-host]").fill("");
    await expect(page.locator("[data-testid=add-database-port]")).toHaveAttribute("placeholder", "27017");
    await expect(page.locator("[data-testid=add-database-include]")).toHaveAttribute("placeholder", /shop\.customers/u);
    await snap(page, "add-database-mongodb.png");
  });
  test("Oracle and DM8 take no certificate and say why", async ({ page }) => {
    await open(page, base);
    await pick(page, "dm8");
    await expect(page.locator("[data-testid=add-database-ca-driver]")).toBeVisible();
    await expect(page.locator("[data-testid=add-database-ca-pem]")).toHaveCount(0);
    await snap(page, "add-database-dm8.png");
  });
  test("every one of the seventeen kinds can be picked", async ({ page }) => {
    await open(page, base);
    await go(page, "nav-sources");
    await go(page, "add-source");
    await go(page, "add-source-database");
    await expect(page.locator("[data-testid=add-database-kind] option")).toHaveCount(17);
    await expect(page.locator("[data-testid=add-database-kind] option:disabled")).toHaveCount(0);
  });
});
