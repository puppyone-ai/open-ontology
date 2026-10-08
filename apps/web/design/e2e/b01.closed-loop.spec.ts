import { readFileSync, writeFileSync } from "node:fs";
import { expect, test } from "@playwright/test";

const evidence = process.env.FE01_EVIDENCE;
const tokenFile = process.env.FE01_BOOTSTRAP_FILE;
if (!evidence || !tokenFile) {
  throw new Error("FE01_EVIDENCE and FE01_BOOTSTRAP_FILE are required");
}

test("test issuer logs in, initializes, opens admin, logs out, then denies a reviewer", async ({ page }) => {
  const token = readFileSync(tokenFile, "utf8").trim();
  const observed: { path: string; status: number }[] = [];
  const fonts: string[] = [];
  page.on("response", (response) => {
    const url = new URL(response.url());
    if (url.pathname.startsWith("/control/")) {
      observed.push({ path: url.pathname, status: response.status() });
    }
  });
  page.on("request", (request) => {
    if (request.url().includes("/fonts/")) fonts.push(request.url());
  });

  await page.goto("/login");
  await page.getByRole("link", { name: "开始管理身份登录" }).click();
  await page.waitForURL(/\/account$/);
  await expect(page.getByText("主体 fixture-admin")).toBeVisible();
  await expect(page.getByText("初始化状态 未初始化")).toBeVisible();
  await page.goto("/admin");
  await expect(page).toHaveURL(/\/admin\/bootstrap$/);
  await page.getByLabel("引导令牌").fill(token);
  await page.getByRole("button", { name: "提交初始化" }).click();
  await expect(page.getByText("已经初始化。客户集成 not_claimed。")).toBeVisible();
  await page.getByRole("link", { name: "管理入口" }).click();
  await expect(page.getByRole("heading", { name: "总览" })).toBeVisible();
  await page.screenshot({ path: `${evidence}/screenshots/admin-overview.png` });

  const rejected = await page.request.post("/control/v1/auth/logout", {
    headers: { "x-oo-csrf": "a".repeat(43) },
  });
  expect(rejected.status()).toBe(403);
  await page.goto("/account");
  await expect(page.getByText("主体 fixture-admin")).toBeVisible();
  await page.getByRole("button", { name: "退出" }).click();
  await expect(page.getByRole("link", { name: "登录" })).toBeVisible();

  writeFileSync(`${evidence}/subject.request`, "fixture-reviewer\n");
  await expect.poll(() => readFileSync(`${evidence}/subject.applied`, "utf8").trim(), { timeout: 15_000 }).toBe("fixture-reviewer");

  await page.goto("/admin");
  await expect(page.getByRole("heading", { name: "管理登录" })).toBeVisible();
  await page.getByRole("link", { name: "开始管理身份登录" }).click();
  await page.waitForURL(/\/admin\/bootstrap$/);
  await expect(page.getByText("已经初始化。客户集成 not_claimed。")).toBeVisible();
  await page.goto("/admin");
  await expect(page.getByRole("heading", { name: "没有权限" })).toBeVisible();
  await expect(page.getByText("当前身份没有执行此操作的权限。")).toBeVisible();
  await expect(page.getByRole("heading", { name: "总览" })).toHaveCount(0);
  await page.screenshot({ path: `${evidence}/screenshots/forbidden.png` });
  await page.goto("/account");
  await expect(page.getByText("主体 fixture-reviewer")).toBeVisible();
  await expect(page.getByText("项目 alpha 角色 reviewer")).toBeVisible();
  await expect(page.getByText("没有管理能力。")).toBeVisible();
  await page.getByRole("button", { name: "退出" }).click();
  await expect(page.getByRole("link", { name: "登录" })).toBeVisible();
  await page.goto("/admin");
  await expect(page.getByRole("heading", { name: "管理登录" })).toBeVisible();

  expect(fonts.some((url) => url.includes("NotoSansSC-full"))).toBe(false);
  expect(fonts.some((url) => url.includes("fonts.googleapis") || url.includes("fonts.gstatic"))).toBe(false);
  const receipt = {
    customer_integration: "not_claimed",
    live: false,
    journey: false,
    release: false,
    proved_roles: ["admin", "reviewer"],
    operations: observed,
  };
  writeFileSync(`${evidence}/raw/browser-receipt.json`, `${JSON.stringify(receipt, null, 2)}\n`);
  expect(observed.some((item) => item.path === "/control/v1/auth/login" && item.status === 302)).toBe(true);
  expect(observed.some((item) => item.path === "/control/v1/auth/callback" && item.status === 302)).toBe(true);
  expect(observed.some((item) => item.path === "/control/v1/identity/current" && item.status === 200)).toBe(true);
  expect(observed.some((item) => item.path === "/control/v1/auth/bootstrap/consume" && item.status === 200)).toBe(true);
  expect(observed.some((item) => item.path === "/control/v1/auth/logout" && item.status === 200)).toBe(true);
});
