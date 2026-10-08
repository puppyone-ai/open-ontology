# M6 守卫合同报告

角色：G2，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a353`。这一刀只改自己的 `production-route.test.tsx`。不签页面验收，不签 live、旅程或发行。

## 改动

固定源 `97909a4b7a42b39910f8277af5aa86cc899cbb99`。五个身份 operation 的目录状态是 `fixture_verified`，`live` 是 false。`adminSessionReady()` 在没有 `project.admin` 会话时仍是 false。

`apps/web/src/features/admin-overview/production-route.test.tsx` sha256 `f9d6a190d97478988a18fd859c21109c3061aeec5368c882f5f99b5c6e759e84`。

- 目录为 `fixture_verified` 时，41 条注册路径仍渲染「管理登录」，不调用 `load`，不调用 `fetch`。`invokeOperation` 对五个身份 id 返回 `not_connected`，对 `createBuild` 返回 `schema_only`。
- `IdentityProvider` 只探测 `GET /control/v1/identity/current`。401、`{"authorized": true}`、未初始化会话、reviewer 会话都不挂载总览。未初始化进入「初始化」。reviewer 进入「没有权限」。
- 解析后的 `project.admin` 会话会挂载总览。总览按钮「刷新」保持禁用，`B04.builder` 仍是 unbound / `not_connected`。探测之外没有别的请求。

`B01.projectRoleBinding` 和 `B04.builder` 仍是 unbound。角色、身份绑定、权限预览页面没有改。M4 的三处仍在这份归档里：连接步骤没有 secret 字段，导航没有 `:id`，`ReviewDesk` 有「没有候选」。

## 已证明

检查用的是上述固定提交的 `apps/web`、`packages/sdk-ts`、锁和 `tools/control-identity`、`tools/selfbuild-control-api`，再覆盖这一份测试。`router.tsx`、`admin-gate.tsx`、`identity-context.tsx`、`identity-session.ts`、`bindings.ts`、字体清单、`tokens.css`、`wizard.tsx`、`review-decision-form.tsx`、`app-shell.tsx`、`blocks.tsx`、`nav.ts`、`permissions/pages.tsx` 与固定提交逐字节一致，哈希在 `raw/frozen-sha256.txt`。锁是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

容器 `oo-a353-node-m6`。镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0，pnpm 12.10.1，aarch64。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。没有安装。node_modules 从这一 UUID 的 M0 私有副本克隆。

| 阶段 | 退出码 |
|---|---|
| tsc app | 0 |
| tsc node | 0 |
| tsc sdk | 0 |
| eslint | 0 |
| vitest | 1。30 个文件，63 个测试：1 失败，62 通过。开始 03:45:28，耗时 29.08s |

通过的包括 `production-route.test.tsx` 3 个测试（555ms）、`modules.test.tsx` 3 个、`sdk-binding.test.ts` 4 个、`identity-session.test.ts` 2 个。唯一失败是 G1 的 `font-coverage.test.ts`：清单 506 个码位，根目录扫到 504 个，多出 U+4FEE 和 U+8BA2。字体文件没有改。

Rust 和 Clippy 没有跑。这一刀没有 Rust。生产包这次没有重新构建。浏览器没有打开。

## 未证明

没有对着真实发行方提交引导令牌或退出。`invokeOperation` 不发送这五个请求。运行时 origin 仍是 null。B04 没有接到 a373 的服务，构建、审核分类和 compiled pack 没有串起来。总览和其他管理页的业务数据仍是未接入。这一刀不签管理页或前端目标完成。
