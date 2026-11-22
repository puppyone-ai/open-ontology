import { expect, test, type Page } from "@playwright/test";
import { stubBridgeSource } from "./stub-bridge.mjs";

const app = "http://127.0.0.1:5276/";
type Scenario = { engine: "healthy"; speakers: true; live?: true; mediaClass?: "conversation" | "non_conversation" };

async function open(page: Page, scenario: Scenario) {
  await page.clock.install({ time: new Date("2026-11-12T10:00:00Z") });
  await page.addInitScript(stubBridgeSource(scenario));
  // The headless shell has no media stack; the playback itself is checked in the T1 scenario (speakers.t1.mjs). The baselines show the player at rest.
  await page.addInitScript(() => window.addEventListener("error", (event) => { if (event.target instanceof HTMLMediaElement) event.stopImmediatePropagation(); }, true));
  await page.goto(app);
  await page.waitForSelector("[data-testid=app-root]");
  await page.clock.resume();
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(400);
}

async function openReview(page: Page) {
  await open(page, { engine: "healthy", speakers: true, live: true });
  await page.locator("[data-testid=nav-sources]").click();
  await page.locator("[data-testid=source-row]").first().waitFor();
  await page.locator("[data-testid=nav-ontology]").click();
  await page.locator("[data-testid=ontology-tab-review]").click();
  await page.locator("[data-testid=speaker-group]").waitFor();
  await page.locator("[data-testid=speaker-clip]").first().waitFor();
}

test.describe("naming speakers beside a player", () => {
  test("review lists Speaker A and Speaker B with clips, a name field and the preview pane", async ({ page }) => {
    await openReview(page);
    await expect(page.locator("[data-testid=speaker-row]")).toHaveCount(2);
    await expect(page.locator("[data-testid=speaker-row]").first()).toContainText("Speaker A");
    await expect(page.locator("[data-testid=speaker-clip-text]").first()).toContainText("Good morning everyone");
    await expect(page.locator("[data-testid=media-utterance]")).toHaveCount(4);
    await expect(page).toHaveScreenshot("review-speakers.png");
  });

  test("the preview pane and the second speaker's clips", async ({ page }) => {
    await openReview(page);
    await expect(page.locator("[data-testid=review-pane]")).toHaveScreenshot("review-pane.png");
    await expect(page.locator("[data-testid=speaker-row]").nth(1)).toHaveScreenshot("speaker-row-b.png");
  });

  test("keyboard focus on a clip's play button and on a transcript line keep a visible ring", async ({ page }) => {
    await openReview(page);
    await page.locator("[data-testid=speaker-clip-play]").first().focus();
    await expect(page.locator("[data-testid=speaker-row]").first()).toHaveScreenshot("speaker-row-focus.png");
    await page.locator("[data-testid=media-utterance]").nth(1).focus();
    await expect(page.locator("[data-testid=media-transcript]")).toHaveScreenshot("transcript-focus.png");
  });
});

test.describe("the class of a recording in the file panel", () => {
  for (const mediaClass of ["conversation", "non_conversation"] as const) {
    test(`file panel: ${mediaClass}`, async ({ page }) => {
      await open(page, { engine: "healthy", speakers: true, live: true, mediaClass });
      await page.locator("[data-testid=nav-sources]").click();
      await page.locator("[data-testid=source-row]").first().click();
      await page.locator("[data-testid=file-row]").first().click();
      await page.locator("[data-testid=file-media]").waitFor();
      await page.locator("[data-testid=media-class]").waitFor();
      await expect(page.locator("[data-testid=media-class]")).toHaveAttribute("data-class", mediaClass);
      await expect(page.locator("[data-testid=file-detail-panel]")).toHaveScreenshot(`file-panel-${mediaClass}.png`);
    });
  }
});
