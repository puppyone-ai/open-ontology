import { expect, test, type Page } from "@playwright/test";
import { stubBridgeSource } from "./stub-bridge.mjs";

// The window-wide drop overlay in each state (25 §4, D86), and what a drop leaves behind: the toast and the upload list.
const app = "http://127.0.0.1:5276/";

async function open(page: Page, scenario: Record<string, unknown> = {}) {
  // `live`: the stub answers the event subscription completely, so the Sources list is filled the way a running engine fills it.
  scenario = { engine: "healthy", live: true, ...scenario };
  await page.clock.install({ time: new Date("2026-11-12T10:00:00Z") });
  await page.addInitScript(stubBridgeSource(scenario));
  await page.goto(app);
  await page.waitForSelector("[data-testid=app-root]");
  await page.clock.resume();
  await page.evaluate(() => document.fonts.ready);
  await page.locator("[data-testid=nav-sources]").click();
  await page.locator("[data-testid=source-list]").waitFor();
  await page.waitForTimeout(400);
}

/** A real DragEvent carrying files, dispatched at the element (the browser builds the same event for a Finder drag). */
async function drag(page: Page, selector: string, type: "dragenter" | "dragover" | "drop", names: string[]) {
  await page.evaluate(({ selector: css, type: kind, names: list }) => {
    const transfer = new DataTransfer();
    for (const name of list) transfer.items.add(new File(["x"], name));
    document.querySelector(css)!.dispatchEvent(new DragEvent(kind, { bubbles: true, cancelable: true, dataTransfer: transfer }));
  }, { selector, type, names });
}

test.describe("drop overlay", () => {
  test("over the Sources list: My files by default, with the count", async ({ page }) => {
    await open(page);
    await drag(page, "[data-testid=screen-sources]", "dragenter", ["a.md", "b.md", "ProjectA"]);
    await page.locator("[data-testid=drop-overlay][data-target=my-files]").waitFor();
    await page.waitForTimeout(300);
    await expect(page).toHaveScreenshot("drop-overlay-my-files.png");
  });

  test("over a source row: names that source and outlines the row", async ({ page }) => {
    await open(page);
    await drag(page, "[data-testid=source-row][data-name='Team docs']", "dragenter", ["note.md"]);
    await page.locator("[data-testid=drop-overlay][data-target=source]").waitFor();
    await page.waitForTimeout(300);
    await expect(page).toHaveScreenshot("drop-overlay-source-row.png");
  });

  test("over a cloud source: says why it cannot take files", async ({ page }) => {
    await open(page, { cloud: true });
    await drag(page, "[data-testid=source-row][data-name='Archive bucket']", "dragenter", ["a.md", "b.md"]);
    await page.locator("[data-testid=drop-overlay][data-state=rejected]").waitFor();
    await page.waitForTimeout(300);
    await expect(page).toHaveScreenshot("drop-overlay-cloud-rejected.png");
  });

  test("the overlay is the same size on every screen and leaves nothing behind", async ({ page }) => {
    await open(page);
    await drag(page, "[data-testid=screen-sources]", "dragenter", ["a.md"]);
    const box = await page.locator("[data-testid=drop-overlay]").evaluate((element) => { const r = element.getBoundingClientRect(); return [r.x, r.y, r.width, r.height]; });
    expect(box).toEqual([0, 0, 1280, 800]);
    await page.evaluate(() => window.dispatchEvent(new Event("dragend")));
    await expect(page.locator("[data-testid=drop-overlay]")).toHaveCount(0);
    await expect(page.locator("[data-drop-over]")).toHaveCount(0);
  });
});

test.describe("after a drop", () => {
  test("the toast offers to keep the folder in sync, and the list shows each item", async ({ page }) => {
    await open(page);
    await drag(page, "[data-testid=screen-sources]", "drop", ["ProjectA", "loose.md"]);
    await page.locator("[data-testid=drop-toast]").waitFor();
    await page.locator("[data-testid=upload-tray]").waitFor();
    await page.evaluate(() => (window as unknown as { __push: (push: unknown) => void }).__push({ channel: "upload-progress", payload: { batchId: "00000000-0000-4000-8000-0000000000b1", items: [
      { itemId: "00000000-0000-4000-8000-000000000100", state: "done", bytesDone: 2048, bytesTotal: 2048, drivePath: "ProjectA/readme.md" },
      { itemId: "00000000-0000-4000-8000-000000000101", state: "copying", bytesDone: 2048, bytesTotal: 4096 },
    ] } }));
    await page.waitForTimeout(400);
    await expect(page).toHaveScreenshot("drop-toast-and-list.png");
  });
});
