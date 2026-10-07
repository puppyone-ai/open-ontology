import { expect, test } from "@playwright/test";

test("admin entry stays on the G1 login redirect", async ({ page }) => {
  await page.goto("/admin");
  await expect(page.getByRole("heading", { name: "管理登录" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "总览" })).toHaveCount(0);
});
