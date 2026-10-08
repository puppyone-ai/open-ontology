# M9 冻结的 selfbuild 合同状态

角色：G2，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a353`。这一刀消费 G1 M10 固定源 `e4a694f4826bcaaf78927c4ff4c51b95f5c71bd3`（`frontend: declare Node globals and require an explicit adapter listen host`，父提交 `7169d2a839fffb898f14f2ed790e99180a5542bb`）。页面列出 SDK 里已经冻结的 selfbuild operation，按钮保持禁用，不发请求。不签构建、审核、分类、发布、live、旅程、发行或前端目标完成。

## 改动

M10 相对 `ac8737ab2e9ef3d6de41ce2527f487222ce52373` 在 web 上改了 7 个文件，+29/−4：`eslint.config.mjs`、部署适配器、示例、README、设计 APIBINDING、`versions.md`。`packages/sdk-ts` 和 `pnpm-lock.yaml` 没有变。配置里的 Node 全局包含 `Buffer`、`URL`、`structuredClone`。G2 没有改这些文件。这次容器归档的是 `e4a694f48`，没有重跑 `ac8737ab2`，也不改写 M8 那份 eslint 退出 1 的记录。

M10 没有新增 SDK operation。十五个 selfbuild id 在这份归档里已经是 `schema_frozen`，`live` 为 false，`invokeOperation` 返回 `schema_only`。合同提交 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。占位 id `B04.*`、`B03.jobs`、`B06.roles` 仍是 unbound，`invokeOperation` 返回 `not_connected`。

这一刀改了十个自有文件：

- `apps/web/src/features/admin-overview/catalog.ts` sha256 `3638152d5fd290508f2f933f8234d819c62b8bc2a15edfc97cf69a906ea84a0e`。A11 增加 `createBuild`、`listBuilds`。A12 增加 `getBuild`、`cancelBuild`、`getBuildCards`、`getBuildCandidates`、`compileBuildPack`、`getBuildPack`。A17 增加 `listReviewItems`。A18 增加 `getReviewItem`、`previewReviewBatch`、`decideReviewItem`。A19 增加 `listClassifications`、`previewClassifications`、`confirmClassifications`。原占位 id 保留。A04–A06 与 A29–A30 没有加上源或发布 id。
- `apps/web/src/features/builds/pages.tsx` sha256 `a5ddbe2e2bf52b132197656d2423ea5315362af9d566016170d4d546634aae4f`。构建与构建详情的 `operationIds` 与目录一致。运行构建、重试仍交给 `AdminSurface`，该组件把动作设为 `disabled`。
- `apps/web/src/features/reviews/pages.tsx` sha256 `4c332f55770409ac4d6ddc0139fc6ae9d17da62cd5729001a0b9044b62e8c5b3`。审核与审核决定的 `operationIds` 与目录一致。刷新、转交保持禁用。页面仍渲染 `ReviewDesk`。
- `apps/web/src/features/classifications/pages.tsx` sha256 `4f364372c09f76efa685a291e08e30f8c7bd7018a669c4303339b37d6a0ade8b`。分类 `operationIds` 与目录一致。批量确认保持禁用。
- `apps/web/src/features/admin-overview/modules.test.tsx` sha256 `679ea04631f02b56cbd6525ba605464ad8c806a6db91657be497ad1e750555c8`。十五个 `SELFBUILD_HTTP` id 都必须出现在目录里，状态 `schema_frozen`，`live` 为 false，调用结果 `schema_only`，页面文字是 `{id} 状态 schema_frozen，调用 schema_only`。其余目录 id 仍是 unbound / `not_connected`。`registerSource`、`getSourceCatalog`、`createRelease`、`rollbackRelease`、`getActiveRolePack` 的 `bindingById` 为 null，目录里没有它们。A18 仍显示「没有候选」，没有名为「接受」的按钮。渲染四十一页期间 `fetch` 没有被调用。
- `apps/web/src/features/builds/APIBINDING.md` sha256 `b6858d137778fff346048a67eec3111b3c72c1f05ccb4468cc1ea8d62b9ff543`
- `apps/web/src/features/reviews/APIBINDING.md` sha256 `7fcea9c492030e10dcbf2a8e1c15cd0ae44e96c772684f2a4f043d73b2cda6ac`
- `apps/web/src/features/classifications/APIBINDING.md` sha256 `55a8ed11a77edd976d9eb338f5a053b2163b6174c6c2795e822ddaa32cfe520f`
- `apps/web/src/features/sources/APIBINDING.md` sha256 `bb338de6974ea8d0efe1e9e85f100abbf7504fb7b535fbf3dde2176499b53ab6`。写明 `registerSource` 与 `getSourceCatalog` 不在 SDK，页面不增加这些 id。
- `apps/web/src/features/releases/APIBINDING.md` sha256 `8b3e8e0bc2ee0763fbd445b060c1e33a003343c4dbaa27c71aac78c7318550cd`。写明 `createRelease`、`rollbackRelease`、`getActiveRolePack` 不在 SDK，页面不增加这些 id。

M8 的三个文件按原哈希覆盖进归档，这一刀没有再改：`production-route.test.tsx` `661e20d1496aa0dd8752fbe6dd468e31f4c37c7bf49cb1e1d163be8f10004e83`，`current-session.test.tsx` `cb859567a9986015def1732d79a09798a0a5965ed799c1aac1e4b36ebbd41d81`，`permissions/APIBINDING.md` `6f3c10060b5a1b656f72f68f9490ec07a37982d06b668987499527bdaab0bb55`。M4 三处仍在：`blocks.tsx` 没有 `kind: "secret"`，`nav.ts` 没有 `:id`，`ReviewDesk` 是「没有候选」加空的 `EvidencePanel`。M7 的当前会话和退出仍在。

## 已证明

检查用的是 `e4a694f48` 的 `apps/web`、`packages/sdk-ts`、锁、`tools/control-identity` 和 `tools/selfbuild-control-api`，再覆盖 `source-inventory.txt` 里的十三个文件。`router.tsx`、`admin-gate.tsx`、`identity-session.ts`、`identity-context.tsx`、`bindings.ts`、`eslint.config.mjs`、`b01-adapter.mjs`、`fonts.css`、字体清单、`app-shell.tsx`、`review-decision-form.tsx`、`blocks.tsx`、`nav.ts`、`admin-frame.tsx`、`pnpm-lock.yaml` 与固定提交逐字节一致，`frozen_cmp=ok`。锁是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

容器 `oo-a353-node-m9`。镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0，pnpm 12.10.1，aarch64，容器内 uid 0。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。没有安装。node_modules 从这一 UUID 的 M0 私有副本克隆。

| 阶段 | 退出码 |
|---|---|
| tsc app | 0 |
| tsc node | 0 |
| tsc sdk | 0 |
| eslint | 0。日志为空 |
| vitest | 0。31 个文件，67 个测试。日志 `Start at 05:17:30`，耗时 30.46s |
| docker | 0 |

通过的包括 `modules.test.tsx` 3 个（972ms）、`production-route.test.tsx` 3 个（703ms）、`current-session.test.tsx` 3 个（301ms）、`font-coverage.test.ts` 3 个（34ms）、`sdk-binding.test.ts` 4 个、`identity-session.test.ts` 3 个。

第一次归档即完成。没有产品重跑。`ac8737ab2` 没有进入这次容器。

Rust 和 Clippy 没有跑。生产包这次没有重新构建。浏览器没有打开。测试里的 `fetch` 是脚本桩。

## 未证明

运行时 origin 仍是 null。`live`、`journey`、`release` 仍是 false。五个身份 operation 的 `invokeOperation` 仍是 `not_connected`。`B01.projectRoleBinding` 仍是 unbound。应用、保存、预览、运行构建、重试、刷新、转交、批量确认保持禁用。`schema_frozen` 的 `decideReviewItem` 不是候选，决定表单没有挂上。

`tools/source-control-api` 与 `tools/release-control-api` 不在 `e4a694f48`，工作区里仍未跟踪。页面不调用它们。B04 HTTP 没有接上。a373 的交接仍写明身份产品未验收、生产者二进制不在、原 E56 未验收。

证明时 `raw/source-head.txt` 记录工作区 `721bfa6ea8ec495f04111a8b6630b03ef606ea76`，`m10_ancestor=yes`。这次提交的父提交是 `1c4797e2f846da7744565c0f95103b68c1d01846`。这一刀不签管理页或前端目标完成。
