import { expect, test } from "@playwright/test";

test("unauthenticated entry stays on login without a password or stored identity", async ({ page }) => {
  await page.goto("/");
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByRole("heading", { level: 1, name: "登录" })).toBeVisible();
  await expect(page.getByText("此能力尚未接入。")).toBeVisible();
  await expect(page.getByText("依赖 B01。")).toBeVisible();
  await expect(page.getByText("这里不收集密码，也不创建本地账号。")).toBeVisible();
  await expect(page.getByLabel(/密码/)).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "设计基线" })).toHaveCount(0);
  await expect(page.locator('a[href="/admin"]')).toHaveCount(0);
  await page.getByRole("radio", { name: "深色" }).check();
  await page.getByRole("radio", { name: "紧凑" }).check();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(page.locator("html")).toHaveAttribute("data-density", "compact");
  const keys = await page.evaluate(() => Object.keys(localStorage).sort());
  expect(keys).toEqual(["oo.pref.density", "oo.pref.theme"]);
});

test("production fonts stay on this origin and leave the full face unloaded", async ({ page }) => {
  const urls: string[] = [];
  page.on("request", (request) => urls.push(request.url()));
  for (const path of ["/", "/account", "/conversations/route-sample"]) {
    await page.goto(path);
    await page.evaluate(async () => {
      await document.fonts.ready;
    });
  }
  const external = urls.filter((url) => new URL(url).host !== "127.0.0.1:4173");
  expect(external).toEqual([]);
  const fonts = urls.filter((url) => url.includes("/fonts/"));
  expect(fonts.some((url) => url.endsWith("/fonts/Geist-wght.woff2"))).toBe(true);
  expect(fonts.some((url) => url.endsWith("/fonts/NotoSansSC-400-subset.woff2"))).toBe(true);
  expect(fonts.some((url) => url.endsWith("/fonts/NotoSansSC-full.woff2"))).toBe(false);
  expect(fonts.every((url) => url.startsWith("http://127.0.0.1:4173/fonts/") && url.endsWith(".woff2"))).toBe(true);
  expect(urls.join("\n")).not.toMatch(/fonts\.googleapis|fonts\.gstatic|use\.typekit/i);
});

test("disabled send stays on the conversation and the menu keeps a 44px target", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/conversations/route-sample");
  const send = page.getByRole("button", { name: "发送" });
  await expect(send).toBeDisabled();
  await expect(page.getByRole("textbox", { name: "问题" })).toBeDisabled();
  await send.click({ force: true });
  await expect(page).toHaveURL(/\/conversations\/route-sample$/);
  await page.goto("/account");
  const minHeight = await page.getByRole("button", { name: "菜单" }).evaluate((element) => {
    return Number.parseFloat(getComputedStyle(element).minHeight);
  });
  expect(minHeight).toBeGreaterThanOrEqual(44);
});
