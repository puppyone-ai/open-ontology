import { expect, test } from "@playwright/test";

test("shows the foundation fixture without a business session", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1, name: "设计基线" })).toBeVisible();
  await expect(page.getByText("不是产品功能")).toBeVisible();
  await expect(page.getByText("此能力尚未接入。路由归 FE-02，类型 SDK 归 FE-03，公共组件归 FE-04。本页只展示已经冻结的 Token、主题、密度和自托管字体。")).toBeVisible();
  await expect(page.getByText("当前身份没有执行此操作的权限。本页不提供申请入口。")).toBeVisible();
  await expect(page.getByText("下面是静态标本，不是服务端结果。")).toBeVisible();
  await expect(page.locator("[data-rare-glyph]")).toHaveText("龘");
  await expect(page.locator('a[href="/admin"], a[href="/"]')).toHaveCount(0);
  await expect(page.getByRole("button", { name: "登录" })).toHaveCount(0);
  const button = page.getByRole("button", { name: "主按钮样本" });
  await expect(button).toBeDisabled();
  await button.click({ force: true });
  await expect(page).toHaveURL("http://127.0.0.1:4173/");
  await page.getByRole("radio", { name: "深色" }).check();
  await page.getByRole("radio", { name: "紧凑" }).check();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(page.locator("html")).toHaveAttribute("data-density", "compact");
  expect(await page.evaluate(() => localStorage.length)).toBe(0);
});

test("loads only same-origin fonts, including the full rare-glyph fallback", async ({ page }) => {
  const urls: string[] = [];
  page.on("request", (request) => urls.push(request.url()));
  await page.goto("/");
  await page.evaluate(async () => {
    await document.fonts.ready;
  });
  const external = urls.filter((url) => new URL(url).host !== "127.0.0.1:4173");
  expect(external).toEqual([]);
  const fonts = urls.filter((url) => url.includes("/fonts/"));
  expect(fonts.some((url) => url.endsWith("/fonts/Geist-wght.woff2"))).toBe(true);
  expect(fonts.some((url) => url.endsWith("/fonts/NotoSansSC-400-subset.woff2"))).toBe(true);
  expect(fonts.some((url) => url.endsWith("/fonts/NotoSansSC-full.woff2"))).toBe(true);
  expect(fonts.every((url) => url.startsWith("http://127.0.0.1:4173/fonts/") && url.endsWith(".woff2"))).toBe(true);
  expect(urls.join("\n")).not.toMatch(/fonts\.googleapis|fonts\.gstatic|use\.typekit/i);
});

test("keeps a 44px touch target at 390px", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  const minHeight = await page.getByRole("button", { name: "主按钮样本" }).evaluate((element) => {
    return Number.parseFloat(getComputedStyle(element).minHeight);
  });
  expect(minHeight).toBeGreaterThanOrEqual(44);
});
