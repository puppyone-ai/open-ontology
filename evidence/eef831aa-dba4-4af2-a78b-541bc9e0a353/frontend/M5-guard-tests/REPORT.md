# M5 守卫测试报告

角色：G2，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a353`。这一刀只更新两份自己的测试，让它们对准 G1 已固定的 M5 守卫。不签页面验收，不签 live、旅程或发行。

## 改动

固定源 `1fce257f41dedd1e460cb4180d5286e256caa10d`。`ADMIN_MODULES` 就是 `ADMIN_FEATURE_MODULES`，41 条，路径不含 `/admin/login` 和 `/admin/bootstrap`。

`apps/web/src/features/admin-overview/modules.test.tsx` 去掉 `ADMIN_MODULES` 为空数组的断言。它核对同一引用、41 条、routeId、path，以及登录和引导路径不在这 41 条里。直接 `load()` 的未接入页面测试还在：占位 operation 仍是 `unbound`，`invokeOperation` 返回 `not_connected`，页面标题在，禁止句「审核决定已保存；尚未发布。」不在，`fetch` 没有被调用。

`apps/web/src/features/admin-overview/production-route.test.tsx` 走 `appRoutes`。`adminSessionReady()` 为 false。`getIdentityCurrent` 的 state 是 `schema_frozen`，`live` 是 false。41 个注册路径（冒号参数换成 `route-sample`）都出现标题「管理登录」，不出现该模块标题；带参数的路径不出现文本 `route-sample`。`load` 间谍没有被调用。`fetch` 没有被调用。断言看渲染结果和调用，不搜索源码里的 `Navigate`。

`modules.test.tsx` sha256 `d94943fa8e453f81045e66caff01e40b80b88abd070a0bbd3d3150d92468576e`。`production-route.test.tsx` sha256 `d3d83d787b1ae97941e4f070ac6c75a4c40a6180b076178c0d7f6e6f07c74d99`。

M4 `e090131ed9df0f3b4d39f126ddfc389c2d129722` 的三处仍在这份归档里。`blocks.tsx` sha256 `289915c57aa493cbaebe9ee3815a8dddc272cb148f68b28222e7c865afc62f27`：连接步骤还在，没有 `kind: "secret"`，`ReviewDesk` 有「没有候选」，没有 `ReviewDecisionForm`。`nav.ts` sha256 `f080a9e665ef6230b07d5beafe3f0a2dfb3d9bbb96bf9f7c5132171f61cc6153`：29 项，没有 `:id` 链接。详情 module 注册仍由未改的测试核对。

## 已证明

检查用的是上述固定提交的 `apps/web`、`packages/sdk-ts`、锁和工作区文件归档，再覆盖这两份测试。第二次运行补上同一提交的 `tools/control-identity` 和 `tools/selfbuild-control-api`，38 个文件与 `git show` 逐字节一致，哈希在 `raw/tools-sha256.txt`。`router.tsx`、`module-registration.ts`、`admin-gate.tsx`、`tokens.css`、`bindings.ts`、`wizard.tsx`、`review-decision-form.tsx`、`app-shell.tsx`、`blocks.tsx`、`nav.ts` 和锁与固定提交一致，哈希在 `raw/frozen-sha256.txt` 和 `raw/source-inventory.txt`。工作区里未提交的 `admin-gate.tsx`、auth 页、user 页和 `identity-context.tsx` 没有进入归档，也没有进入这次提交。`git diff 1fce257f4 HEAD -- apps/web packages/sdk-ts pnpm-lock.yaml` 在提交这两份测试之前是空的，所以已提交的 web 与固定源一致。

容器 `oo-a353-node-m5b`。镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0，pnpm 12.10.1，aarch64。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。没有安装。锁是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。node_modules 从这一 UUID 的 M0 私有副本克隆。

| 阶段 | 退出码 |
|---|---|
| tsc app | 0 |
| tsc node | 0 |
| tsc sdk | 0 |
| eslint | 0 |
| vitest | 1。29 个文件，59 个测试：1 失败，58 通过。开始 00:56:21，耗时 31.49s |
| font-control | 1。只跑字体测试，且临时放回 M5 原版两份 G2 测试。1 个文件，3 个测试：1 失败，2 通过。开始 00:56:53，耗时 764ms |

通过的包括 `modules.test.tsx` 3 个测试（822ms）和 `production-route.test.tsx` 1 个测试（451ms），以及 `sdk-binding.test.ts` 4 个测试。vitest 的唯一失败是 `src/styles/font-coverage.test.ts` 的「covers the reviewed gap and the registered source text」。字体清单 506 个码位，按 `subset-sources.json` 扫到的文本是 504 个。多出来的是 U+4FEE 和 U+8BA2。`raw/font-delta.txt` 记录了这个差。两份 G2 测试里没有这两个字符。放回原版测试后，同一断言仍然失败，差的还是这两个码位。

Rust 和 Clippy 没有跑。这一刀没有 Rust。生产包这次没有重新构建。

## 失败尝试

第一次容器 `oo-a353-node-m5` 的类型检查和 eslint 退出码是 0，vitest 退出码是 1。当时归档没有 `tools/`，`sdk-binding.test.ts` 打不开 `tools/control-identity/openapi/control-identity.openapi.yaml`。字体测试同时失败。日志在 `raw/failed/attempt-1/`。该容器已删除。第二次补上固定提交里的 tools 之后，schema 哈希测试通过。

## 未证明

浏览器没有打开。Playwright 没有执行。`apps/web/tests/e2e/admin/unmounted.spec.ts` 没有改。B01–B12 的产品占位仍是 unbound。`schema_frozen` 的 `getIdentityCurrent` 不是已登录会话。没有真实身份、候选、数据源或详情对象。这一刀不签管理页或前端目标完成。
