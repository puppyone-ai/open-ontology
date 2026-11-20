import { expect, test, type Page } from "@playwright/test";
import { stubBridgeSource } from "./stub-bridge.mjs";

const app = "http://127.0.0.1:5276/";
type Scenario = { engine: "healthy"; ask?: "answered" | "unanswerable" | "pending" | "no_model"; unlinked?: boolean };

async function open(page: Page, scenario: Scenario) {
  await page.clock.install({ time: new Date("2026-11-12T10:00:00Z") });
  await page.addInitScript(stubBridgeSource(scenario));
  await page.goto(app);
  await page.waitForSelector("[data-testid=app-root]");
  await page.clock.resume();
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(600);
}

async function ask(page: Page, question: string) {
  await page.locator("[data-testid=nav-ontology]").click();
  await page.locator("[data-testid=ontology-tab-ask]").click();
  await page.locator("[data-testid=ask-input]").fill(question);
  await page.locator("[data-testid=ask-send]").click();
}

test.describe("ask", () => {
  test("an answer with its source chips and the equivalent command", async ({ page }) => {
    await open(page, { engine: "healthy", ask: "answered" });
    await ask(page, "Who owns Project Alpha, and what is its 2026 budget?");
    await page.locator("[data-testid=ask-answer]").waitFor();
    await page.locator("[data-testid=ask-cli-toggle]").click();
    await expect(page).toHaveScreenshot("ask-answer.png");
  });
  test("the data has no answer: said plainly, with what was searched", async ({ page }) => {
    await open(page, { engine: "healthy", ask: "unanswerable" });
    await ask(page, "What is Dana Lee's email address?");
    await page.locator("[data-testid=ask-unanswerable]").waitFor();
    await expect(page).toHaveScreenshot("ask-unanswerable.png");
  });
  test("progress: each step as a quiet line while the answer is pending", async ({ page }) => {
    await open(page, { engine: "healthy", ask: "pending" });
    await ask(page, "Who is Evan Lin?");
    await page.locator("[data-testid=ask-progress]").waitFor();
    const askId = await page.evaluate(() => (window as unknown as { __askId: string }).__askId);
    const push = (payload: object) => page.evaluate((p) => (window as unknown as { __push: (x: unknown) => void }).__push({ channel: "ask-progress", payload: p }), { askId, type: "step", ...payload });
    await push({ n: 1, tool: "search", arg: "Evan Lin", state: "done" });
    await push({ n: 2, tool: "search", arg: "林远舟", state: "done" });
    await push({ n: 3, tool: "entity", arg: "oo:doc/d1/person:evan-lin", state: "running" });
    await page.locator("[data-testid=ask-step]").nth(2).waitFor();
    await expect(page).toHaveScreenshot("ask-progress.png");
  });
  test("no model: a calm inline state with the next step", async ({ page }) => {
    await open(page, { engine: "healthy", ask: "no_model" });
    await ask(page, "Who is Evan Lin?");
    await page.locator("[data-testid=ask-error]").waitFor();
    await expect(page).toHaveScreenshot("ask-no-model.png");
  });
});

test.describe("not connected", () => {
  test("the filter lists what has nothing connected, each with its reason", async ({ page }) => {
    await open(page, { engine: "healthy", unlinked: true });
    await page.locator("[data-testid=nav-ontology]").click();
    await page.locator("[data-testid=ontology-tab-atlas]").click();
    const filter = page.locator("[data-testid=atlas-unlinked-filter]");
    await filter.waitFor();
    await filter.click();
    await page.locator("[data-testid=atlas-list-member]").first().waitFor();
    await expect(page).toHaveScreenshot("not-connected.png");
  });
});
