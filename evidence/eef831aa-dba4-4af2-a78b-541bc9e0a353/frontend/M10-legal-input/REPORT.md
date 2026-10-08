# M10 合法输入预检

角色：G2，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a353`。这一刀消费 G1 M11 固定源 `11e35da6eb0c3588f7cdaea6592daa0e943c4bf8`（`frontend: register published B04 strings without calling the service`）。管理页对每个 operation 做 `prepareCall` 预检。预检不发请求。目录上的 `schema_only` 行不是构建、审核、分类或发布完成。不签 live、旅程、发行或前端目标完成。

## 改动

固定源已经包含 M9 的十五条目录行，也包含 `prepareCall`、`csrfFromCurrent`、`registerRuntime`。`prepareCall` 不调用 fetch。B04 在 CSRF 通过后仍是 `schema_only`。身份变更在 CSRF 通过后仍是 `not_connected`。G2 没有改 SDK、路由、字体、eslint 或身份客户端。

启动前没有名为 `oo-a353` 的容器，也没有 Node 容器。正在运行的 `oo-a383-rust`、`oo-a386-chain-full` 和各 PG 容器没有动。

这一刀改了九个自有文件：

- `apps/web/src/features/admin-overview/legal-call.ts` sha256 `125305813f862b54c96940e86fc1a4b0a111904a3915137405a99c23643c14cf`。用 `GROUP_PUBLICATION.B04.contractCommit` 登记 B04，origin 是 null，ready 是 false。登记被接受才传给 `prepareCall`。
- `apps/web/src/features/admin-overview/surface.tsx` sha256 `e38890720a15c2e86fce1ccb6c35b4ef33a2c13673bd1b1b581ff4a5c8cae0b9`。每个 operation 增加一行 `{id} 预检 {reason}`。页面没有原始 current 信封，预检传入 null。动作仍是 disabled，原因仍是「此能力尚未接入。」
- `apps/web/src/features/admin-overview/legal-input.test.tsx` sha256 `f2fa1f64338f8a7f7d1be401edc6d408f75ca60f857a67be950c191742fc839e`。合法 current 上 `createBuild` 预检是 `schema_only` 且返回值不含 CSRF。`{authorized:true}` 与 null current 上的 POST 是 `csrf_required`。占位和未进 SDK 的 id 是 `not_precise`。`logoutManagementSession` 与 `consumeBootstrapToken` 在合法 current 上是 `not_connected`。构建页按钮「运行构建」保持禁用。未初始化会话打开「初始化」，只请求 `GET /control/v1/identity/current`，不提交引导令牌。
- `apps/web/src/features/admin-overview/modules.test.tsx` sha256 `9e2ebe7f4c77c545e9311e563cfbda47ceb68482b88eea92d37fdd1d424a675d`。四十一页都要出现预检行。POST 是 `csrf_required`，GET 是 `schema_only`，其余是 `not_precise`。`schema_frozen` 目录行仍在。渲染期间没有 fetch。
- `apps/web/src/features/permissions/current-session.test.tsx` sha256 `7675e9e2615897c6c15746ac424f3a34df6c1646169fe835c0f30932b07d493f`。角色与绑定页的占位预检是 `not_precise`。已初始化会话的「退出」仍走 `logoutManagementSession`。
- `apps/web/src/features/builds/APIBINDING.md` sha256 `e6449f7edd322a36eb96667d6cd92ded9b9d2fc03067e39298f2f476d46dde58`
- `apps/web/src/features/reviews/APIBINDING.md` sha256 `19170ff0dad5e81cb8d76d07291f6c1b83cfd5a924f90aca1ce2b7e38de6bc9c`
- `apps/web/src/features/classifications/APIBINDING.md` sha256 `52be3c165b50c778335a2a51930eb3383529f62af867e13fb1b44ff3e02c6854`
- `apps/web/src/features/permissions/APIBINDING.md` sha256 `b87cd543bc6fe38e80c4586f35fefa9698eb9ac3bc5b1e09075f75ce5f7d667b`

页面预检在 current 为 null 时：`createBuild`、`cancelBuild`、`previewReviewBatch`、`decideReviewItem`、`previewClassifications`、`confirmClassifications`、`compileBuildPack` 是 `csrf_required`。对应 GET 是 `schema_only`。`B04.builder`、`B01.projectRoleBinding` 以及其他占位是 `not_precise`。源和发布 id 没有写入页面。

M7 的当前会话和退出仍在。M4 三处仍在。M8 的五个角色守卫没有改。

## 已证明

检查用的是 `11e35da6e` 的 `apps/web`、`packages/sdk-ts`、锁、`tools/control-identity` 和 `tools/selfbuild-control-api`，再覆盖上面九个文件。`router.tsx`、`admin-gate.tsx`、`identity-session.ts`、`identity-context.tsx`、`bindings.ts`、`client.ts`、`eslint.config.mjs`、`b01-adapter.mjs`、`fonts.css`、字体清单、`app-shell.tsx`、`review-decision-form.tsx`、`blocks.tsx`、`nav.ts`、`admin-frame.tsx`、`pnpm-lock.yaml` 与固定提交逐字节一致，`frozen_cmp=ok`。锁是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

容器 `oo-a353-node-m10`。镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0，pnpm 12.10.1，aarch64，容器内 uid 0。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。没有安装。node_modules 从这一 UUID 的 M0 私有副本克隆。

| 阶段 | 退出码 |
|---|---|
| tsc app | 0 |
| tsc node | 0 |
| tsc sdk | 0 |
| eslint | 0。日志为空 |
| vitest | 0。33 个文件，74 个测试。日志 `Start at 07:47:37`，耗时 34.07s |
| docker | 0 |

`legal-input.test.tsx` 3 个测试，200ms。第一次归档即完成。

Rust 和 Clippy 没有跑。生产包这次没有重新构建。浏览器没有打开。构建页和预检里的 `fetch` 是脚本桩，次数是 0。退出测试里的 `fetch` 仍是同源 `/control/v1/identity/current` 与 `/control/v1/auth/logout`。

## 未证明

运行时 origin 仍是 null。`live`、`journey`、`release` 仍是 false。B04 ready 仍是 false。页面预检拿不到原始 current，所以已登录页面上的 POST 仍显示 `csrf_required`，不伪造信封。`prepareCall("logoutManagementSession")` 的 `not_connected` 没有替换 `logoutManagementSession`。`prepareCall("consumeBootstrapToken")` 的 `not_connected` 没有替换 `/admin/bootstrap` 的真实提交。

`B01.projectRoleBinding` 没有写入接口。应用、保存、预览保持禁用。`registerSource`、`getSourceCatalog`、`createRelease`、`rollbackRelease`、`getActiveRolePack` 不在 SDK。B04 HTTP 没有接上。a373 的交接仍写明生产者二进制未交付、E56 未验收。

证明时 `raw/source-head.txt` 记录工作区 `bb831355662c1dffd76b86bf60417158f4513134`，`m11_ancestor=yes`。这次提交的父提交是 `2f9f92d9f3cb595ca3a1b50f83bd1f0101738c8a`。这一刀不签管理页或前端目标完成。
