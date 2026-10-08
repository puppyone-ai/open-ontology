# M8 五个角色回执报告

角色：G2，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a353`。这一刀消费 G1 M9 的固定回执。调用方 JSON 不是权限。不签页面验收，不签 live、旅程或发行。

## 改动

固定源 `ac8737ab2e9ef3d6de41ce2527f487222ce52373`。身份服务提交仍是 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。回执常量是 `CONTROL_IDENTITY_FIXTURE_RECEIPT`，证据文件是 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M9-b01-roles/raw/browser-receipt.json`。

- `apps/web/src/features/admin-overview/production-route.test.tsx` sha256 `661e20d1496aa0dd8752fbe6dd468e31f4c37c7bf49cb1e1d163be8f10004e83`。`provedRoles` 是 admin、reviewer、steward、auditor、business_owner，项目 `alpha`。四个没有 `project.admin` 的角色在 `/admin/permissions/roles` 显示「没有权限」和「当前身份没有执行此操作的权限。」，不挂载「角色」或「总览」，不调用 `load`。`{"authorized":true, claims:["project.admin"]}` 以及带 `claims` 键的当前信封都停在「管理登录」，页面上没有 `caller`。`registerSource`、`getSourceCatalog`、`createRelease`、`rollbackRelease`、`getActiveRolePack` 的 `bindingById` 是 null，`invokeOperation` 返回 `unknown_operation`。`createBuild` 仍是 `schema_frozen`。
- `apps/web/src/features/permissions/current-session.test.tsx` sha256 `cb859567a9986015def1732d79a09798a0a5965ed799c1aac1e4b36ebbd41d81`。管理员角色页不出现 `claims`。身份绑定页保留「这里不列出人、组或 claims。」，保存仍禁用。
- `apps/web/src/features/permissions/APIBINDING.md` sha256 `6f3c10060b5a1b656f72f68f9490ec07a37982d06b668987499527bdaab0bb55`。写明 M9 回执和上述未进入 SDK 的 operation。

页面组件没有改。M7 的退出和当前会话显示仍在。M4 三处仍在这份归档里。

## 已证明

检查用的是上述固定提交的 `apps/web`、`packages/sdk-ts`、锁、`tools/control-identity` 和 `tools/selfbuild-control-api`，再覆盖上面三个文件。`router.tsx`、`admin-gate.tsx`、`identity-context.tsx`、`identity-session.ts`、`bindings.ts`、字体清单、`fonts.css`、`app-shell.tsx`、`blocks.tsx`、`nav.ts`、`admin-frame.tsx`、`permissions/pages.tsx`、`b01-adapter.mjs`、`eslint.config.mjs` 与固定提交逐字节一致。锁是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

容器 `oo-a353-node-m8`。镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0，pnpm 12.10.1，aarch64。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。没有安装。node_modules 从这一 UUID 的 M0 私有副本克隆。

| 阶段 | 退出码 |
|---|---|
| tsc app | 0 |
| tsc node | 0 |
| tsc sdk | 0 |
| eslint | 1 |
| vitest | 0。31 个文件，67 个测试。开始 05:00:49，耗时 27.45s |
| docker | 1 |

通过的包括 `production-route.test.tsx` 3 个（619ms）、`current-session.test.tsx` 3 个（251ms）、`font-coverage.test.ts` 3 个、`identity-session.test.ts` 3 个。

eslint 的 7 个错误都在与固定提交逐字节一致的部署文件：`b01-adapter.mjs` 262、273 的 `Buffer` 和 321 的 `URL`；`b01-adapter.test.mjs` 12、13 的 `URL` 和 281、296 的 `Buffer`。自有文件没有 lint 条目。G2 没有改 eslint 配置。

Rust 和 Clippy 没有跑。生产包这次没有重新构建。浏览器没有打开。测试里的 `fetch` 是脚本桩。

## 未证明

运行时 origin 仍是 null。`live`、`journey`、`release` 仍是 false。`invokeOperation` 对五个身份 id 返回 `not_connected`。`B01.projectRoleBinding` 仍是 unbound。应用、保存、预览保持禁用。

`tools/source-control-api` 和 `tools/release-control-api` 在工作区是未跟踪目录，不在 `ac8737ab2`，也不在 HEAD。第一次归档因此退出 128，没有启动容器。合同文本可以读到「调用方 JSON 不是权限」，但它们不是已发布的固定提交。页面不调用它们，也不制造源、构建或发布结果。B04 仍是 `schema_frozen`。a373 的交接仍写明原 E56 未验收。

分支上另有 `e4a694f48`，给 eslint 增加了 `Buffer`、`URL`、`structuredClone`。这次容器没有执行那个提交，G2 也没有改那份配置。

提交前工作区是 `53017ed29c98c49fb44a7eb18d5df6d5de273046`。相对固定源，web 上多出来的仍是 `e4a694f48`。这一刀不签管理页或前端目标完成。
