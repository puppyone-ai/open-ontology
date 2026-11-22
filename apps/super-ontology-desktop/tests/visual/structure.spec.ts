import { expect, test, type Page } from "@playwright/test";
import { stubBridgeSource } from "./stub-bridge.mjs";

const app = "http://127.0.0.1:5276/";

async function open(page: Page, language?: string) {
  await page.clock.install({ time: new Date("2026-11-12T10:00:00Z") });
  await page.addInitScript(stubBridgeSource({ engine: "healthy", live: true, ...(language ? { language } : {}) }));
  await page.goto(app);
  await page.waitForSelector("[data-testid=app-root]");
  await page.clock.resume();
  await page.evaluate(() => document.fonts.ready);
  await page.locator("[data-testid=nav-ontology]").click();
  await page.locator("[data-testid=ontology-tab-structure]").click();
  await page.locator("[data-testid=structure-class]").first().waitFor();
  await page.waitForTimeout(400);
}

test.describe("structure", () => {
  test("the classes with their properties, types and coverage", async ({ page }) => {
    await open(page);
    await expect(page.locator("[data-testid=structure-class]")).toHaveCount(3);
    await expect(page.locator("[data-testid=structure-coverage]")).toHaveAttribute("data-confirmed", "14");
    await expect(page).toHaveScreenshot("structure.png");
  });

  test("confirming a class changes its status and the coverage", async ({ page }) => {
    await open(page);
    const meeting = page.locator("[data-testid=structure-class][data-class=meeting_minutes]");
    await meeting.locator("[data-testid=structure-confirm]").click();
    await expect(meeting).toHaveAttribute("data-status", "confirmed");
    await expect(page.locator("[data-testid=structure-coverage]")).toHaveAttribute("data-confirmed", "32");
    await expect(page).toHaveScreenshot("structure-confirmed.png");
  });

  test("renaming and merging a property", async ({ page }) => {
    await open(page);
    const row = page.locator("[data-testid=structure-class][data-class=meeting_minutes] [data-testid=structure-property][data-slug=attendees]");
    await row.getByRole("button").last().click();
    await page.locator("[data-testid=structure-rename]").click();
    await page.locator("[data-testid=structure-dialog] input").fill("Who came");
    await expect(page).toHaveScreenshot("structure-rename.png");
    await page.locator("[data-testid=structure-dialog-save]").click();
    await expect(row.locator(".so-list-title")).toHaveText("Who came");
    await page.locator("[data-testid=structure-class][data-class=meeting_minutes] [data-testid=structure-property][data-slug=agenda]").getByRole("button").last().click();
    await page.locator("[data-testid=structure-merge]").click();
    await expect(page.locator("[data-testid=structure-dialog] select")).toBeVisible();
    await page.locator("[data-testid=structure-dialog-save]").click();
    await expect(page.locator("[data-testid=structure-property][data-slug=agenda]")).toHaveCount(0);
  });

  test("the structure YAML is one of the ontology files", async ({ page }) => {
    await open(page);
    await page.locator("[data-testid=ontology-tab-files]").click();
    await page.locator("[data-testid=ontology-files]").waitFor();
    await page.getByRole("button", { name: "ontology-structure-My files.yaml" }).or(page.getByRole("button", { name: "ontology-structure.yaml" })).first().click();
    await expect(page.locator("[data-testid=ontology-yaml]")).toContainText("meeting_minutes");
  });

  for (const [language, word] of [["zh-Hans", "确认结构"], ["de", "Struktur bestätigen"]] as const) {
    test(`the tab speaks ${language}`, async ({ page }) => {
      await page.setViewportSize({ width: 1280, height: 1400 });
      await open(page, language);
      await expect(page.locator("[data-testid=structure-confirm]").first()).toHaveText(word);
      await expect(page).toHaveScreenshot(`structure-${language}.png`);
    });
  }
});
