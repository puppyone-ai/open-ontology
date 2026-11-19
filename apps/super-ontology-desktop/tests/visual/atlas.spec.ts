import fs from "node:fs";
import path from "node:path";
import { expect, test } from "@playwright/test";

// The static atlas layout (reduced motion draws exactly this): the settled synthetic corpus as SVG, compared pixel for pixel.
test("atlas static layout", async ({ page }) => {
  const svg = fs.readFileSync(path.resolve(import.meta.dirname, "../unit/renderer/atlas/__snapshots__/static-layout.svg"), "utf8");
  await page.setViewportSize({ width: 960, height: 600 });
  await page.setContent(`<body style="margin:0">${svg}</body>`);
  await expect(page).toHaveScreenshot("atlas-static.png");
});
