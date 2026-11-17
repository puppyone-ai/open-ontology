# Super Ontology 前端分册（Puppyone Desktop 侧）

> 状态：Proposed（2026-11-16 落版，待评审）
> 前提：[00 总纲与决策记录](00-overview-and-decisions.md)（D3、D6、D26、D28–D35、D40，§5.1、§5.4、§6）；[10 后端](10-backend.md) §8（**控制 API 契约 `docs-api/1.0` 以它为准**）；[30 闭环与 Sprint](30-closed-loops-and-sprints.md)；[50 提交与追溯](50-commit-and-traceability.md)；[60 端到端与 owner 验收](60-e2e-and-owner-acceptance.md)。
> 仓库：Puppyone Desktop `puppyone-ai/puppyone`，集成分支 `codex/super-drive`（从本地 `main` 拉出，开发期间不碰 `main`/`qubits`，最后一次性提 PR，见 00 D43、50 §2.2），任务分支 `codex/super-drive-F-S<n>-<nn>-<短描述>`。
> 本文所有“现有代码”的引用基于 `puppyone@0c666447`（`main`，0.3.27）。`qubits`（`d2c9f079`）是 `main` 的祖先，落后 98 个提交，并且**没有** `src/features/cloud/share/`（ShareWizard 是 0.3.27 才进 `main` 的），见 §8.2 Q1。
> 和 00 冲突的地方以 00 为准；我认为 00 或 10 需要改的地方都列在 §8.2“待总纲裁决”，正文按现行契约写。

---

## 0. 阅读指引

| 章 | 内容 | 主要读者 |
|---|---|---|
| §1 | 前端架构：目录与文件、分层、主进程服务、IPC、store、DataPort、与 10 §8 的逐项对接、开关与验收版身份 | 实现者 |
| §2 | 设计规范落地：token、复用组件、新组件、动效、无障碍与可测试性 | 实现者、设计评审 |
| §3 | 页面与交互规格（线框、交互、状态呈现） | 实现者、产品 |
| §4 | 状态页面矩阵（文件、Share、待确认、引擎、连接、页面级） | 实现者、测试 |
| §5 | i18n | 实现者 |
| §6 | 测试方案（unit / component / integration / architecture / T1 / T2 / 视觉） | 测试 |
| §7 | 按 Sprint 的前端任务卡 | 主线程、执行者 |
| §8 | 风险、开放问题、待总纲裁决、后端契约依赖清单 | 主线程 |

术语：**Drive** 指一个登记的文件根（00 §7）；**引擎**指 `oo serve` 守护进程及其管理的容器（10 §2.1）；**主进程服务**指本文新增的 Electron 主进程模块 `electron/main/super-ontology/`；**渲染层**指 `src/` 下的 React 代码。

---

## 1. 前端架构

### 1.1 分层总览

```
渲染层（src/features/super-drive/，React 18，Context + useSyncExternalStore）
  SuperDriveSurface（sidebar/main 两个槽，经 WorkspaceSurfaceOutlet 投影）
   ├─ 浏览器：DriveSidebar（Drive 切换 + 分区导航 + ExplorerTree 文件夹树）
   │          DriveToolbar / DriveBreadcrumb / DriveFileTable | DriveFileGrid / DriveBannerStack
   ├─ 详情栏 FileDetailsPane（概览 / 解析结果 / 实体 / 共享）
   ├─ 队列面板 ProcessingQueuePanel（右下角浮层：上传 + 处理）
   ├─ 待确认 ReviewQueuePage（M2：RevisionView）
   ├─ 分享 SuperDriveShareDialog、ShareListPage
   ├─ 引擎 EngineStatusPage / EngineStatusPill
   └─ 设置 DriveSettingsPage（Drive 登记、模型、用量）
        │ superDriveApi.ts（唯一触达 window.puppyoneDesktop.superOntology 的文件）
        │ superDriveStore.ts（只读副本 + 事件归并 + 乐观更新）
────────┼──────────────────────── preload（electron/preload.cjs）────────────────────────────
        │ window.puppyoneDesktop.superOntology.*（invoke / subscribe；上传时用 webUtils 取路径）
────────┼──────────────────────── 主进程（electron/main/super-ontology/）───────────────────
  super-ontology-ipc.mjs（经 trusted-ipc 注册；参数校验 → 服务 → 统一信封）
  service.mjs（组合根）
   ├─ engine-locator.mjs      找 docker、oo 可执行文件（用户命令环境 PATH + 手动指定）
   ├─ docker-preflight.mjs    docker 是否安装、daemon 是否运行、磁盘余量
   ├─ engine-lifecycle.mjs    oo serve 启动/复用/停止/崩溃重启；健康巡检；状态机
   ├─ serve-credentials.mjs   读 serve.json 与 token 文件（0600、O_NOFOLLOW、不出主进程）
   ├─ control-client.mjs      HTTP 客户端：Bearer、Idempotency-Key、If-Match、超时、错误归一
   ├─ event-bridge.mjs        SSE → IPC：since 游标、去重、410 全量重拉、环形缓冲、合并推送
   ├─ grants.mjs              文件夹授权（Drive 根、挂载目标、oo 可执行文件），绑定 WebContents
   ├─ secret-files.mjs        把用户输入的密钥写成 0600 文件，生成 file: secret_ref
   ├─ upload-service.mjs      把文件复制进 fs Drive（临时名 + 原子改名）并触发 :sync
   └─ drive-file-protocol.mjs puppyone-drive: 能力 URL，给预览器读 Drive 原件
        │ HTTP 127.0.0.1:<port>，Authorization: Bearer oodt_…，OO-Docs-Api-Min: 1.0
────────┴──────────────────────── oo serve（10 §8.6）───────────────────────────────────────
```

分层规则（全部有对应的检查，见 §6.4）：

1. 渲染层**只**通过 `superDriveApi.ts` 调 `window.puppyoneDesktop.superOntology`；其他文件出现 `puppyoneDesktop` 即失败。
2. 渲染层拿不到控制 token、refs 凭证、端口号、宿主绝对路径以外的密钥；IPC 返回值里不含 `token`、`oodt_`、`oodr_` 子串（主进程出口断言 + 架构测试）。
3. 主进程从不接受渲染层给的 URL 或端口；HTTP 基址只来自 `serve.json`，主机名写死 `127.0.0.1`，不复用 `cloud:api-request`，也不修改 `shared/cloudEndpoint.js` 的白名单（D32）。
4. 宿主路径（Drive 根、挂载目标、oo 可执行文件）只能来自主进程弹出的原生选择框，渲染层拿到的是 `grantId` 与显示用路径，模式同 `electron/main/project-location-grants.mjs`。
5. 现有数据工作区一行不改（D33）：不改 `src/features/data-workspace/**`、`packages/shared-ui/src/data/**`、`src/features/app-shell/DesktopDataWorkspaceSurface.tsx`。Super Drive 通过现有的 surface 投影机制（`DesktopDataWorkspaceSurface.tsx:363-405` 已把非 `data` surface 的 sidebar/main 投到 `explorerSlot`/`mainSlot`）挂进来。

### 1.2 目录与文件清单

内部组织遵循 `puppy-issues/document/puppyone-desktop/platform/renderer-code-organization.md`：领域内平铺，按子功能分目录，不按技术层分 `domain/application/infrastructure`；调用主进程的函数集中在一个 `*Api.ts`。子目录名避开 `sidebar|sections|rows|hooks|styles`（`scripts/check-sidebar-architecture.mjs:71` 会把跨 feature 引用这些目录判为深引用）。领域目录放在 `src/features/super-drive/`，因为现有全部检查脚本都按 `src/features/<name>` 识别领域（例如 `check-sidebar-architecture.mjs:443`）；该文档的目标布局 `src/<domain>/` 尚未迁移。

#### 1.2.1 渲染层 `src/features/super-drive/`

| 文件 | 职责 | 首次出现 |
|---|---|---|
| `index.ts` | 公开入口：`createSuperDriveWorkspaceSurface`、`SuperDriveNavigationIcon`、类型。别的领域只能从这里引用 | F-S1-02 |
| `SuperDriveSurface.tsx` | 产出 `{sidebar, main}`；按 `route.section` 切换主区页面；挂 `ProcessingQueuePanel` 与对话框宿主 | F-S1-02 |
| `superDriveApi.ts` | 唯一的桥接文件：调用 preload、校验返回（用生成的类型守卫）、把失败统一成 `SuperDriveError`；缺桥时返回 `SO_BRIDGE_UNAVAILABLE` | F-S1-03 |
| `superDriveStore.ts` | 渲染层只读副本：引擎、连接、Drive、文件、计数、待确认、Share、用量、上传；`subscribe/getSnapshot`；引用计数 `connect()`（模式同 `src/features/model-connections/application/ModelConnectionStore.ts`） | F-S2-05 |
| `superDriveEvents.ts` | 纯函数：`applyEvents(state, events) → state`；按 `seq` 去重与乱序丢弃；`files.bulk_changed` 等触发“需重拉”标记 | F-S2-05 |
| `superDriveSelectors.ts` | `useSuperDriveSelector(selector, isEqual)`：在 `useSyncExternalStore` 上做选择与浅比较缓存（不引入 zustand 或 `use-sync-external-store`） | F-S2-05 |
| `superDriveRoute.ts` | 页面内路由状态（分区、Drive、路径、选中文件、详情栏开关）与 URL 无关的序列化（存 sessionStorage，窗口重开恢复） | F-S2-05 |
| `superDriveErrors.ts` | 错误码 → 文案 key、是否可重试、建议动作；未知码按前缀族回退（§4.8） | F-S2-05 |
| `superDriveStateModel.ts` | **状态矩阵的唯一真相源**：`FILE_STATE_PRESENTATION`、`SHARE_STATE_PRESENTATION`、`REVIEW_KIND_PRESENTATION`、`ENGINE_STATUS_PRESENTATION`（图标、tone、文案 key、动作、可重试）。§4 的表与它逐项一致，单测检查覆盖生成枚举的每个值 | F-S3-02 |
| `superDriveDataPort.ts` | objectstore 版 `DataPort`（§1.8），供 `ExplorerTree` 与 `FilePreview` 使用 | F-S2-07 |
| `mountCommand.ts` | 依据 10 §7.9 生成 `oo context mount` 的 argv 与按平台转义的显示串；与引擎返回的 `Share.command` 做一致性校验 | F-S6-02 |
| `operationIds.ts` | 为每个用户意图生成 `operationId`（作 `Idempotency-Key`，`^[A-Za-z0-9_-]{16,128}$`），超时重发复用同一个 | F-S2-05 |
| `contract/docsApi.generated.ts` | 由 `contracts/docs-api/*.json` 生成的 TS 类型、枚举数组与类型守卫（只读，禁止手改） | F-S1-05 |
| `super-drive.css` | 样式清单：`@import` 下列各组件 CSS，全部在 `@layer features { … }` 内 | F-S1-02 |
| `super-drive-tokens.css` | Feature token 层：**唯一允许出现字面尺寸的 CSS 文件**，其余文件只能用 `var(--…)`（§2.2） | F-S1-02 |
| `browser/DriveSidebar.tsx` | 左栏：Drive 切换器、分区导航（文件/待确认/共享/引擎/设置，含计数徽标）、`ExplorerTree` 文件夹树 | F-S2-07 |
| `browser/DriveToolbar.tsx` | 面包屑、搜索、视图切换（列表/网格）、上传、共享、`EngineStatusPill` | F-S2-07 |
| `browser/DriveBreadcrumb.tsx` | 面包屑（长路径折叠成“…”菜单） | F-S2-07 |
| `browser/DriveFileTable.tsx` | 列表视图：虚拟滚动、表头排序、多选、键盘、右键菜单入口 | F-S2-07 |
| `browser/DriveFileGrid.tsx` | 网格视图：同一选择模型 | F-S3-05 |
| `browser/DriveFileRow.tsx` | 单行（名称、处理状态、大小、待确认、进入状态的时间） | F-S2-07 |
| `browser/FileStateBadge.tsx` | 处理状态徽标（§2.4） | F-S2-07 |
| `browser/DriveDropZone.tsx` | 拖拽上传区（含可访问名称与键盘等价入口） | F-S3-01 |
| `browser/DriveContextMenu.tsx` | 右键菜单与“更多”菜单（同一份菜单模型） | F-S3-03 |
| `browser/DriveSelectionBar.tsx` | 多选后的批量操作条 | F-S3-03 |
| `browser/useDriveSelection.ts` | 选择模型（单选、Shift 区间、Cmd/Ctrl 追加、全选=服务端过滤条件） | F-S3-03 |
| `browser/useDriveKeyboard.ts` | 列表键盘映射（§3.2.4） | F-S3-03 |
| `browser/useDriveFolderPages.ts` | 按目录键集分页加载、骨架占位、`as_of_seq` 对齐 | F-S2-07 |
| `browser/browser.css` | 浏览器组件样式 | F-S2-07 |
| `banners/DriveBannerStack.tsx` | 横幅栈（最多同时 2 条，按 §4.7 优先级） | F-S3-03 |
| `banners/DriveBanner.tsx` | 单条横幅（tone、图标、标题、说明、动作、可关闭） | F-S3-03 |
| `banners/bannerModel.ts` | 从 store 推导应出现的横幅（纯函数，可单测） | F-S3-03 |
| `banners/banners.css` | 横幅样式 | F-S3-03 |
| `queue/ProcessingQueuePanel.tsx` | 右下角浮层：上传与处理两段，折叠/展开 | F-S3-01 |
| `queue/UploadQueueList.tsx` | 上传条目列表（进度、取消、重试） | F-S3-01 |
| `queue/uploadQueue.ts` | 客户端 `uploading` 状态模型（00 §5.1：只存在于客户端） | F-S3-01 |
| `queue/queue.css` | 队列面板样式 | F-S3-01 |
| `details/FileDetailsPane.tsx` | 右侧详情栏（标签页宿主，宽度可拖） | F-S3-04 |
| `details/DetailsOverviewTab.tsx` | 概览：状态历史、版本、解析信息、LLM 状态 | F-S3-04 |
| `details/DetailsBlocksTab.tsx` | 解析结果：块列表 + 锚点 + 预览定位 | F-S3-04 |
| `details/DetailsEntitiesTab.tsx` | 实体：识别出的实体与事实、证据 | F-S5-02 |
| `details/DetailsSharesTab.tsx` | 被哪些 Share 用到 | F-S6-04 |
| `details/EvidenceSnippet.tsx` | 证据片段（引文高亮 + 锚点 + “在预览中定位”） | F-S3-04 |
| `details/EntityDetailView.tsx` | 实体详情（事实、双时间、别名、取代历史、证据跳转） | F-S5-02 |
| `details/details.css` | 详情栏样式 | F-S3-04 |
| `review/ReviewQueuePage.tsx` | 待确认队列页 | F-S5-01 |
| `review/ReviewGroup.tsx` | 分组（按来源文件或按种类）与“整体接受/拒绝” | F-S5-01 |
| `review/ReviewItemRow.tsx` | 单项（展开看证据，行内决定，**不弹窗**） | F-S5-01 |
| `review/ReviewBatchBar.tsx` | 批量操作条 | F-S5-01 |
| `review/reviewKinds.ts` | 各种类的主体渲染与可用决定 | F-S5-01 |
| `review/RevisionView.tsx` | M2 修订模式（删除线/插入高亮、按建议或按来源接受） | F-S8-01 |
| `review/review.css` | 样式 | F-S5-01 |
| `share/SuperDriveShareDialog.tsx` | 分享面板（范围 → 设置 → 交付三步） | F-S6-02 |
| `share/ShareOptionsStep.tsx` | 角色、预算、有效期、任务描述、BRIEF 预览 | F-S6-02 |
| `share/ShareHandoffStep.tsx` | 命令行、复制、一键执行、结果 | F-S6-02 |
| `share/ShareListPage.tsx` | 共享列表（含 `refreshed_from` 链） | F-S6-03 |
| `share/ShareRevokeDialog.tsx` | 撤销确认（写明“Agent 已读内容无法收回”） | F-S6-03 |
| `share/BriefPreview.tsx` | BRIEF 只读预览，`[src:…]` 可点并定位到证据 | F-S6-05 |
| `share/share.css` | 样式 | F-S6-02 |
| `engine/EngineStatusPage.tsx` | 引擎页：状态、preflight、组件、磁盘、版本、日志 | F-S2-06 |
| `engine/EngineStatusPill.tsx` | 工具栏里的状态胶囊，点开引擎抽屉 | F-S2-06 |
| `engine/PreflightChecklist.tsx` | Docker / oo / 平台 / 磁盘检查清单 | F-S2-06 |
| `engine/ComponentList.tsx` | db、parser、model、cas、engine 组件状态 | F-S2-06 |
| `engine/StorageMeter.tsx` | 磁盘与 CAS 占用 | F-S4-04 |
| `engine/engine.css` | 样式 | F-S2-06 |
| `settings/DriveSettingsPage.tsx` | Drive 列表与编辑 | F-S2-06 |
| `settings/DriveRegisterDialog.tsx` | 登记 Drive（fs；S4 起 s3；S11 起 oss/cos/r2） | F-S2-06 |
| `settings/ModelSettingsSection.tsx` | 每个 Drive 的模型开关与日上限；引擎模型配置入口 | F-S5-03 |
| `settings/UsageSection.tsx` | LLM 用量（按天、按 Drive） | F-S5-03 |
| `settings/settings.css` | 样式 | F-S2-06 |
| `onboarding/SuperDriveEntryState.tsx` | 首次使用与不可用空态（复用 `src/components/DesktopEntryState.tsx`） | F-S2-06 |
| `onboarding/onboarding.css` | 样式 | F-S2-06 |

文件行数预算：单个 `.tsx` ≤ 400 行、单个 `.css` ≤ 450 行（参照 `scripts/check-cloud-frontend-architecture.mjs:30-58` 对 cloud 的做法），由 §6.4 的架构测试检查。

#### 1.2.2 preload、类型与共享契约

| 文件 | 改动 | 说明 |
|---|---|---|
| `electron/preload.cjs` | 追加 `superOntology` 命名空间（§1.5） | 只在主进程以 `--puppyone-super-drive=1` 启动参数告知启用时暴露（同 `preload.cjs:3` 的 `gitAutoCommitAvailable` 做法）；flag 关闭时 `window.puppyoneDesktop.superOntology` 为 `undefined` |
| `src/types/electron.d.ts` | 追加 `superOntology?: SuperOntologyBridge` | 类型来自 `shared/super-ontology/bridge.d.ts` |
| `shared/super-ontology/bridge.d.ts` | 新增 | IPC 请求/响应类型（与生成的 `docsApi` 类型组合） |
| `shared/super-ontology/requests.mjs` + `.d.mts` | 新增 | IPC 请求解析与校验（`parseSuperOntologyRequest(channel, raw)`，模式同 `shared/model-connections/schema.mjs`） |
| `shared/super-ontology/errors.mjs` | 新增 | 主进程侧错误码 `SO_*` 与 `DOCS_*` 透传规则 |
| `contracts/docs-api/docs-api.v1.schema.json`、`errors.v1.json`、`events.v1.schema.json` | 从 OO 仓库按 sha 复制 | 真相源在 OO（10 §8.7）；Puppyone 侧只读副本 |
| `contracts/docs-api/SOURCE.json` | 新增 | `{"repository":"open-ontology","commit":"<sha>","x-docs-api-version":"1.0"}` |
| `scripts/generate-super-drive-contract.mjs` | 新增 | 从 schema 生成 `docsApi.generated.ts` 与测试 mock 夹具；`--check` 模式比对 |

#### 1.2.3 主进程 `electron/main/super-ontology/` 与 IPC

| 文件 | 职责 |
|---|---|
| `service.mjs` | 组合根 `createSuperOntologyService({ platformHost, paths, logger, broadcast, dialog, shell, fetchImpl })`；空闲时不做任何事（不探测、不起进程），第一次 IPC 调用才初始化 |
| `engine-locator.mjs` | 经 `electron/main/platform/common/command-environment.mjs` 的 `readUserCommandEnvironment` 得到用户 PATH，查找 `docker` 与 `oo`；支持用户手动指定 `oo`（grant）；`oo --version --json` 读版本 |
| `docker-preflight.mjs` | 用 `electron/main/platform/common/bounded-process-probe.mjs` 的 `runBoundedProcessProbe` 跑 `docker version --format '{{json .}}'` 与 `docker info --format '{{json .}}'`（超时 4 s）；`fs.statfs(<stateDir>)` 读余量 |
| `engine-lifecycle.mjs` | 引擎状态机（§4.4）：`oo serve --status --json` 判定可复用实例 → 否则 `oo serve --state-dir <stateDir> --port 0 --json` 常驻子进程 → 等 `serve.json` → 轮询 `/livez` 与 `GET /v1/docs/health`；退出码映射（10 §8.6.9）；崩溃退避重启（5 s → 15 s → 45 s，10 分钟内 3 次后转 `crashed` 需手动）；`app.before-quit` 时对自己拉起的实例发 SIGTERM（用户可选“退出后保持引擎运行”= `--keep-engine`） |
| `serve-credentials.mjs` | 读 `<stateDir>/serve.json`（protocol `oo.docs.serve/v1`）与 `token_file`；校验：路径在 stateDir 内、`O_NOFOLLOW`、属主为当前用户、权限 `0600`；token 只存在闭包里；每次引擎重启后重读（token 每次启动轮换，10 §8.6.4） |
| `control-client.mjs` | 路由白名单（只有 10 §8.3 中 Desktop 用到的模板）；路径参数按 `Uuid`/`DrivePath` 校验后 `encodeURIComponent`；头：`Authorization`、`OO-Docs-Api-Min: 1.0`、`Idempotency-Key`（POST 有副作用时必带）、`If-Match: "r<revision>"`（PATCH/DELETE）；不发 `Origin`；超时：读 10 s、写 30 s、`POST /shares` 120 s；响应体上限 8 MiB；解析 `OO-Docs-Api` 头做版本检查；错误归一为 `{code, retryable, details, httpStatus}`（丢弃 `message`，只写日志）；`429` 按 `Retry-After` 退避一次 |
| `event-bridge.mjs` | SSE 客户端（§1.6） |
| `grants.mjs` | `issue(sender, purpose, canonicalPath) → {grantId, displayPath}`、`resolve(sender, grantId, purpose)`；`purpose ∈ drive-root / mount-target / oo-executable`；按 WebContents 存在 `WeakMap` 里，用一次即失效（mount-target 除外，可用于同一对话框内重试） |
| `secret-files.mjs` | 把 S3 密钥与模型 API key 写到 `<stateDir>/../secrets/<uuid>`（0600，`O_EXCL`），返回 `{"secret_ref":"file:<绝对路径>"}`；删除 Drive 时一并删除；渲染层只送一次明文，不回显 |
| `upload-service.mjs` | fs Drive 上传：源路径来自 preload 的 `webUtils.getPathForFile`；目标 = Drive 根 + `DrivePath`（规范化，禁止 `..`、符号链接穿越）；写 `.<name>.puppyone-upload-<id>.part`（10 §4.3.2 会忽略点开头与 `*.part`），完成后 `rename` 原子替换；重名按“名称 (2).ext”；并发 2；每批完成后防抖 1 s 调 `POST /drives/{id}:sync`；进度推送 `super-ontology:upload-progress` |
| `drive-file-protocol.mjs` | 注册 `puppyone-drive:` 协议：能力 token → `(driveRoot, relativePath)`，只读、`Cache-Control: no-store`、`X-Content-Type-Options: nosniff`、支持 `Range`（复用 `local-api/files/byte-range.mjs` 的 `parseSingleByteRange`）；只服务 fs Drive；s3 Drive 的预览见 §8.2 Q6 |
| `snapshot.mjs` | 组装 `EngineSnapshot`（§1.5.1）与对外的脱敏副本 |
| `electron/main/ipc/super-ontology-ipc.mjs` | `registerSuperOntologyIpcHandlers({ ipcMain, service })`：只在 trusted-ipc 门面上注册；每个通道先 `parseSuperOntologyRequest` 再调服务；返回统一信封（§1.5.2） |

接入现有文件（主线程独占，见 §1.3）：`electron/main.mjs` 在 `registerIpcHandlers()`（`main.mjs:1028`）中加一行 `registerSuperOntologyIpcHandlers({ ipcMain: trustedIpcMain, service: superOntologyService })`，在服务组装处创建 `superOntologyService`，在 `before-quit` 时调用 `superOntologyService.shutdown()`；`puppyone-drive:` 协议需要在 `app.ready` 前 `protocol.registerSchemesAsPrivileged`。

`electron/main.mjs` 不得出现 `process.platform`（`scripts/check-desktop-platform-architecture.mjs:57`）：平台差异（Windows 不支持、macOS 的 Docker Desktop 路径）全部在 `engine-locator.mjs` 里经平台宿主处理。

#### 1.2.4 本地化、测试与其他

| 路径 | 内容 |
|---|---|
| `locales/renderer/{en,es,pt-BR,fr,de,ja,ko,zh-Hans}/super-drive.json` | 新 namespace（§5） |
| `src/localization/catalog-loaders/{8 种语言}.ts` | 各加一行 `import superDrive from …/super-drive.json` 与 `"super-drive": superDrive` |
| `locales/native/{8 种语言}.json` | 原生选择框标题：`native.superDrive.pickDriveRoot.title`、`native.superDrive.pickMountTarget.title`、`native.superDrive.pickOoExecutable.title` |
| `locales/renderer/*/settings.json` | 实验功能开关文案 `experimental.superDrive.*`（随 `ExperimentalSettingsView.tsx` 的 `messageKey` 约定） |
| `tests/{unit,component,integration,architecture}/super-drive/<capability>/` | 见 §6 |
| `tests/e2e/super-drive/<capability>/*.smoke.mjs` | T1 场景 |
| `tests/e2e/super-drive/packaged/`（建议路径，见 §8.2 Q9） | T2 打包黑盒场景 |
| `tests/support/super-drive/` | 生成的 mock、真实引擎拉起脚本、Peekaboo 与 CDP 辅助 |

### 1.3 对现有文件的最小改动清单

| 文件 | 改动 | 归属 |
|---|---|---|
| `src/features/flags/registry.ts` | `FEATURE_FLAG_DEFAULTS` 加 `superDrive: false` | 执行者（F-S1-01） |
| `src/features/flags/flags.json` | 加 `"superDrive": false`（验收构建置 `true`） | **主线程独占** |
| `src/features/app-shell/workspace-surfaces/workspaceSurfaceRegistry.ts` | 加 `defineWorkspaceSurface({ id: "super-drive", navigation: { labelId: "super-drive.navigation.label", group: "workspace", order: 15 }, lifecycle: { sidebar: "on-demand", main: "on-demand" }, isAvailable: ({ superDriveEnabled }) => superDriveEnabled })` | **主线程独占** |
| `src/features/app-shell/workspace-surfaces/workspaceSurfaceTypes.ts` | `WorkspaceSurfaceId` 加 `"super-drive"`；`WorkspaceSurfaceCapabilities` 加 `superDriveEnabled: boolean` | 执行者（F-S1-02） |
| `src/features/app-shell/workspace-surfaces/useWorkspaceSurfaceContent.tsx` | 计算 `superDriveEnabled = useFeatureFlag("superDrive") && preferences.experimentalSettings.enableSuperDrive`；`adapters["super-drive"] = () => createSuperDriveWorkspaceSurface({ workspace })` | 执行者（F-S1-02） |
| `src/features/app-shell/navigation/navigationModel.tsx`、`navigation/types.ts` | `DESKTOP_NAV_ITEMS` 追加 `{ view: "super-drive", labelId: "super-drive.navigation.label", icon: SuperDriveNavigationIcon }`；`DesktopNavigationItem.view` 放宽为 `Extract<DesktopView, "git" \| "super-drive">`；`getDesktopNavigationBadge` 对 `super-drive` 返回待确认计数（只显示点，数字在 Super Drive 内部） | 执行者（F-S1-02） |
| `src/preferences.ts`、`src/features/settings/main/ExperimentalSettingsView.tsx` | `ExperimentalSettings` 加 `enableSuperDrive`（默认 `false`）；实验功能列表在 `superDrive` flag 为真时显示该项（60 §4 第 2 步） | 执行者（F-S1-02） |
| `src/features/cloud/share/` | 抽出 `ShareStepper`、`ScopeOption`、`ModeOption`、`CopyField` 到 `ShareWizardParts.tsx` 并从 `share/index.ts` 导出（纯移动，不改行为） | 执行者（F-S6-01），见 §8.2 Q4 |
| `electron/main.mjs` | 服务组装、IPC 注册、协议注册、退出钩子、preload 启动参数 | **主线程独占** |
| `electron/main/trusted-ipc.mjs` 注册表 | 只通过 `createTrustedIpcMain` 返回的门面注册，不改该文件 | **主线程独占**（如需改） |
| `package.json` | 新脚本 `check:super-drive-contract`、`test:super-drive:e2e`、`test:super-drive:packaged`；把 `check:super-drive-contract` 串进 `check:boundaries`；`puppyoneCapabilities.superDrive`（§1.10） | **主线程独占** |
| `tests/config/discovery.mjs` | `testDomains` 加 `"super-drive"`（否则 `scripts/check-test-layout.mjs` 拒绝新目录） | 建议列为主线程独占（§8.2 Q10） |
| `tests/support/runners/run-desktop-e2e.mjs`、`scripts/release-checks/checks.json` | 注册 T1 场景（60 §3.1） | 建议列为主线程独占（§8.2 Q10） |
| `shared/desktop/application-identity.mjs`、`shared/desktop/release-identity.mjs`（渠道枚举）、`electron/main/build-info-service.mjs` | 验收版渠道与用户数据目录覆盖（§1.10.2） | 建议列为主线程独占（§8.2 Q8） |
| `locales/manifest.json` | **不改**（namespace 不在 manifest 里登记；8 种语言列表不变） | — |

`check-sidebar-architecture.mjs:113` 只要求注册表中保留 `data/git/cloud/settings` 四个 id、不得出现 `plugins/access/automation/agent/terminal`，加入 `super-drive` 不冲突。

### 1.4 主进程 `super-ontology` 服务

#### 1.4.1 引擎生命周期管理器（`engine-lifecycle.mjs`）

状态由主进程计算，渲染层只读（§4.4 是它的呈现）。

```
               ┌──────────────────────── preflight ─────────────────────────┐
 (idle) ──first IPC──▶ probing ──▶ unsupported_platform | oo_missing | docker_missing | docker_stopped
                          │                                                   ▲ 用户点“重试”
                          ▼ 全部通过
                       stopped ──start──▶ starting ──health ok──▶ healthy ⇄ degraded
                          ▲                  │  ▲                     │
                          │                  │  └── restarting ◀──────┤ 进程退出 / health unavailable
                          │                  ▼                        │   （退避 5/15/45 s，10 分钟 3 次）
                          │              crashed（需手动）◀────────────┘
                          │              incompatible（版本或 schema 不符，需升级）
                          └──stop── stopping ◀── healthy / degraded
```

- **复用**：先跑 `oo serve --status --json --state-dir <stateDir>`；有存活实例且 `api_version` 主版本为 1 → 直接读 `serve.json` 接管（`ownedByDesktop=false`，退出时不停它）。
- **启动**：`oo serve --state-dir <stateDir> --port 0 --json`，用 `child_process.spawn`（`detached: false`，stdout 只解析首行 JSON，stderr 截断后写日志文件 `<userData>/logs/super-ontology-serve.log`，轮转 5 MiB × 3）；等待 `serve.json` 出现（≤ 150 s，覆盖 10 §8.6.3 的 120 s 引擎等待）；随后 `GET /livez` 与 `GET /v1/docs/health`。
- **退出码映射**（10 §8.6.9）：`3` → `docker_stopped`（`DOCS_ENGINE_DOCKER_UNAVAILABLE`）；`4` → `crashed(SO_SERVE_IMAGE_INVALID)`；`5` → `crashed(SO_SERVE_STATE_DIR_INVALID)`；`6` → 换端口重试一次后 `crashed(SO_SERVE_PORT_IN_USE)`；`7` → `crashed(SO_SERVE_START_TIMEOUT)`；`1`/信号 → `restarting`。
- **健康巡检**：每 10 s `GET /v1/docs/health`；SSE 的 `engine.health_changed` 事件到达时立刻更新，不等轮询。`status=starting` 或 `503 DOCS_ENGINE_STARTING` → `starting`；`status=unavailable` 或 `503 DOCS_ENGINE_UNAVAILABLE`（守护进程活着、引擎容器不可达，守护进程自己在 5–60 s 退避 `compose up`，10 §4.12）→ `restarting(reason=engine_container)`；`503 DOCS_ENGINE_SCHEMA_MISMATCH` 或主版本不符 → `incompatible`。
- **Drive 变更重建容器**：登记或删除本地 Drive 会让守护进程重建 `docs-engine` 容器（10 §2.1 理由 2、§14 风险表），期间 SSE 断开几秒。主进程在发出 `POST/DELETE drives` 后把快照标为 `restarting(reason=drive_change)`，直到 health 恢复，界面显示“正在应用 Drive 变更”。
- **退出**：`before-quit` 时若 `ownedByDesktop` 且未开“保持引擎运行”→ SIGTERM，最多等 12 s（守护进程自己最多等 10 s 在途请求，10 §8.6.7），超时 SIGKILL；开启时传 `--keep-engine` 启动、退出时不发信号。
- **状态目录**：`<userData>/super-ontology/state`（通过 `--state-dir` 传入，10 §14 开放问题 2）。验收版与正式版因 `userData` 不同而天然隔离（§1.10.2），compose 项目名 `oo-docs-<hash>` 由状态目录派生，互不干扰。

#### 1.4.2 控制 API 客户端（`control-client.mjs`）

| 关注点 | 规则 |
|---|---|
| 基址 | `http://127.0.0.1:${serve.port}`；`port` 必须是 1–65535 的整数；`Host` 头由 fetch 按基址生成，满足 10 §8.1 的 Host 校验 |
| 鉴权 | `Authorization: Bearer <oodt_…>`；错误对象、日志、IPC 返回值中都不出现 token；`401 DOCS_ENGINE_UNAUTHENTICATED` → 重读 token 文件重试一次（引擎重启轮换），仍失败 → 引擎快照转 `restarting` |
| 版本 | 每个请求带 `OO-Docs-Api-Min: 1.0`；响应头 `OO-Docs-Api` 主版本 ≠ 1 → `SO_ENGINE_INCOMPATIBLE`；未知枚举值不报错，按“未知”呈现（10 §8.1 版本行） |
| 幂等 | 有副作用的 POST（`drives`、`:sync`、`files:retry`、`review:decide`、`context-packs:compile`、`shares`、`:refresh`、`:revoke`）带 `Idempotency-Key = operationId`；渲染层超时后重发同一 `operationId`，服务端重放原响应（`OO-Idempotent-Replay: true`）；`POST /resolve` 是只读 POST，不带 |
| 并发 | `PATCH/DELETE /drives/{id}` 带 `If-Match: "r<revision>"`；`412` 时把 `details.current_revision` 与最新 Drive 一起返回，渲染层提示“已被其他地方修改”并给出最新值 |
| 批量 | `files:retry` 每批 ≤ 500、`review:decide` 每批 ≤ 500：主进程负责切批，按批顺序提交，合并逐项结果返回；任一批失败不回滚已成功的批（逐项结果如实返回） |
| 超时 | GET 10 s；写 30 s；`POST /shares` 120 s（含物理挂载）；`POST /context-packs:compile` 60 s |
| 错误 | 网络错误/连接拒绝 → `SO_ENGINE_UNREACHABLE`（retryable）；非 JSON 或不合 schema 的响应 → `SO_RESPONSE_INVALID`；`DOCS_*` 原样透传 `code/retryable/details`，`message` 只写日志 |
| 响应校验 | 用生成的守卫校验关键字段（`FileSummary.state`、`Share.state` 等）；未知枚举保留原值并打 `unknown` 标记 |

#### 1.4.3 SSE → IPC 事件桥（`event-bridge.mjs`）

```
 oo serve /v1/docs/events ──SSE──▶ event-bridge（主进程，每个引擎实例一条连接）
   since=<lastSeq> 或 Last-Event-ID                │
   持久事件（有 id）：按 seq 去重、单调推进 lastSeq  ├─▶ ring buffer（最近 5,000 条）
   瞬时事件（无 id）：file.progress、stream.lagging  ├─▶ 每 50 ms 合并一次推送 super-ontology:events
   410 DOCS_ENGINE_CURSOR_EXPIRED ──────────────────┴─▶ 推送 super-ontology:resync-required
```

| 规则 | 说明 |
|---|---|
| 连接时机 | 引擎进入 `healthy/degraded` 且至少一个窗口订阅后连接；没有订阅者 60 s 后断开（主进程仍保留 `lastSeq`） |
| 游标 | `lastSeq` 只由持久事件推进；初次连接用 `health.events_head` 作为起点（之前的状态由各列表的 `as_of_seq` 对齐，§1.7.4） |
| 去重 | `seq <= lastSeq` 的事件丢弃；`?types=` 过滤导致的跳号不视为缺口（10 §8.5 末段） |
| 重连 | 断开后退避 1 s、2 s、5 s、10 s、30 s（上限，带 ±20% 抖动）；服务端 `retry:` 字段若更大则用服务端值；每次重连带 `since=lastSeq` |
| 410 | 收到 `DOCS_ENGINE_CURSOR_EXPIRED`：清空环形缓冲，推送 `resync-required { reason: "cursor_expired", headSeq }`；渲染层全量重拉（§1.7.4），主进程从 `details.head_seq` 重新订阅 |
| `stream.lagging` | 服务端随后会关闭连接；按普通断线处理（带 `since` 重连），不推给渲染层 |
| `file.progress` | 只转发，不进环形缓冲，不补齐；渲染层用它更新进度条；断线期间丢失的进度由下一次 `file.state_changed` 或 `GET /files/{fid}` 覆盖 |
| `files.bulk_changed` | 转发；渲染层把对应 Drive 的已加载目录标“需重拉” |
| 引擎实例变化 | `serve.json.started_at` 或 `pid` 变化 = 新实例；事件序号可能重置 → 视同 410 处理 |
| 推送合并 | 50 ms 窗口合并为 `{ instanceId, events: Event[], lastSeq }`；单批 ≤ 1,000 条，超出分批 |
| 连接状态 | `connected / reconnecting / resyncing / offline` 经 `super-ontology:connection-changed` 推送，含 `attempt`、`nextRetryAt`、`lastSeq` |
| 窗口补齐 | 渲染层（重新）挂载或窗口重开时调 `events-since { seq }`：环形缓冲覆盖则返回缺失事件；不覆盖 → `SO_EVENTS_GAP`，渲染层全量重拉 |

#### 1.4.4 token 与凭证读取（`serve-credentials.mjs`）

1. `serve.json` 路径固定为 `<stateDir>/serve.json`，读取前 `lstat` 确认不是符号链接、属主为 `process.getuid()`、`mode & 0o077 === 0`。
2. 校验 `protocol === "oo.docs.serve/v1"`、`api_version` 主版本 1、`token_file` 是 `<stateDir>` 下的绝对路径。
3. 用 `fs.open(token_file, O_RDONLY | O_NOFOLLOW)` 读，同样校验属主与 `0600`，内容匹配 `^oodt_[A-Za-z0-9_-]{43}$`。
4. token 只存在 `control-client` 闭包里；`snapshot.mjs` 输出的快照不含 `token_file` 路径与 pid 以外的进程细节。
5. 任一校验失败 → `crashed(SO_SERVE_CREDENTIALS_INVALID)`，界面提示“引擎凭证文件权限异常”，提供“打开状态目录”。

### 1.5 IPC 通道清单

#### 1.5.1 约定

- 通道名 `super-ontology:<动作>`；全部用 `ipcMain.handle`（经 trusted-ipc 门面，只接受应用主框架，`electron/main/trusted-ipc.mjs:51-63`），推送用 `webContents.send`。
- 请求对象一律带 `operationId?: string`（有副作用的调用必带）；主进程用它作 `Idempotency-Key`。
- 渲染层看到的类型名与 10 §8.2 的 `$defs` 同名（`Drive`、`FileSummary`、`FileDetail`、`Block`、`Entity`、`ReviewItem`、`ReviewGroup`、`ContextPack`、`Share`、`RevokeResult`、`Usage`、`Event`），字段是 snake_case 原样；Desktop 自有类型用 camelCase（`EngineSnapshot`、`UploadProgress`、`Grant`）。

`EngineSnapshot`（主进程计算）：

```ts
type EngineSnapshot = {
  revision: number;                       // 单调递增，渲染层丢弃旧快照
  status: "idle" | "probing" | "unsupported_platform" | "oo_missing" | "docker_missing" | "docker_stopped"
        | "stopped" | "starting" | "healthy" | "degraded" | "restarting" | "crashed" | "incompatible" | "stopping";
  reason: string | null;                  // 例如 "drive_change"、"engine_container"、"process_exit"
  code: string | null;                    // SO_* 或 DOCS_*
  preflight: { platform: "ok" | "unsupported"; oo: { found: boolean; version: string | null; source: "path" | "user" | null };
               docker: { installed: boolean; running: boolean; version: string | null }; diskFreeBytes: number | null };
  health: Health | null;                  // 10 §8.2 Health 原样（含 components、queue、disk、events_head）
  restarts: { count: number; nextAttemptAt: string | null };
  ownedByDesktop: boolean;
  keepEngineOnQuit: boolean;
  startedAt: string | null;
  connection: { state: "connected" | "reconnecting" | "resyncing" | "offline"; attempt: number; nextRetryAt: string | null; lastSeq: number };
};
```

#### 1.5.2 返回信封

```ts
type SuperOntologyResult<T> =
  | { ok: true; value: T; replayed?: boolean }                 // replayed = OO-Idempotent-Replay
  | { ok: false; error: { code: string; retryable: boolean; details: Record<string, unknown>; httpStatus?: number } };
```

`details` 只透传 10 §8.4 表中为该码列出的字段（白名单），其余丢弃。主进程永不抛出原生 Error 给渲染层（同 `electron/main/ipc/model-connections-ipc.mjs:9-14` 的做法）。

#### 1.5.3 通道表

| 通道 | 请求 | 返回 `value` | 对应 OO 调用（10 §8.3） | 典型错误 |
|---|---|---|---|---|
| `engine-read` | `{}` | `EngineSnapshot` | 无 / `GET /v1/docs/health` | — |
| `engine-preflight` | `{}` | `EngineSnapshot` | 本机探测 | `SO_PLATFORM_UNSUPPORTED`、`SO_OO_MISSING`、`SO_DOCKER_MISSING`、`SO_DOCKER_STOPPED` |
| `engine-start` | `{ keepEngineOnQuit?: boolean }` | `EngineSnapshot`（`starting`） | `oo serve …` | 同上 + `SO_SERVE_*` |
| `engine-stop` | `{}` | `EngineSnapshot` | SIGTERM | `SO_ENGINE_NOT_OWNED`（复用的外部实例不能由 Desktop 停） |
| `engine-restart` | `{}` | `EngineSnapshot` | stop + start | 同上 |
| `engine-reveal-logs` | `{}` | `{ revealed: true }` | 在访达中显示日志文件 | — |
| `engine-set-executable` | `{ grantId }` | `EngineSnapshot` | `oo --version --json` | `SO_GRANT_REQUIRED`、`SO_OO_INVALID` |
| `pick-path` | `{ purpose: "drive-root" \| "mount-target" \| "oo-executable" }` | `{ grantId, displayPath } \| { cancelled: true }` | 原生选择框 | — |
| `drives-list` | `{}` | `Drive[]`（主进程翻完全部页） | `GET /drives` | 引擎类 |
| `drive-get` | `{ driveId }` | `Drive` | `GET /drives/{id}` | `DOCS_SOURCE_DRIVE_NOT_FOUND` |
| `drive-create` | `{ operationId, name, backend: "fs", rootGrantId, exclude?, modelEnabled?, llmDailyCalls? }` 或 `{ …, backend: "s3"\|"oss"\|"cos"\|"r2", endpoint, region?, bucket, prefix?, pathStyle?, accessKeyId, secretAccessKey, sessionToken? }` | `Drive` | `POST /drives`（主进程把 grant 换成 `config.root`，把密钥写成 `file:` secret_ref） | `DOCS_SOURCE_DRIVE_EXISTS`、`_ROOT_MISSING`、`_AUTH_FAILED`、`_BUCKET_NOT_FOUND`、`_CREDENTIAL_MISSING`、`_ENDPOINT_INVALID` |
| `drive-update` | `{ driveId, revision, patch: DrivePatch }` | `Drive` | `PATCH /drives/{id}`（If-Match） | `DOCS_ENGINE_PRECONDITION_FAILED` |
| `drive-delete` | `{ driveId, revision }` | `Drive`（`status=deleting`） | `DELETE /drives/{id}?purge=false`（If-Match） | `DOCS_SOURCE_DRIVE_HAS_ACTIVE_SHARES` |
| `drive-sync` | `{ operationId, driveId, deep?, confirmMassDelete? }` | `SyncInfo` | `POST /drives/{id}:sync` | `DOCS_SOURCE_SYNC_RUNNING`（视为成功）、`DOCS_SOURCE_MASS_DELETE_HELD` |
| `drive-tree` | `{ driveId, path }` | `{ dirs, files: Page<FileSummary> }` | `GET /drives/{id}/tree` | — |
| `files-list` | `{ driveId, path?, recursive?, states?, q?, sort?: "path"\|"state_since", limit?, cursor? }` | `Page<FileSummary>` | `GET /drives/{id}/files` | `DOCS_ENGINE_CURSOR_INVALID` |
| `file-detail` | `{ fileId, include?: ("history"\|"entities"\|"facts"\|"shares")[] }` | `FileDetail` | `GET /files/{fid}` | `DOCS_ENGINE_NOT_FOUND` |
| `file-blocks` | `{ fileId, page?, limit?, cursor? }` | `Page<Block>` | `GET /files/{fid}/blocks` | `DOCS_PARSE_NOT_READY` |
| `files-retry` | `{ operationId, fileIds: string[] (≤ 5,000，主进程切批), mode?: "normal"\|"safe" }` | `{ results: {id,status,code?}[] }` | `POST /files:retry` | `413` 不会出现（已切批） |
| `file-preview-url` | `{ driveId, path }` | `{ url, expiresAt }` | 本机（`puppyone-drive:`） | `SO_PREVIEW_UNAVAILABLE`（s3） |
| `file-reveal` | `{ driveId, path }` | `{ revealed: true }` | `shell.showItemInFolder`（仅 fs） | — |
| `upload-start` | preload 内部：`{ operationId, driveId, targetPath, sourcePaths }` | `{ batchId, items: { itemId, name, size }[] }` | 本机复制 + `:sync` | `SO_UPLOAD_SOURCE_MISSING`、`SO_DISK_FULL`、`SO_PERMISSION_DENIED`、`SO_UPLOAD_UNSUPPORTED_BACKEND` |
| `upload-cancel` | `{ batchId, itemId? }` | `{ cancelled: number }` | 本机 | — |
| `entities-search` | `{ q, driveId?, type?, limit?, cursor? }` | `Page<EntityRef & counts>` | `GET /entities` | — |
| `entity-detail` | `{ entityIdOrKey, includeHistory? }` | `Entity` | `GET /entities/{eid}` | `DOCS_ENGINE_NOT_FOUND` |
| `resolve` | `{ text, driveId?, types?, limit? }` | `ResolveResult` | `POST /resolve` | — |
| `review-list` | `{ driveId, kind?, source?, status?, groupBy?: "source", limit?, cursor? }` | `Page<ReviewItem>` 或 `Page<ReviewGroup>` | `GET /review` | — |
| `review-decide` | `{ operationId, items?: {id,revision,decision,edit?,note?}[] (≤ 5,000), bySource?: { fileId, kinds?, decision, asOfSeq } }` | `ReviewDecideResult`（合并后） | `POST /review:decide` | `DOCS_REVIEW_*` 逐项 |
| `pack-compile` | `{ operationId, driveId, scope, task?, seeds?, budget, role, version? }` | `ContextPack`（含 `brief_preview`） | `POST /context-packs:compile` | `DOCS_PACK_*` |
| `pack-read` | `{ packId }` | `ContextPack` | `GET /context-packs/{pid}` | `DOCS_PACK_NOT_FOUND` |
| `shares-list` | `{ driveId?, states?, limit?, cursor? }` | `Page<Share>` | `GET /shares` | — |
| `share-get` | `{ shareId }` | `Share` | `GET /shares/{sid}` | `DOCS_SHARE_NOT_FOUND` |
| `share-mount` | `{ operationId, compile: CompileRequest, targetGrantId, expiresInSeconds }` | `Share` | `POST /shares`（主进程把 grant 换成 `mount.target_dir`） | `DOCS_SHARE_TARGET_OCCUPIED`、`_TARGET_INVALID`、`_TARGET_WRITE_FAILED`、`_ROLE_UNAVAILABLE`、`_EXPIRY_INVALID`、`DOCS_PACK_*` |
| `share-refresh` | `{ operationId, shareId }` | `Share`（新的 active，`refreshed_from=shareId`） | `POST /shares/{sid}:refresh` | `DOCS_SHARE_INVALID_TRANSITION` |
| `share-revoke` | `{ operationId, shareId, reason? }` | `RevokeResult` | `POST /shares/{sid}:revoke` | `DOCS_SHARE_NOT_FOUND` |
| `share-reveal` | `{ shareId }` | `{ revealed: boolean }` | `shell.showItemInFolder(share.mount.target_dir)` | — |
| `usage-read` | `{ driveId?, days? }` | `Usage` | `GET /usage` | — |
| `ontology-versions` | `{ limit?, cursor? }` | `Page<{n,as_of,…}>` | `GET /ontology/versions` | — |
| `model-config-read` / `model-config-save` | 见 §8.2 Q5 | — | 待后端提供 | — |
| `events-subscribe` | `{}` | `{ instanceId, lastSeq, connection }` | 启动 SSE | — |
| `events-since` | `{ instanceId, seq }` | `{ events: Event[], lastSeq }` | 主进程环形缓冲 | `SO_EVENTS_GAP` |
| `events-unsubscribe` | `send`，`{}` | — | — | — |

推送通道：

| 通道 | 载荷 | 说明 |
|---|---|---|
| `super-ontology:engine-changed` | `EngineSnapshot` | 状态或快照变化即推 |
| `super-ontology:events` | `{ instanceId, events: Event[], lastSeq }` | 50 ms 合并；含瞬时事件（`file.progress`，无 `seq`） |
| `super-ontology:connection-changed` | `EngineSnapshot["connection"]` | — |
| `super-ontology:resync-required` | `{ instanceId, reason: "cursor_expired" \| "engine_restarted" \| "gap" }` | 渲染层全量重拉 |
| `super-ontology:upload-progress` | `{ batchId, items: { itemId, state, bytesDone, bytesTotal, code?, drivePath? }[] }` | ≤ 4 次/秒 |

preload 形状（`window.puppyoneDesktop.superOntology`）：

```js
superOntology: {
  invoke: (channel, request) => ipcRenderer.invoke(`super-ontology:${channel}`, request), // channel 在 preload 内按白名单校验
  upload: ({ operationId, driveId, targetPath, files }) => ipcRenderer.invoke("super-ontology:upload-start", {
    operationId, driveId, targetPath, sourcePaths: files.map((file) => webUtils.getPathForFile(file)),
  }),
  subscribe: (callback) => { /* 同时监听上述 5 个推送通道，返回取消函数 */ },
}
```

preload 的通道白名单与 `shared/super-ontology/requests.mjs` 的通道表共用一份常量，避免两处漂移（`renderer-code-organization.md` 记录的“契约只写一份”方向）。

### 1.6 错误模型

| 来源 | 码 | 渲染层处理 |
|---|---|---|
| 引擎（10 §8.4） | `DOCS_*` | `superDriveErrors.ts` 按码取文案 key `super-drive.errors.<CODE>`；没有专门文案的码按前缀族回退 `super-drive.errors.family.<FAMILY>`；`retryable` 决定是否显示“重试” |
| 主进程 | `SO_BRIDGE_UNAVAILABLE`（渲染层自生）、`SO_INVALID_REQUEST`、`SO_PLATFORM_UNSUPPORTED`、`SO_OO_MISSING`、`SO_OO_INVALID`、`SO_DOCKER_MISSING`、`SO_DOCKER_STOPPED`、`SO_SERVE_IMAGE_INVALID`、`SO_SERVE_STATE_DIR_INVALID`、`SO_SERVE_PORT_IN_USE`、`SO_SERVE_START_TIMEOUT`、`SO_SERVE_CREDENTIALS_INVALID`、`SO_ENGINE_NOT_RUNNING`、`SO_ENGINE_UNREACHABLE`、`SO_ENGINE_INCOMPATIBLE`、`SO_ENGINE_NOT_OWNED`、`SO_TIMEOUT`、`SO_RESPONSE_INVALID`、`SO_GRANT_REQUIRED`、`SO_EVENTS_GAP`、`SO_UPLOAD_SOURCE_MISSING`、`SO_UPLOAD_CONFLICT`、`SO_UPLOAD_UNSUPPORTED_BACKEND`、`SO_DISK_FULL`、`SO_PERMISSION_DENIED`、`SO_PREVIEW_UNAVAILABLE` | 文案 key `super-drive.errors.<CODE>` |
| 结果不确定 | `SO_TIMEOUT` 发生在写请求上 | 显示“结果待确认”，不盲目重发新请求；用户点“重试”时复用同一 `operationId`（服务端幂等重放） |

### 1.7 渲染层 store

#### 1.7.1 形状

```ts
type SuperDriveState = {
  engine: EngineSnapshot | null;
  connection: EngineSnapshot["connection"] & { instanceId: string | null; resyncing: boolean };
  drives: { byId: Record<string, Drive>; order: string[]; loaded: boolean; error: SuperDriveError | null };
  route: { section: "files" | "review" | "shares" | "engine" | "settings"; driveId: string | null; path: string;
           detailsFileId: string | null; detailsTab: "overview" | "blocks" | "entities" | "shares"; view: "list" | "grid" };
  folders: Record<FolderKey, FolderPage>;          // FolderKey = `${driveId}\u0000${path}\u0000${filterHash}`
  files: Record<string, FileRecord>;               // FileSummary + { seq: number; optimistic?: OptimisticMark; progress?: Progress }
  details: LruMap<string, FileDetail>;             // ≤ 200
  entities: LruMap<string, Entity>;                // ≤ 500，按 id 与 key 双索引；merged_into 跟随重定向
  review: { groups: Record<string, ReviewGroupState>; pendingByKind: Record<string, number>;
            submitting: Record<string, "accept" | "reject">; asOfSeq: number };
  shares: { byId: Record<string, Share>; order: string[]; chains: Record<string, string[]> };   // chains: 根 id → 刷新链
  usage: Usage | null;
  uploads: UploadQueueState;
  notices: Notice[];                                // 控件内状态无法承载的瞬时通知（例如批量结果）
  selection: { driveId: string | null; ids: Set<string>; anchorId: string | null; allMatching: boolean };
};
type FolderPage = { ids: string[]; dirs: DirSummary[]; nextCursor: string | null; total: number | null;
                    asOfSeq: number; loading: boolean; stale: boolean; error: SuperDriveError | null };
```

#### 1.7.2 实体缓存

- 文件以 `id` 为主键存一份（`files`），目录页只存 id 列表，避免同一文件在多个目录/筛选结果里出现多个副本。
- `details` 与 `entities` 用 LRU（`superDriveStore.ts` 内 40 行的 Map 实现，不引入库）；`file.state_changed` 让对应 `details` 失效；`ontology.version_published` 让全部 `entities` 失效（下次打开重拉）；`review.decided` 让 `item_ids` 涉及的来源文件 `details` 失效。
- 实体按 `key` 访问时，若返回 `redirect_from` 非空或 `status=merged`，把旧 key 指向新 id，详情栏显示“已合并到 …”。

#### 1.7.3 事件游标与归并

- 每个 `FileRecord` 记录最后一次更新它的 `seq`；列表页加载时记录响应的 `as_of_seq`，把页内每个文件的 `seq` 设为 `as_of_seq`。
- `file.state_changed`：`event.seq > record.seq` 才覆盖；文件所在目录已加载但不在 `ids` 中（新文件）→ 按当前排序插入（`sort=path` 时按 `path` 比较；`state_since` 时插到头部）；不在任何已加载目录 → 只更新 Drive 计数。
- `drive.updated`（含 `counts`、`pending_review`）覆盖 Drive 计数；同时以 `file.state_changed` 增量修正计数，两者以 `drive.updated` 为准。
- `file.renamed`：从旧目录移除、加入新目录（若已加载）；`file.removed`：从所有目录移除并清除选择。
- `files.bulk_changed`：对应 Drive 所有已加载目录 `stale=true`，当前可见目录 300 ms 防抖重拉第一页并保持滚动位置。
- `review.changed`：更新 `pendingByKind` 与侧栏徽标；若待确认页打开且 `by_source_changed` 命中已展开的分组 → 重拉该分组。
- `share.created / share.state_changed`：按 `id` upsert；`refreshed_from` 非空时把新 id 接到链上（§3.7）。
- `share.mount_removed`：更新 `share.mount.present`。
- `usage.changed`：更新用量，并在 `limited=true` 时触发横幅。
- `engine.health_changed`：只作提示，权威快照来自 `engine-changed`。

#### 1.7.4 断线与全量重拉

| 触发 | 渲染层动作 |
|---|---|
| `connection-changed(reconnecting)` | 横幅“与引擎的连接已断开，正在重连（第 n 次，m 秒后）”；列表保持可读，写操作按钮仍可用（写请求走 HTTP，不依赖 SSE），状态列右侧显示“可能不是最新” |
| 重连成功（`connected`） | 主进程用 `since` 补齐；渲染层收到补齐事件后横幅消失，显示 3 s 的“已同步 N 条变化”（`role=status`，不抢焦点） |
| `resync-required` / `SO_EVENTS_GAP` / `410` | `connection.resyncing=true`：横幅“正在重新同步…”；按序重拉 `drives-list` → 当前 Drive 的当前目录第一页（保留滚动锚点）→ 待确认计数 → 共享列表；其余已加载目录标 `stale`，下次可见时再拉；完成后以最大的 `as_of_seq` 作为新游标调用 `events-subscribe`；进行中的上传与乐观标记保留 |
| `file.progress` 丢失 | 不补；进度条保留最后值，下一次状态事件覆盖 |
| 窗口重开 | `events-since(lastSeq)`；失败按 `resync-required` 处理 |

#### 1.7.5 乐观更新与回滚

| 操作 | 乐观 | 确认 | 回滚 |
|---|---|---|---|
| 批量重试 | 目标文件 `optimistic={op:"retry", prev}`，徽标显示“排队中…”（虚线描边区分） | 逐项结果 `ok` → 等事件把状态推到 `queued`；`not_retryable/not_found` → 立刻回滚该项 | 回滚到 `prev`，批量结果通知“k 个文件无法重试” |
| 审核决定（逐项、批量、按来源） | 项移入 `submitting`，从待确认计数里扣除，行显示“提交中” | `results[].status=ok` → 从列表移除；`publish_pending=true` 时在页头显示“正在发布新版本”直到 `ontology.version_published` | `conflict`（`DOCS_REVIEW_STALE_REVISION`）→ 重拉该项并恢复；`already_decided` → 移除并提示“已被处理”；`invalid` → 恢复并显示码 |
| 模型开关、日上限、名称（PATCH Drive） | 立即更新 UI | 返回的 `Drive` 覆盖 | `412` → 用 `current_revision` 的最新值覆盖并提示 |
| 上传 | 客户端行立即出现（`uploading`） | 复制完成 → 行进入“等待引擎发现”，被 `file.state_changed(stored)` 接管（按 `drivePath` 匹配） | 复制失败 → 行标失败，可重试 |
| **不乐观** | Drive 登记与删除、Share 挂载、刷新、撤销、同步确认 | 显示进行中状态，等响应 | — |

不乐观的理由：这些操作有宿主机副作用（重建容器、写或删目录、作废凭证），界面先显示成功会误导（同 OO 侧 `specs/frontend/api-state-contracts.md` §5 的约束：不乐观显示审批与发布结果）。审核决定的“乐观”只体现为“提交中”，不显示“已发布”。

### 1.8 objectstore 版 DataPort（`superDriveDataPort.ts`）

`DataPort` 定义在 `packages/shared-ui/src/core/types.ts:296`。Super Drive 只实现浏览与预览需要的子集，**不实现写接口**（D3：OO 只读；上传走专用 IPC）。

| `DataPort` 成员 | 实现 | 说明 |
|---|---|---|
| `listChildren(folderPath)` | `drive-tree { driveId, path }` → `dirs` 映射为 `type: "folder"`、`hasChildren: true`；`files.items` 映射为文件节点 | 文件树只显示目录（`ExplorerTree` 用于左栏）；主区列表不走 `DataPort`，直接用 store |
| `resolveNode(path)` | 先查 store；否则 `files-list { path, limit: 1 }` | — |
| `readFile(path)` | fs Drive：`fetch(file-preview-url)` 读文本（≤ 2 MiB，超出只给 URL）；s3：`SO_PREVIEW_UNAVAILABLE` | 供 `FilePreview` 的文本类查看器 |
| `getFileUrl(path, { purpose })` | `file-preview-url` | 供 PDF、Office、图片查看器 |
| `revokeFileUrl(url)` | 通知主进程作废能力 token | — |
| `openExternalFile`、`revealInFileManager` | `file-reveal`（仅 fs） | — |
| 其余（`createFolder`、`importFiles`、`renameNode`、`deleteNode`、`moveNode`、`copyNode`、`documentPersistence`） | 不提供 | `ExplorerTree` 因此不渲染这些动作 |

`DataNode` 映射：`id = "sd:" + driveId + ":" + path`；`path = path`（Drive 内相对路径）；`type = getFileSemanticKind(name)`（`@puppyone/shared-ui` 导出）；`size` 用 `formatBytes`（`src/features/cloud/utils.ts:40`）格式化；`source = backend === "fs" ? "local" : "connector"`；`status` 不用（`DataNodeStatus` 是 Git 语义）。

处理状态在左栏树上通过 `ExplorerTree` 的 `renderNodeActions(node)` 槽显示一个只读的 `FileStateBadge compact`（只有失败与待确认会显示，避免树太吵）。

`FilePreview`（`packages/shared-ui/src/editor/host/FilePreview.tsx:37`）以只读方式使用：传 `documentPersistence={null}`、`hideSourceView`；它没有“跳到锚点”的接口（检索 `packages/shared-ui/src/editor` 未发现 `revealRange`/`scrollToLine` 一类公开 API），所以证据定位按 §3.4.3 处理，不改 shared-ui。

### 1.9 与 10 §8 的逐项对接

| 10 §8.3 端点 | 前端使用处 | 首次 |
|---|---|---|
| `GET /livez` | 主进程启动探测 | S2 |
| `GET /v1/docs/health` | 引擎页、状态胶囊、横幅、磁盘 | S2 |
| `GET/POST /drives`、`GET/PATCH/DELETE /drives/{id}` | 设置页、首次引导、模型开关、暂停 | S2（fs）/ S4（s3）/ S11（oss、cos、r2） |
| `POST /drives/{id}:sync` | 上传后、工具栏“立即同步”、mass delete 确认横幅 | S2 |
| `GET /drives/{id}/files` | 列表、筛选、搜索、批量全选 | S2 |
| `GET /drives/{id}/tree` | 左栏树、目录行的汇总计数 | S2 |
| `GET /files/{fid}` | 详情栏 | S3 |
| `GET /files/{fid}/blocks` | 解析结果标签页 | S3 |
| `POST /files:retry` | 批量重试 | S3 |
| `GET /entities`、`GET /entities/{eid}` | 实体搜索、实体详情 | S5 |
| `POST /resolve` | 分享面板任务描述的“可能指的是”提示、实体搜索框 | S6 |
| `GET /review`、`POST /review:decide` | 待确认队列 | S5 |
| `POST /context-packs:compile`、`GET /context-packs/{pid}` | 分享面板预览、BRIEF 预览 | S6 |
| `GET /context-packs/{pid}/files/{path}` | 不用（守护进程挂载时自己拉） | — |
| `POST /shares`、`GET /shares`、`GET /shares/{sid}`、`:refresh`、`:revoke` | 分享面板、共享列表 | S6（refresh 的差异预览 S10） |
| `GET /events` | 主进程事件桥 | S2 |
| `GET /usage` | 用量、模型横幅 | S5 |
| `GET /ontology/versions` | 待确认页头“当前版本 vN”、实体详情 | S5 |
| `GET /metrics`、`GET /refs/{b3}` | 不用 | — |

SSE 事件（10 §8.5）对接见 §1.7.3；错误码（10 §8.4）对接见 §4.8；FileState 枚举对接见 §4.1（契约测试保证一一对应）。

### 1.10 开关与验收版身份

#### 1.10.1 两级开关

| 层 | 名称 | 默认 | 谁改 | 作用 |
|---|---|---|---|---|
| 构建 | `src/features/flags/flags.json` 的 `superDrive`（渲染层）+ `package.json` `puppyoneCapabilities.superDrive`（主进程，读取方式同 `resolveGitAutoCommitFeatureProfile`，`electron/main.mjs:236`） | `false` | 主线程；验收构建置 `true` | `false` 时：注册表 `isAvailable` 为假、实验功能里不出现开关、主进程不注册 `super-ontology:*` 处理器、preload 不暴露 `superOntology` |
| 运行时 | 实验功能 `enableSuperDrive`（`src/preferences.ts` 的 `ExperimentalSettings`） | `false` | 用户在“设置 → 实验功能”里打开（60 §4 第 2 步） | 打开后侧栏出现 Super Drive；关闭后页面消失，引擎不自动停（引擎页可停） |

“flag 关闭时现有测试与行为无差异”（30 §4 横切门禁）由 F-S1-06 的回归测试保证：flag 关时 IPC 通道数、preload 暴露键、surface 列表、导航项与基线逐项相等。

#### 1.10.2 验收版：独立应用名与用户数据目录（与正式版共存）

现状（已读代码）：

- 应用身份由构建渠道决定：`shared/desktop/application-identity.mjs:5-33` 定义 `dev`（`PuppyOne Development`、`ai.puppyone.desktop.dev`、`userData=puppyone-development`）、`internal`、`stable`（`PuppyOne`、`ai.puppyone.desktop`、`userData=puppyone`）三种。
- `electron/main/build-info-service.mjs:43-55` 无条件 `app.setName(policy.applicationName)` 与 `app.setPath("userData", <appData>/<userDataName>)`。这意味着 60 §3.2 写的 `--user-data-dir=<dir>` **不会生效**（Chromium 开关先被读取，随后被 `setPath` 覆盖）。
- `npm run dist:mac` 走 `prepare:desktop-build:dev`，产物已经是 `PuppyOne Development`，天然不碰正式版 `PuppyOne` 的数据；但 owner 若日常也跑 dev 构建，会与之共用 `puppyone-development`。

方案（F-S2-10）：

1. 新增构建渠道 `acceptance`：`applicationId: "ai.puppyone.desktop.acceptance"`、`applicationName: "PuppyOne Acceptance"`、`userDataName: "puppyone-acceptance"`、`releaseChannel: null`、`updateChannel: null`（不检查更新）、`published: false`、安全策略同 `dev`（不签名）。打包命令 `node scripts/prepare-desktop-build.mjs --channel acceptance` + `dist:mac:prepared`，产物 `PuppyOne Acceptance.app`、`PuppyOne-super-drive-L<n>.dmg`（60 §3.4）。
2. 用户数据目录覆盖：只在 `acceptance` 与 `dev` 渠道，`configureDesktopApplicationIdentity` 识别 `--puppyone-user-data-dir=<绝对路径>`（或环境变量 `PUPPYONE_USER_DATA_DIR`），用它代替 `<appData>/<userDataName>`；`stable/internal` 渠道忽略该参数（避免正式版被参数劫持数据目录）。T2 启动命令相应改为：
   ```bash
   "$APP/Contents/MacOS/PuppyOne Acceptance" --puppyone-user-data-dir="$RUN/profile" --remote-debugging-port=0
   ```
3. 引擎隔离：状态目录在 `<userData>/super-ontology/state`，compose 项目名由它派生，所以验收版、正式版、每次 T2 运行的引擎互不共享；T2 结束时测试脚本调用 `oo serve --stop --state-dir "$RUN/profile/super-ontology/state"` 并按 `oo.lane` 标签清理容器（60 §3.1）。
4. 构建开关：验收渠道构建时 `flags.json.superDrive=true`、`puppyoneCapabilities.superDrive=true`，实验开关仍默认关（owner 按 60 §4 第 2 步自己打开）。

涉及 `shared/desktop/*` 与 `build-info-service.mjs`，属于发版身份的共享代码，建议主线程实施（§8.2 Q8）。

---

## 2. 设计规范落地

依据：`puppy-issues/document/puppyone-desktop/platform/design-guidelines.md`（按钮、Tooltip、文案与状态克制、稳定内边距）、`renderer-style-architecture.md`、代码中的 token 定义。规则：**颜色只能用 `--po-*` 语义 token；尺寸、圆角、阴影、字号、动效时长只能用 `var(--…)`**；字面量只允许出现在 `super-drive-tokens.css` 一个文件里，并且能映射到现有 token 的必须映射。

### 2.1 现有 token 使用表

| 用途 | token | 定义处 |
|---|---|---|
| 主背景 / 面板 / 浮起面板 / 内凹 / 浮层 | `--po-canvas`、`--po-panel`、`--po-panel-raised`、`--po-inset`、`--po-overlay` | `src/styles/tokens.css`；配色由 `sub-themes/*/theme.css` 生成到 `src/styles/fallback-theme.generated.css` |
| 文本：主 / 次 / 更弱 / 禁用 / 反色 | `--po-text`、`--po-text-muted`、`--po-text-subtle`、`--po-text-disabled`、`--po-text-inverse` | `fallback-theme.generated.css` |
| 边框与分隔 | `--po-border`、`--po-border-subtle`、`--po-border-strong`、`--po-divider` | 同上 |
| 交互态 | `--po-hover`、`--po-active`、`--po-selected`、`--po-control`、`--po-control-hover`、`--po-focus-ring` | 同上 |
| 语义色 | `--po-accent`（主操作）、`--po-success`、`--po-warning`、`--po-danger`、`--po-info`、`--po-purple`、`--po-notification`（计数点） | 同上 |
| 修订模式 | `--po-diff-added-bg`、`--po-diff-added-text`、`--po-diff-removed-bg`、`--po-diff-removed-text` | 同上 |
| 骨架 | `--po-skeleton-base`、`--po-skeleton-shimmer`、`--po-skeleton-edge`；关键帧 `puppy-shimmer` | `fallback-theme.generated.css`；`src/styles/animations.css` |
| 文件类型强调色 | `--po-file-accent-pdf/-word/-sheet/-presentation/-markdown/-default…` | 同上；图标经 `FilePreviewIcon`/`getFileAccent`（`@puppyone/shared-ui`）使用，不直接引用 |
| 控件高度 | `--po-control-size`（32px 中号）、`--po-control-size-compact`、`--po-control-size-small`、`--po-control-size-tight`、`--po-control-size-roomy` | `packages/shared-ui/src/styles/control-geometry.css` |
| 行高（列表） | `--desktop-sidebar-row-height`（= `--po-control-size`）、`--desktop-sidebar-virtual-row-size` | `src/styles/tokens.css` |
| 圆角 | `--desktop-control-radius`（6px）、`--desktop-sidebar-row-radius`（6px）、`--po-menu-radius`（8px）、`--po-menu-item-radius` | `src/styles/tokens.css` |
| 阴影 | `--po-elevation-low`、`--po-elevation-popover`、`--po-menu-shadow`、`--po-menu-shadow-compact` | `src/styles/tokens.css:243-268` |
| 页内边距 | `--desktop-utility-page-padding-top`（24px）、`--desktop-utility-page-padding-inline`（28px）、`--desktop-utility-page-padding-bottom`（40px） | `src/styles/tokens.css:102-104` |
| 字号（UI 角色） | `--po-type-ui-micro`（11）、`--po-type-ui-caption`（12）、`--po-type-ui-meta`（13）、`--po-type-ui-control`/`--po-type-ui-body`（14）、`--po-type-ui-body-large`（15）、`--po-type-ui-section-title`（16）、`--po-type-ui-title`（17）、`--po-type-ui-page-title`（21） | `src/styles/typography/foundations.css:96-110` |
| 字号（侧栏） | `--desktop-sidebar-font-size`、`--desktop-sidebar-font-size-meta`、`--desktop-sidebar-section-title-*` | `src/styles/tokens.css` |
| 字重 | `--po-text-weight-regular`（400）、`--po-text-weight-medium`（500）、`--po-text-weight-semibold`（600） | `src/styles/typography/foundations.css:76-78` |
| 字体 | `--po-font-sans`、`--po-font-mono`（命令行、锚点、路径） | `src/styles/typography/roles.css` |
| 动效 | `--po-pane-motion-duration`（360ms）、`--po-pane-motion-easing`、`--po-scrollbar-transition`（0.16s） | `control-geometry.css`、`tokens.css` |
| 浮层层级 | `--po-overlay-root-z-index` | `src/styles/tokens.css:81` |
| 滚动条 | `data-po-scrollbar="content"` 属性 | `src/styles/scrollbars.css` |

### 2.2 Feature token 层（`super-drive-tokens.css`）

仓库没有通用的间距刻度与短时长动效 token，所以在一个文件里集中声明，能映射的都映射到现有 token；评审通过后可上提到 `src/styles/tokens.css`。

```css
@layer features {
  .super-drive-root {
    /* 间距：能映射的都映射 */
    --sd-space-1: var(--desktop-sidebar-nav-gap);                 /* 4px  */
    --sd-space-2: var(--desktop-sidebar-navigation-padding-balanced); /* 8px */
    --sd-space-3: var(--desktop-sidebar-row-left-gap);            /* 12px */
    --sd-space-4: var(--desktop-sidebar-toolbar-padding-end);     /* 16px */
    --sd-space-5: var(--desktop-utility-page-padding-top);        /* 24px */
    --sd-space-page: var(--desktop-utility-page-padding-inline);  /* 28px */
    /* 尺寸 */
    --sd-row-height: var(--po-control-size);
    --sd-toolbar-height: var(--desktop-sidebar-toolbar-height);
    --sd-details-width: 360px;          /* 详情栏默认宽，可拖 280–560 */
    --sd-details-min-width: 280px;
    --sd-details-max-width: 560px;
    --sd-queue-width: 360px;            /* 队列面板 */
    --sd-queue-max-height: 50vh;
    --sd-grid-tile: 132px;              /* 网格瓦片 */
    --sd-badge-height: 20px;            /* 与 .po-status-badge 一致 */
    --sd-radius: var(--desktop-control-radius);
    --sd-radius-card: var(--po-menu-radius);
    /* 动效 */
    --sd-motion-quick: 160ms;           /* 徽标、行状态、横幅进出；与 --po-scrollbar-transition 同值 */
    --sd-motion-pane: var(--po-pane-motion-duration);
    --sd-motion-easing: var(--po-pane-motion-easing);
    /* 语义映射 */
    --sd-dropzone-border: var(--po-accent);
    --sd-dropzone-fill: color-mix(in srgb, var(--po-accent) 8%, transparent);
    --sd-highlight-fill: color-mix(in srgb, var(--po-accent) 18%, transparent);
    --sd-optimistic-outline: var(--po-border-strong);
  }
}
```

### 2.3 复用的现有组件

| 组件 | 路径 | 在 Super Drive 的用法 |
|---|---|---|
| `Button`、`IconButton` | `packages/shared-ui/src/primitives/Button.tsx`、`IconButton.tsx`（经 `@puppyone/shared-ui` 导出） | 所有按钮；`tone="primary"` 只给每屏的下一步主操作 |
| `StatusBadge` | `packages/shared-ui/src/primitives/StatusBadge.tsx`（tone：neutral/success/warning/danger/info；样式 `shared-ui-patterns.css:80-118`） | `FileStateBadge`、`ShareStateBadge` 的底座 |
| `EmptyState` | `packages/shared-ui/src/primitives/EmptyState.tsx` | 列表内空态（空目录、筛选无结果） |
| `PulseGridLoader`、`DotsLoader`、`InlineLoading` | `packages/shared-ui/src/primitives/LoadingIndicator.tsx` | `parsing/extracting` 徽标、按钮内加载；减少动效时自身降级（`animations.css` 的 `[data-puppy-loader]` 规则） |
| `Tooltip` | `packages/shared-ui/src/primitives/Tooltip.tsx` + `src/components/DesktopTooltipLayer.tsx` | 只包图标按钮与紧凑徽标；有可见文字的按钮不加 Tooltip（design-guidelines “Tooltip Policy”） |
| `ExplorerTree` | `packages/shared-ui/src/data/ExplorerTree.tsx`（`ExplorerTreeProps` 在第 63 行；`loadingPresentation: "skeleton"`；`renderNodeActions`；内建虚拟窗口 `useExplorerVirtualWindow`） | 左栏文件夹树 |
| `FilePreview` | `packages/shared-ui/src/editor/host/FilePreview.tsx` | 详情栏“打开预览”与锚点所在页的只读预览 |
| `FilePreviewIcon`、`getFileIcon`、`getFileAccent` | `packages/shared-ui/src/file/fileIcons`（经 index 导出） | 文件图标 |
| `VirtualSidebarList`、`useVirtualSidebarWindow`、`shouldVirtualizeSidebarList`（阈值 200） | `packages/shared-ui/src/sidebar/*` | 主区列表的虚拟窗口（复用窗口计算 hook，行渲染自己做） |
| `CollapsiblePaneFrame`、`useCollapsiblePaneResize` | `packages/shared-ui/src/sidebar/CollapsiblePaneFrame.tsx`、`primitives/useCollapsiblePaneResize.ts` | 详情栏可拖宽度与折叠 |
| `DesktopDialogRoot`、`DesktopDialogSurface`、`DesktopDialogCloseButton` | `src/components/DesktopDialog.tsx`（焦点陷阱、`dismissalPolicy`、原生表面遮挡租约） | 分享面板、撤销确认、Drive 登记 |
| `DesktopOverlayLayer` | `src/features/app-shell/DesktopOverlayPortal.tsx:71` | 对话框与队列面板的浮层宿主 |
| `DesktopMenuSurface`、`DesktopMenuItem`、`DesktopMenuSection`、`DesktopMenuSeparator` | `src/components/DesktopMenu.tsx` | 右键菜单、“更多”菜单、面包屑折叠菜单 |
| `DesktopEntryState` | `src/components/DesktopEntryState.tsx`（样式 `src/styles/entry-state.css`） | 整页空态：缺 Docker、引擎未启动、首次使用 |
| `ShareTargetMark`、`share.css` 的步骤条/范围/模式/复制字段样式 | `src/features/cloud/share/ShareWizardDialog.tsx`（`ShareStepper` 204 行、`ScopeOption` 422 行、`ModeOption` 448 行、`CopyField` 1085 行）、`share.css` | 分享面板复用这些部件（经 F-S6-01 抽出后引用） |
| `CloudCommandBlock` | `src/features/cloud/components/shared.tsx:335` | 引擎页“手动启动命令”展示 |
| `copyText`、`formatBytes`、`formatRelativeTime`、`shellQuote` | `src/features/cloud/utils.ts:150,40,260,146` | 复制、字节数、相对时间、命令转义（经 `cloud` 公开入口或复制到 `super-drive` 内的工具函数，避免深引用 `cloud/utils`；见 F-S2-05） |
| `useLocalization`、`bidiIsolate` | `@puppyone/localization`、`@puppyone/localization/react` | 文案、数字与日期格式化、文件名双向隔离 |
| `DesktopCloudCreateScopeRequest` / `DesktopCloudScope` | `src/lib/cloudApi.ts:375-410` | 分享范围的数据形状（`name/path/exclude/max_mode`）；Cloud 模式（L7）下直接调用 `createCloudScope`（§3.6.4） |
| `withTestLocalization` | `tests/support/react/localization.ts` | 组件测试 |

### 2.4 新组件清单

每个组件都要满足 §2.6 的 A1–A5。

| 组件 | Props | 状态 / 变体 | 无障碍 |
|---|---|---|---|
| `FileStateBadge` | `state: FileState \| "uploading" \| "unknown"`、`code?: string`、`progress?: {unit,done,total}`、`blockedReason?`、`pendingReview?: number`、`optimistic?: boolean`、`variant: "full" \| "compact"` | full：图标 + 文字（+ 进度“第 3/12 页”）；compact：只图标，文字放 `aria-label` 并包 `Tooltip`；optimistic：虚线描边 `--sd-optimistic-outline`；unknown：中性色 + “未知状态（code）” | 文字始终存在（可见或 `aria-label`）；颜色不是唯一信息；进行中的进度在详情栏用 `role="progressbar"` 带 `aria-valuenow/max` |
| `PendingReviewCount` | `count`、`onOpen` | 0 时不渲染；>99 显示 99+ | 按钮，名称“{n} 条待确认”；数字变化经 `role="status"` 播报（合并 1 s） |
| `DriveFileTable` | `driveId`、`folderKey`、`columns`、`sort`、`onSort`、`selection`、`onOpen`、`onContextMenu` | 加载中（骨架行）、空、筛选无结果、错误、stale（顶部细条“正在刷新”） | `role="grid"`、`aria-rowcount=total+1`、`aria-multiselectable`；行 `role="row"` 带 `aria-rowindex`、`aria-selected`；表头可排序列 `aria-sort` |
| `DriveFileGrid` | 同上（无列） | 同上 | `role="grid"`，按行列计算 `aria-rowindex/aria-colindex` |
| `DriveBreadcrumb` | `driveName`、`path`、`onNavigate` | 超过 4 段时中间折叠为“…”菜单 | `<nav aria-label>` + `<ol>`；当前段 `aria-current="page"` |
| `DriveDropZone` | `enabled`、`targetLabel`、`onFiles`、`reasonDisabled?` | 空闲（不可见）、悬停（虚线框 + “松开以上传到 {folder}”）、禁用（s3 Drive，显示原因） | 区域 `role="region"` + 可访问名称“上传到 {folder}”；同时提供工具栏“上传”按钮作为键盘等价入口 |
| `DriveSelectionBar` | `count`、`allMatching`、`actions` | 选中 ≥ 1 时出现；全选跨页时显示“已选择全部 {n} 个匹配项” | `role="toolbar"`，名称“已选择 {n} 项” |
| `DriveBanner` | `id`、`tone: "info" \| "warning" \| "danger"`、`icon`、`title`、`description?`、`actions?`、`dismissible?` | 进入/离开动效 `--sd-motion-quick`；最多 2 条 | `role="status"`（危险类也用 status，避免打断；需要立即注意的引擎崩溃用 `role="alert"` 一次） |
| `ProcessingQueuePanel` | 无（读 store） | 折叠（一行汇总）、展开（上传段 + 处理段）、隐藏（无活动且用户关闭） | `role="region"` 名称“上传与处理队列”；汇总行 `aria-live="polite"`；折叠按钮 `aria-expanded` |
| `FileDetailsPane` | `fileId`、`tab`、`onTab`、`onClose` | 加载、错误、文件已删除（显示最后状态与“已从 Drive 移除”） | `role="complementary"` 名称“文件详情：{name}”；标签页 `role="tablist/tab/tabpanel"`，方向键切换 |
| `EvidenceSnippet` | `evidence: Evidence`、`highlight?: [start,end]`、`onLocate` | 正常、已撤回（`withdrawn_at` 非空：删除线 + “证据已撤回”）、来源文件已不在（禁用定位） | 引文用 `<blockquote>`；高亮用 `<mark>`；“在预览中定位”是按钮，名称含锚点（“定位到 第 3 页”） |
| `EntityChip` | `entity: EntityRef`、`onOpen` | 核心类型图标；`Thing` 显示 `label` | 按钮名称“{类型}：{名称}” |
| `EntityDetailView` | `entityIdOrKey` | 加载、已合并（重定向提示）、已撤回 | 标题层级 h2/h3；事实表 `<table>` 带 `<caption>` |
| `ReviewGroup` | `group`、`items`、`expanded`、`selection`、`onDecideGroup`、`onDecideItems` | 折叠、展开、提交中、部分冲突 | 分组标题是按钮（`aria-expanded`）；“整体接受”按钮名称包含来源与数量 |
| `ReviewItemRow` | `item`、`selected`、`onDecide`、`onToggleExpand` | 待定、提交中、冲突（需刷新）、已处理（短暂停留后移除） | `role="row"`；接受/拒绝是行内按钮，无弹窗 |
| `ReviewBatchBar` | `selectedCount`、`onAccept`、`onReject` | — | `role="toolbar"` |
| `RevisionView`（M2） | `suggestion`、`diff`、`onDecide` | 删除线（`--po-diff-removed-*`）、插入（`--po-diff-added-*`） | 增删除了颜色还有前缀符号与 `aria-label`“删除：…/新增：…” |
| `ShareStateBadge` | `share: Share` | 五种状态（§4.2） | 同 `FileStateBadge` |
| `ShareChain` | `chain: Share[]` | 展开显示刷新历史 | 列表 `<ol>` |
| `MountCommandField` | `argv`、`platform` | 正常、禁用（未选目标目录时只显示占位） | 复制按钮名称“复制命令” |
| `EngineStatusPill` | `snapshot` | §4.4 全部状态 | 按钮名称“引擎：{状态}”；状态变化 `role="status"` |
| `PreflightChecklist` | `snapshot`、`onRetry`、`onStart` | 每项：通过、失败、检查中 | `<ul>`，每项含状态文字 |
| `ComponentList` | `health` | db/parser/model/cas/engine 每项 7 种 `ComponentStatus` | `<dl>` |
| `StorageMeter` | `used`、`free`、`cacheLimit` | 正常、低（`DOCS_ENGINE_DISK_LOW`）、满（`DISK_FULL`） | `role="meter"` 带 `aria-valuenow/min/max` 与文字 |
| `SuperDriveEntryState` | `variant` | `first-run`、`flag-only`、`engine-*`（§4.4） | 继承 `DesktopEntryState` 的 `aria-label`；主操作按钮可见文字 |
| `DriveTableSkeletonRow` | `index` | 静态（减少动效时不闪烁） | `aria-hidden="true"`；列表 `aria-busy="true"` |

### 2.5 动效

| 场景 | 时长 / 曲线 | 说明 |
|---|---|---|
| 详情栏开合、队列面板展开 | `--sd-motion-pane` / `--sd-motion-easing` | 与现有 pane 动效一致；只动 `transform` 与 `opacity` |
| 徽标状态切换、横幅进出、行进入列表 | `--sd-motion-quick` | `opacity` + 4px 位移，复用 `animations.css` 的 `fadeIn` 关键帧 |
| `parsing/extracting` | `PulseGridLoader size="xs" tone="info"` | 不自造旋转动画 |
| 骨架 | `puppy-shimmer` | — |
| 拖拽悬停 | 无动画，只换描边与底色 | 避免拖拽过程中掉帧 |
| 大量状态变化（1 万文件冷启动） | 不对行做进入动画；超过 50 条/秒时关闭行级过渡 | 60fps 优先 |

减少动效：Super Drive 渲染在 `.app-shell` 内，`src/styles/animations.css` 末尾的 `@media (prefers-reduced-motion: reduce)` 已把过渡与动画压到 0.01ms，`[data-puppy-loader]` 停止动画；新组件不得用 `!important` 或内联动画覆盖它。窗口在后台时（`document.hidden`）暂停进度条动画与骨架闪烁。

### 2.6 无障碍与可测试性（60 §6，从 F-S2 起每张卡的验收项）

| 编号 | 要求 | 如何验证 |
|---|---|---|
| **A1** 稳定可访问名称 | 每个可交互元素都有可访问名称（可见文字或 `aria-label`），名称来自 i18n，不拼接未翻译的片段；同一元素在不同状态下名称稳定（例如“全部重试”不因计数变化而改名，计数放 `aria-describedby`）；8 种语言下都非空且不同于 key | 组件测试遍历渲染树：`button, a, input, [role=button|tab|menuitem|row|checkbox|switch|option]` 均有非空名称；伪语言 + 8 语言快照检查 |
| **A2** 状态播报 | 状态徽标的变化、横幅、队列面板汇总数字、断线重连、批量结果用 `role="status"` 或 `aria-live="polite"`；引擎崩溃一次性 `role="alert"` | 组件测试断言属性；T2 用 Peekaboo 读辅助功能树 |
| **A3** 可被辅助功能树找到 | 拖拽区有可访问名称；右键菜单用 `DesktopMenuSurface role="menu"` + `DesktopMenuItem role="menuitem"`；每个有右键菜单的行同时有“更多操作”按钮（Shift+F10 / 菜单键也能打开） | 组件测试；T2 场景按名称点菜单项 |
| **A4** 测试标识 | `data-testid` 只用于断言（T1 与 T2 的 CDP 断言），命名 `sd-<区域>-<元素>`，行级附 `data-file-id`、`data-file-state`、`data-share-id`、`data-share-state`、`data-review-id`；T2 的**操作**只用可访问名称 | 架构测试：`tests/e2e/super-drive/packaged/` 下的操作脚本不得出现 `data-testid` 选择器 |
| **A5** 覆盖率门禁 | Super Drive 所有可交互元素可访问名称覆盖率 100%（60 §7） | 架构测试 `tests/architecture/super-drive/accessibility/accessibleNames.test.tsx` 渲染每个页面的全部 fixture 状态后统计 |
| 键盘 | 所有功能可用键盘完成（§3.2.4、§3.5.3）；焦点可见用 `--po-focus-ring` | 组件测试 + T1 |
| 对比度 | 只用语义 token；`--po-text-subtle` 不用于承载关键信息 | 视觉检查 |
| 双向文本 | 文件名、路径、实体名经 `bidiIsolate` 或 `<bdi>`，容器 `dir="auto"`（同 `ShareWizardDialog.tsx:123` 的做法） | 组件测试 |

### 2.7 文案原则

遵循 design-guidelines “Copy and action status”：进度、成功、可恢复失败放在原控件里，不另起一句话重复；保留会改变决定的说明（成本与上限、可见范围、删除后果、命令原文、格式限制）。Super Drive 特有的四条：

1. 失败一律给“发生了什么 + 能做什么”，不显示原始 `message`；错误码放在“详细信息”里（可复制）。
2. 不承诺未实现的能力（例如 M1 的“可编辑”角色显示为禁用，说明“M2 开放”，不写“即将推出”之类的营销语）。
3. 撤销 Share 的确认框必须出现 `super-drive.share.revoke.irrevocable`（“Agent 已读内容无法收回”），并与引擎返回的 `notice_code=DOCS_SHARE_READ_CONTENT_IRREVOCABLE` 对应。
4. 计数用 ICU 复数，数字用 `formatNumber`；时间用相对时间 + 悬停不出 Tooltip（与 Git 侧栏规则一致），完整时间放详情栏。

---

## 3. 页面与交互规格

线框里 `[x]` 表示按钮，`( )` 表示单选，`[ ]` 表示复选框，`v` 表示下拉，`>` 表示折叠项，`*` 表示状态图标。线框表达结构与优先级，不表达像素。

### 3.1 Super Drive 主框架

Super Drive 是一个 workspace surface：左栏投影到 Explorer 槽，主区投影到 main 槽（`DesktopDataWorkspaceSurface.tsx:363-405`）。数据工作区保持挂载（`check-sidebar-architecture.mjs` 要求），切回“数据”时状态不丢。

```
┌──────────────────────────────┬────────────────────────────────────────────────────────────────────────────┐
│ 左栏（sidebar 槽）            │ 主区（main 槽）                                                            │
│ ┌──────────────────────────┐ │ ┌ 工具栏 ────────────────────────────────────────────────────────────────┐ │
│ │ 我的资料          v      │ │ │ 我的资料 > Projects > 项目A      [搜索…]  [列表|网格]  [上传] [共享]   │ │
│ │ 本地文件夹 · 1,284 个文件│ │ │                                                    * 引擎：健康        │ │
│ └──────────────────────────┘ │ └────────────────────────────────────────────────────────────────────────┘ │
│  文件                         │ ┌ 横幅区（0–2 条）──────────────────────────────────────────────────────┐ │
│  待确认               (12)    │ │ ! 3 个文件处理失败                    [只看失败] [全部重试]        [x] │ │
│  共享                  (2)    │ └────────────────────────────────────────────────────────────────────────┘ │
│  引擎                    *    │ ┌ 内容区 ────────────────────────────────────┐┌ 详情栏（可拖、可关）────┐ │
│  设置                         │ │ （按分区切换：文件 / 待确认 / 共享 / 引擎 / ││ 会议纪要-1103.docx      │ │
│ ─────────────────────────────│ │   设置）                                    ││ [概览][解析][实体][共享]│ │
│  文件夹                        │ │                                            ││                          │ │
│  > Projects                   │ │                                            ││                          │ │
│    > 项目A                ! 2 │ │                                            ││                          │ │
│  > 纪要                       │ │                                            │└──────────────────────────┘ │
│  > People                     │ │                                            │ ┌ 队列面板（右下浮层）────┐ │
│                               │ └────────────────────────────────────────────┘ │ 上传 2 · 处理中 14 · ! 3 ^│ │
└──────────────────────────────┴───────────────────────────────────────────────── └─────────────────────────┘ ┘
```

| 元素 | 说明 |
|---|---|
| Drive 切换器 | 下拉列出已登记 Drive（名称、后端、文件数、待确认数）；底部“登记新 Drive…”跳到设置页的登记对话框。只有一个 Drive 时仍显示（不隐藏），保证位置稳定 |
| 分区导航 | 文件 / 待确认 / 共享 / 引擎 / 设置；待确认显示 `pending_review` 总数（`PendingReviewCount`）；共享显示 `drifted` 数量（若 > 0，`--po-warning`）；引擎显示状态点（§4.4 tone） |
| 文件夹树 | `ExplorerTree`，数据来自 `superDriveDataPort.listChildren`；只列目录；目录右侧只在有失败或待确认时显示紧凑徽标（来自 `tree` 的 `counts`） |
| 工具栏 | 面包屑；搜索（名称子串，`q=`，300 ms 防抖）；视图切换；上传（s3 Drive 禁用并说明原因）；共享（以当前目录为初始范围打开分享面板）；`EngineStatusPill`（点开引擎页） |
| 横幅区 | `DriveBannerStack`，规则见 §4.7 |
| 详情栏 | 选中单个文件且用户打开时显示；宽度 `--sd-details-width`，可拖 280–560；关闭后记住状态 |
| 队列面板 | 右下角浮层，见 §3.3 |

窗口最小宽度 640px（`--desktop-window-min-width`）：主区宽度 < 720px 时详情栏改为覆盖式（盖住列表右半，带关闭按钮），网格列数自动减少；工具栏搜索框折叠为图标按钮。

### 3.2 文件列表 / 网格

#### 3.2.1 列表视图

```
┌ 内容区 · 列表 ──────────────────────────────────────────────────────────────────────────────┐
│ [ ]  名称 ^                         处理状态              待确认   大小      进入状态        │
│ ─────────────────────────────────────────────────────────────────────────────────────────── │
│ [ ]  [D] 纪要                        12 个文件 · ! 1       3        —         —              │
│ [x]  [P] 方案二-评估.pdf             * 已发布               2        2.1 MB    3 分钟前       │
│ [ ]  [W] 会议纪要-1103.docx          * 解析中 · 第 3/12 页  —        480 KB    刚刚           │
│ [ ]  [S] 人员表.xlsx                 * 排队中               —        96 KB     1 分钟前       │
│ [ ]  [P] 合同扫描件.pdf              * 暂不支持 · 扫描件    —        8.4 MB    5 分钟前       │
│ [ ]  [P] 损坏的方案.pdf              ! 解析失败 · 文件损坏  —        1.2 MB    2 分钟前  [...]│
│ ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ │ ← 骨架行
│                                               显示 1–200 / 共 1,284                            │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

| 列 | 来源 | 排序 |
|---|---|---|
| 名称（图标 + 文件名 + 扩展名弱化） | `FileSummary.name`、`format` | `sort=path`（默认） |
| 处理状态 | `FileStateBadge`（state、`state_code`、`progress`、`blocked_reason`） | `sort=state_since` 时按进入时间；按状态分组请用筛选 |
| 待确认 | `pending_review` | 不可排序（服务端无该排序键） |
| 大小 | `size` | 不可排序（同上） |
| 进入状态 | `state_since`（相对时间） | 同“处理状态” |

目录行来自 `tree.dirs`：显示文件数与失败数，双击或 Enter 进入。**“修改时间”列不提供**：`FileSummary` 没有源文件的修改时间字段（10 §8.2），见 §8.3 依赖 G1。

筛选（工具栏“筛选”菜单，或点击横幅/状态计数进入）：状态多选（按 §4.1 的分组：处理中 / 已发布 / 失败 / 跳过 / 历史），映射为 `state=` 逗号列表；“包含子文件夹”映射 `recursive=true`。筛选条件显示为工具栏下方的筛选标签，可逐个移除。

#### 3.2.2 网格视图

```
┌ 内容区 · 网格 ───────────────────────────────────────────────────────────┐
│ ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐        │
│ │   [D]    │ │   [P]    │ │   [W]    │ │   [S]    │ │   [P]    │        │
│ │  纪要    │ │ 方案二…  │ │ 会议纪要…│ │ 人员表…  │ │ 损坏的…  │        │
│ │ ! 1      │ │ * 已发布 │ │ * 第3/12 │ │ * 排队中 │ │ ! 失败   │        │
│ └──────────┘ └──────────┘ └──────────┘ └──────────┘ └──────────┘        │
└──────────────────────────────────────────────────────────────────────────┘
```

瓦片宽 `--sd-grid-tile`；状态用 `FileStateBadge variant="compact"`（可访问名称完整）；选择、键盘、右键与列表共用一份模型。视图选择按 Drive 记忆。

#### 3.2.3 选择与批量操作

- 单击选中并（若详情栏开着）切换详情；Cmd/Ctrl+单击追加；Shift+单击区间；复选框列等同 Cmd 单击。
- Cmd/Ctrl+A：先选中已加载的全部行；若 `next_cursor` 非空，选择条显示“选择全部 {total} 个匹配项”链接，点后 `allMatching=true`（批量操作时由主进程用相同的筛选条件分页取 id，再切批调用）。
- 选择条动作：重试（只对 `retryable` 的项生效，数量写在按钮旁）、共享所选（以所选路径为范围打开分享面板）、在访达中显示（单选，fs）、打开详情（单选）、取消选择。
- 范围变化（切目录、改筛选）清空选择。

#### 3.2.4 键盘

| 键 | 行为 |
|---|---|
| ↑ / ↓ | 移动焦点行；Shift 扩展选择 |
| ← / → | 网格中左右移动；列表中无行为 |
| Home / End、PageUp / PageDown | 跳到首行/末行（末行会触发加载剩余页并显示骨架）、翻页 |
| Enter | 目录：进入；文件：打开详情栏 |
| Space | 切换当前行选择 |
| Cmd/Ctrl+A | 全选（见上） |
| Esc | 依次：关闭菜单 → 清空选择 → 关闭详情栏 |
| Backspace 或 Cmd/Ctrl+↑ | 上一级目录 |
| Cmd/Ctrl+I | 开关详情栏 |
| Shift+F10 或菜单键 | 打开当前行的上下文菜单 |
| Cmd/Ctrl+F | 聚焦搜索框 |
| R（选择条可见时） | 重试所选（带确认数量的按钮获得焦点，不直接执行） |

列表采用 roving tabindex：整个 grid 只有一个 Tab 停靠点。

#### 3.2.5 右键菜单

`DesktopMenuSurface`，菜单项随对象与状态变化（不可用的项不显示，而不是灰显，除非需要解释原因）：

| 对象 | 菜单项 |
|---|---|
| 文件 | 打开详情 · 打开预览 · 在访达中显示（fs） · 复制路径 · ── · 重试（失败或部分抽取时） · 安全模式重试（`mode=safe`，`DOCS_PARSE_OOM/CRASH` 时） · 查看待确认（`pending_review>0`） · ── · 共享此文件 |
| 目录 | 打开 · 共享此文件夹 · 只看此文件夹中的失败 · 立即同步此 Drive |
| 多选 | 重试所选 · 共享所选 · 取消选择 |
| 空白处 | 上传文件… · 上传文件夹… · 立即同步 · 视图 > 列表/网格 |

#### 3.2.6 拖拽上传

- 只在 fs Drive 可用（s3 Drive 的上传见 §8.2 Q6）。拖入主区任意位置 → 全区 `DriveDropZone` 出现：“松开以上传到 {当前文件夹}”；拖到左栏树的某个目录或列表中的目录行 → 目标改为该目录，行高亮。
- 松开：preload `upload()` 立刻用 `webUtils.getPathForFile` 取路径（不能读取路径的项，例如来自浏览器的虚拟文件，列为“无法上传”并说明原因）。
- 上传前预检（渲染层，不阻塞）：单文件 > 200 MiB（md/txt > 64 MiB，D31/10 §4.11）→ 在队列面板里该项旁注“超过处理上限，会被存储但不会被读懂”，仍上传；Drive 文件数将超过 1 万 → 选择条上方提示“超过 1 万个文件的部分不会被处理”。
- 文件夹拖入：保留相对目录结构；隐藏文件与 `*.part`、`*.tmp` 等（10 §4.3.2 的忽略规则）不复制，并在队列面板汇总“已跳过 k 个隐藏或临时文件”。
- 重名：自动改名“名称 (2).ext”，在队列条目上显示新名字。

### 3.3 队列面板（右下角）

```
┌ 上传与处理 ───────────────────────────── [—] [x] ┐   ← 折叠：一行
│ 上传 2 · 处理中 14 · 失败 3                      │
├──────────────────────────────────────────────────┤
│ 上传                                    [全部取消]│
│  会议纪要-1103.docx     ████████░░ 82%      [x]   │
│  人员表.xlsx            等待中               [x]   │
│  报价单.pdf             ! 磁盘空间不足       [重试]│
├──────────────────────────────────────────────────┤
│ 处理                                              │
│  排队 9 · 解析 3 · 抽取 2 · 等待中 1（解析服务不可用）│
│  失败 3                              [查看] [全部重试]│
│  已发布 1,262 · 待确认 12                 [去确认] │
└──────────────────────────────────────────────────┘
```

| 部分 | 数据 | 行为 |
|---|---|---|
| 汇总行 | 上传进行中数量 + Drive `counts`（排队+解析+抽取）+ 失败数（`parse_failed`+`extract_partial`） | `aria-live="polite"`，合并 1 s 播报；无任何活动且最近 10 s 没有变化时自动折叠成汇总行；用户点 [x] 后隐藏，直到出现新的上传或失败 |
| 上传段 | `uploads`（客户端状态） | 进度、取消、失败重试；完成项保留 5 s 后转入处理段计数；按 `drivePath` 与 `file.state_changed` 对接（交接期间显示“等待引擎发现”，超过 `fs_poll_seconds`=30 s 仍未发现时显示“立即同步”按钮） |
| 处理段 | Drive `counts`、`health.queue`（`blocked` 数量）、`blocked_reason` 聚合 | “查看”跳到筛选后的列表；“全部重试”= 对当前 Drive 的全部可重试失败文件调用 `files-retry`（主进程分页取 id，切批）；“去确认”跳到待确认页 |

面板宽 `--sd-queue-width`，最大高 `--sd-queue-max-height`，`--po-panel-raised` 底，`--po-elevation-popover` 阴影，`--sd-radius-card` 圆角；位置固定在主区右下，避开详情栏（详情栏打开时左移到详情栏左边缘）。

### 3.4 详情栏

#### 3.4.1 概览

```
┌ 会议纪要-1103.docx                          [x] ┐
│ [概览] [解析结果] [实体] [共享]                  │
├─────────────────────────────────────────────────┤
│ * 已发布 · 2 条待确认            [去确认]        │
│ 路径   纪要/会议纪要-1103.docx                   │
│ 大小   480 KB      格式 docx                     │
│ 内容   b3:7e1c…（可复制）                        │
│ 解析   docling 2.x · 43 块                       │
│ 抽取   规则 + 模型（12 次调用，命中缓存 5）      │
│ ─ 处理记录 ───────────────────────────────────── │
│  11:02  已存入                                   │
│  11:02  排队中                                   │
│  11:03  解析中（第 1 次）                        │
│  11:03  解析失败 · 解析超时 · 第 1 次，已自动重排 │
│  11:04  解析中（第 2 次）→ 已解析 → 抽取中        │
│  11:05  已建索引 → 已发布（v17）                 │
│ ─ 版本 ───────────────────────────────────────── │
│  当前   11:02 存入                               │
│  旧版   11-01 存入，11-02 被取代                 │
│ [打开预览] [在访达中显示] [重试]                 │
└─────────────────────────────────────────────────┘
```

数据：`file-detail { include: ["history"] }`；`history`（≤200 条 `StateEntry`）、`versions`、`parse`、`llm`。失败时顶部换成“失败原因卡”：标题用 `super-drive.errors.<state_code>`，正文给建议（§4.8），按钮“重试”/“安全模式重试”，“详细信息”折叠区显示 `state_code`、`attempt`、`details`（可复制）。

#### 3.4.2 解析结果（S3）

```
┌ [概览] [解析结果] [实体] [共享] ─────────────────┐
│ 43 个块 · 12 页        [页 v 全部]  [打开预览]   │
├─────────────────────────────────────────────────┤
│ #1  标题   第 1 页  ¶ 1.1                       │
│     2026 年 11 月 3 日 项目A 周会                │
│ #2  段落   第 1 页  行 3–7          [定位]       │
│     与会：王小明、李华……                         │
│ #3  表格   第 2 页  Sheet!A1:C9      [定位]       │
│     ┌────┬────┬────┐（前 5 行）                  │
│ …（虚拟列表，滚到底加载下一页）                  │
└─────────────────────────────────────────────────┘
```

数据：`file-blocks { fileId, page?, limit: 100, cursor }`；块的 `src` 字段是出处标签（10 §3.5）；块类型、锚点从 `oo.doc-ir/v1` 的 block 读。`409 DOCS_PARSE_NOT_READY` 时显示“解析完成后可查看”并在 `file.state_changed` 到 `parsed` 之后自动加载。

#### 3.4.3 证据定位预览

`FilePreview` 没有跳到锚点的接口，且不改 shared-ui，所以定位分两级：

1. **片段内定位（所有格式，M1 必做）**：`EvidenceSnippet` 展示 `quote`（≤ 600 字）并高亮命中区间（`--sd-highlight-fill`），上方显示锚点（`第 3 页`、`Sheet!A1:C9`、`标题路径 › 行 12–18`）；这是 P10 与 L4 验收 3“点开并定位到原文”的最低保证。
2. **原文内定位（按格式渐进）**：
   - PDF：`getFileUrl` 返回的 `puppyone-drive:` URL 追加 `#page=<n>`；`local-file-protocol.mjs:25-28` 的注释说明 Chromium 自定义协议会把片段当作查看器状态，PDF 查看器据此翻页（**未确认**：`PdfViewer` 是否把 URL 片段传给内嵌查看器，F-S3-04 需实测；不行则退回第 1 级）。
   - Markdown / TXT / CSV：在详情栏内用只读文本视图渲染块所在的前后 20 行并高亮行区间（自绘，不进 `FilePreview`）。
   - DOCX / PPTX / XLSX：M1 只做第 1 级 + “打开预览”到该文件；精确定位等 shared-ui 提供公开的 reveal 接口后另开卡（§8.1 R6）。

#### 3.4.4 实体（S5）

列出 `FileDetail.entities`（`EntityChip`，按类型分组：人、组织、项目、决定、文档、主题、事件、术语、其他）与 `facts`（主语 – 谓语 – 宾语/值、有效期、置信度、来源 rule/llm/human/suggest、状态 draft/published/retracted/rejected）。点实体打开 `EntityDetailView`（在详情栏内推入一层，带返回）：

```
┌ < 返回   王小明（人）                          ┐
│ 别名  小王 · Xiaoming Wang · 王工               │
│ ─ 当前事实 ──────────────────────────────────── │
│  属于  项目A           2026-09 起   规则  [证据]│
│  负责  方案二评估      2026-10 起   模型 0.86 … │
│ ─ 已被取代 ──────────────────────────────────── │
│  负责  方案一          2026-08–2026-10（被“方案二”取代）│
│ ─ 出现在 ────────────────────────────────────── │
│  会议纪要-1103.docx · 方案二-评估.pdf · …       │
└────────────────────────────────────────────────┘
```

双时间呈现：有效期（`valid_from`–`valid_to`，按 `valid_precision` 显示到日/月/年）在正文；系统时间（`recorded_at`、`retracted_at`）放在每条事实的“详细信息”里。默认不显示已撤回事实，“显示已撤回”开关打开后以删除线显示（D17 默认过滤失效事实）。

#### 3.4.5 共享（S6）

`FileDetail.shares`：每个引用此文件的 Share（状态徽标、用到的事实数 `facts_used`、任务描述、到期时间），点击跳到共享列表并定位该 Share。为空时：“还没有 Share 用到这个文件”。

### 3.5 待确认队列

#### 3.5.1 页面

```
┌ 待确认 · 12 ────────────────────────────── 当前版本 v17 ┐
│ 分组 (o)按来源文件 ( )按种类      种类 [全部 v]           │
├──────────────────────────────────────────────────────────┤
│ v 会议纪要-1103.docx · 5 条              [整体接受] [整体拒绝]│
│   [ ] ? 低置信事实  王小明 负责 方案二评估   0.62  [接受][拒绝] >│
│         “……由王小明负责方案二的评估……”  第 2 页  [定位]   │
│   [ ] = 实体合并    “李华” 与 “Li Hua”       0.71  [合并][保持分开] │
│   [ ] ~ 别名冲突    “小王” 指向 2 个人             [选择归属 v]   │
│ > 方案二-评估.pdf · 4 条                 [整体接受] [整体拒绝]│
│ > 人员表.xlsx · 3 条                     [整体接受] [整体拒绝]│
├──────────────────────────────────────────────────────────┤
│ 已选 2 项                          [接受所选] [拒绝所选]  │
└──────────────────────────────────────────────────────────┘
```

| 元素 | 数据与行为 |
|---|---|
| 分组：按来源文件（默认） | `review-list { groupBy: "source", driveId }` 得到 `ReviewGroup{source,kind,count}`，按来源聚合；展开时 `review-list { source: fileId, status: "pending" }` 拉明细 |
| 分组：按种类 | `review-list { kind }` |
| 整体接受 / 拒绝（按来源） | `review-decide { bySource: { fileId, kinds?, decision, asOfSeq } }`；`asOfSeq` 取该分组加载时的 `as_of_seq`，只处理那之前入队的项（10 §8.2 `by_source.as_of_seq`）；**一次操作**（30 L4 验收 6：100 条 ≤ 1 次操作） |
| 行内决定 | `review-decide { items: [{ id, revision, decision }] }`；不弹窗（D24）；`edit` 决定在行内展开编辑区（值、有效期、合并目标、别名归属），提交即 `decision: "edit"` |
| 批量 | 选中后底部条“接受所选/拒绝所选”；主进程切批 ≤500 |
| 结果 | 逐项结果合并显示为一条 `role=status` 通知：“已接受 98 条，2 条已被处理”；`publish_pending=true` 时页头显示“正在发布新版本”，`ontology.version_published` 后更新为 `v18` |
| 证据 | 每项最多 20 条 `evidence`，默认显示第一条的 `EvidenceSnippet`，展开看全部 |

种类与可用决定见 §4.3。M1 不提供撤销（后端没有撤销接口，§8.2 Q7）；为降低误操作，“整体接受/拒绝”按钮显示数量（“整体接受 5 条”），键盘触发时需要第二次确认键（见下）。

#### 3.5.2 空态与边界

- 待确认为 0：`EmptyState`“没有需要确认的内容”，副文“低置信的内容会出现在这里；高置信与确定性的内容已自动发布”。
- 未配置模型时：低置信事实很少，空态副文改为“当前只用确定性规则，配置模型后会识别更多内容”并给“去配置”。
- 审核疲劳监测（00 §8 风险）：页面记录每周确认率（接受/（接受+拒绝））到本地遥测开关允许的范围内（**未确认**是否接入 `src/features/telemetry`；默认不上报，只在 L4 签收证据里由测试脚本读取）。

#### 3.5.3 键盘

| 键 | 行为 |
|---|---|
| J / K | 下一项 / 上一项（跨分组） |
| X | 选中/取消当前项 |
| A / D | 接受 / 拒绝当前项（`decision=accept/reject`） |
| E | 展开当前项的编辑区 |
| Shift+A | 聚焦当前分组的“整体接受”按钮（再按 Enter 才执行） |
| O | 展开/折叠当前分组 |
| L | 定位到当前项第一条证据 |

#### 3.5.4 M2：Suggest 审核与修订模式（S7、S8）

- 新种类 `suggestion`（S7）与 `reprocess_diff`（S10，D23 重跑差异）加入种类筛选；`suggestion` 默认按“当日摘要”分组（影响式规则自动接受的项也列出，可事后撤销——撤销接口由后端 11.1 提供，前端只在接口存在时显示）。
- 修订模式（S8，`RevisionView`）：

```
┌ 建议 · 来自 Share「跟进方案二」· Agent 写于 10:42 ───────────────┐
│ 影响：1 个正在进行的 Share 会标为“需刷新”     [查看受影响的任务] │
│ Projects/项目A/决定.md                                             │
│   ## 当前方案                                                      │
│ - 采用方案一（2026-08）                    ← 删除线 --po-diff-removed-* │
│ + 采用方案二（2026-11），方案一因成本放弃  ← 高亮 --po-diff-added-*   │
│   理由：……（rationale）  证据：BRIEF 第 12 行 [定位]                │
│                         [接受此建议] [拒绝] [按来源全部接受]        │
└───────────────────────────────────────────────────────────────────┘
```

数据来自 `GET /v1/docs/suggestions/{id}/diff`（10 §11.1）；`editor` 角色接受后的文件写入由 Desktop 执行（主进程写 Drive 文件，再触发 `:sync`；若该 Drive 也是 Git 工作区，交给现有 Git 自动提交流程，见 F-S8-03）。

### 3.6 分享面板

#### 3.6.1 流程

三步，复用 ShareWizard 的步骤条与范围选择部件（F-S6-01 抽出后引用）：

```
① 范围 ──▶ ② 设置 ──▶ ③ 交付
```

```
┌ 共享给 Agent · 我的资料 ──────────────────────────────── [x] ┐
│  (1) 范围 ─── (2) 设置 ─── (3) 交付                          │
├──────────────────────────────────────────────────────────────┤
│ ① 共享哪些内容？                    │ 预览                    │
│  ( ) 整个 Drive                     │ Agent 将看到：          │
│  (o) Projects/项目A                 │ Projects/项目A          │
│  [ ] 再添加文件夹或文件…            │ 约 128 个文件           │
│                                     │ 不包含范围外的任何内容  │
├──────────────────────────────────────────────────────────────┤
│                                              [下一步]         │
└──────────────────────────────────────────────────────────────┘

┌ ② 设置 ─────────────────────────────────────────────────────┐
│ 角色    (o) 可建议（默认）  ( ) 只读  ( ) 可编辑 · M2 开放   │
│ 任务    [跟进方案二的评估，下周三前出结论                ]   │
│         可能指的是：方案二评估（项目）· 方案二（决定）       │
│ 预算    [ 800 v ] token    （400 / 800 / 1,600 / 3,200 / 自定义）│
│ 有效期  [ 7 天 v ]        （1 天 / 3 天 / 7 天 / 30 天）      │
│ ─ BRIEF 预览（v17）────────────────────────────── 612/800 ─ │
│  # BRIEF                                                     │
│  ## 你在做什么                                               │
│  跟进方案二的评估……                     [src:纪要/…#p2]      │
│  …                                                           │
│  已拦截 3 条（范围外）· 裁剪：“其他相关”降为只列标题          │
├──────────────────────────────────────────────────────────────┤
│ [上一步]                                       [下一步]       │
└──────────────────────────────────────────────────────────────┘

┌ ③ 交付 ─────────────────────────────────────────────────────┐
│ 挂载到    [选择文件夹…]  ~/tasks/proj-a                      │
│ 命令      oo context mount --drive 我的资料 --scope Projects/项目A \│
│             --to ~/tasks/proj-a --for "跟进方案二的评估…" \  │
│             --budget 800 --role suggester --expires 7d  [复制]│
│ 说明      在 Agent 的终端里运行这行命令，或直接点“一键执行”。 │
│                                       [一键执行]（主操作）   │
│ ─ 结果 ─────────────────────────────────────────────────── │
│ * 已挂载 · ~/tasks/proj-a/.context  · 到期 11-23            │
│ [在访达中打开] [查看 BRIEF] [完成]                           │
└──────────────────────────────────────────────────────────────┘
```

| 步骤 | 数据与行为 |
|---|---|
| ① 范围 | 默认取打开面板时的上下文（当前目录、右键对象或多选）；可追加多条路径（`scope` ≤ 64，`DrivePath`）；“整个 Drive”= 空 `scope`。范围数据形状复用 `DesktopCloudCreateScopeRequest`（`path`、`exclude`、`max_mode`），本地模式只用形状，不调 Cloud（§3.6.4） |
| ② 设置 | 角色：`suggester` 默认（M1 行为同只读，10 §7.9）；`editor` 禁用并显示“M2 开放”。任务：≤ 2,000 字（`DOCS_PACK_TASK_TOO_LONG`），输入停顿 600 ms 后调 `resolve` 显示“可能指的是”（低置信不自动选种子，00 §5.3 第 2 步）。预算：200–20,000，默认 800（10 §2.8 `pack.default_budget`）。有效期：1 h–30 d，默认 7 d。设置变化后 800 ms 防抖调用 `pack-compile`（dry 预览，返回 `ContextPack.brief_preview`、`tokens`、`trimmed`、`blocked`、`maybe`）；预览区显示“已拦截 N 条”只给数量（D26） |
| ③ 交付 | “选择文件夹…”= `pick-path { purpose: "mount-target" }`；命令由 `mountCommand.ts` 依 10 §7.9 生成（含 `--to` 的显示路径；`--drive` 用名称，名称含空格或非 ASCII 时用 id）；“复制”只复制命令；“一键执行”= `share-mount { compile, targetGrantId, expiresInSeconds }`（主进程调 `POST /shares`，守护进程完成物理挂载后返回），**不在 shell 里执行命令**（D32：CLI 子进程只作兜底）；返回的 `Share.command` 与本地生成的命令做一致性比对，不一致时以 `Share.command` 为准并记录告警 |

#### 3.6.2 状态与错误

| 情况 | 呈现 |
|---|---|
| 预览编译中 | 预览区顶部细进度条；“下一步”可用（预览不是前置条件） |
| `DOCS_PACK_NOTHING_VISIBLE` | 预览区换成说明“范围内还没有已发布的内容”，给“查看处理进度”；“一键执行”禁用 |
| `DOCS_PACK_SCOPE_INVALID` | 标出无效路径（`details.paths`） |
| `DOCS_PACK_BUDGET_TOO_SMALL` | 预算框下提示“至少 {min_budget}” |
| `DOCS_PACK_LEAK_GUARD` / `PROVENANCE_GAP` | 危险提示“编译被安全检查拦下（这是缺陷，请报告）”，附“复制诊断信息”（只含码与数量）；不可重试 |
| `DOCS_SHARE_TARGET_OCCUPIED` | “这个文件夹里已有不是本引擎创建的 .context”，给“换一个文件夹” |
| `DOCS_SHARE_TARGET_INVALID` | 按 `details.reason` 解释（不存在、在引擎状态目录内、是根目录或家目录） |
| `DOCS_SHARE_TARGET_WRITE_FAILED` | “写入失败，已回滚”，可重试（同一 `operationId`） |
| 一键执行进行中 | 主按钮内 `InlineLoading`“正在挂载…”；对话框不可关闭（`dismissalPolicy="action-required"`），直到返回 |
| 超时 `SO_TIMEOUT` | “结果待确认”，按钮变“重新查询”（复用 `operationId` 重放） |
| 引擎未就绪 | 第 ③ 步顶部说明并给“打开引擎页”；“复制命令”仍可用（命令在引擎启动后可用） |

#### 3.6.3 撤销确认

```
┌ 撤销 Share「跟进方案二」？ ─────────────────────────────┐
│ 撤销后：                                                  │
│  · Agent 不能再用这个 Share 拉取原文；                    │
│  · ~/tasks/proj-a/.context 会被删除。                     │
│ Agent 已读内容无法收回。                                  │
│                                   [取消] [撤销]（危险色） │
└───────────────────────────────────────────────────────────┘
```

`share-revoke`；返回 `RevokeResult.local_removed=false` 时补充“本地目录未能删除，请手动删除：{path}”并给“在访达中显示”；`notice_code` 存在时确认框里的那句话使用它对应的文案 key（`super-drive.errors.DOCS_SHARE_READ_CONTENT_IRREVOCABLE`，与 `share.revoke.irrevocable` 同文）。

#### 3.6.4 与 ShareWizard、DesktopCloudScope 的关系

- **复用**：步骤条、范围单选、模式单选、复制字段的组件与 `share.css` 样式（F-S6-01 把 `ShareStepper/ScopeOption/ModeOption/CopyField` 从 `ShareWizardDialog.tsx` 移到 `ShareWizardParts.tsx` 并导出，行为与快照不变，`tests/component/cloud/share/*.test.tsx` 全绿）。不复用 `ShareWizardDialog` 本体：它的目标步骤是 MCP/链接渠道（`shareTargets.ts`），交付是 Cloud MCP 端点，和本地挂载不同。
- **DesktopCloudScope**：本地 Drive（fs）不涉及 Cloud 项目，`createCloudScope` 需要 Cloud 会话与 `projectId`，所以 M1 只复用 `DesktopCloudCreateScopeRequest` 的形状作为范围模型；L7 Cloud 代理模式下（Drive 位于 Puppyone Cloud 的桶），范围会同步建成 `DesktopCloudScope`（`max_mode: "r"`，suggester 在 M2 为 `"rw"` 仅限 `suggest/`），由 F-S12-01 实现。见 §8.2 Q4。

### 3.7 共享列表

```
┌ 共享 · 我的资料 ─────────────────────── [只看生效中 v] [共享…] ┐
│ 任务 / 范围                    角色    版本  状态       到期     │
│ ───────────────────────────────────────────────────────────────── │
│ 跟进方案二 · Projects/项目A     可建议  v18   * 生效中    6 天后  [...]│
│   └ 由 11-14 的 Share 刷新而来（v15，已刷新）                      │
│ 整理人员表 · People             只读    v12   ! 需刷新    2 天后  [刷新]│
│   内容有变化：3 条事实、1 个文件                                  │
│ 周报汇总 · 纪要                 可建议  v9    已过期      —       [...]│
│ 客户回访 · Clients/B            只读    v7    已撤销      —       [...]│
└───────────────────────────────────────────────────────────────────┘
```

| 元素 | 数据与行为 |
|---|---|
| 列表 | `shares-list { driveId }` 全部页；按状态排序：`drifted` → `active` → 终态；终态默认折叠在“历史”分组里 |
| 刷新链 | `refreshed_from` 把旧 Share 链到新 Share 下（`ShareChain`）；默认只显示链头（最新的 `active`）与一行“由 … 刷新而来”，展开看整条链 |
| 行操作 | 生效中：查看 BRIEF、在访达中打开、复制命令、撤销；需刷新：刷新（主操作）、查看变化、撤销；已过期/已撤销/已刷新：查看 BRIEF（只读）、再共享（以同参数打开分享面板） |
| 刷新 | `share-refresh`；返回新 Share 后链头更新；S10 起刷新前先显示“自上次以来的变化”预览（新 BRIEF 头部的差异节），M1 直接刷新 |
| 挂载缺失 | `mount.present=false`（用户手动删了目录）：行内提示“本地目录已不存在”，给“重新挂载到…” |
| 实时 | `share.created`、`share.state_changed`、`share.mount_removed`、`refs.accessed`（行内显示“Agent 最近拉取原文 {n} 次”，只作信息） |

`BriefPreview`（S6）：`pack-read { packId }` 的 `brief_preview` 用现有 Markdown 只读渲染（`react-markdown` + `remark-gfm` 已在依赖中，`package.json`）；`[src:…]` 识别为按钮，点后在详情栏打开对应文件并显示该证据（L4 验收 3）。

### 3.8 引擎状态页 / 抽屉

```
┌ 引擎 ───────────────────────────────────────────────────────────┐
│ * 健康 · oo 0.3.0 · 接口 1.0 · 已运行 2 小时    [停止] [重启]    │
│ ─ 检查 ──────────────────────────────────────────────────────── │
│  * 平台受支持（macOS）                                           │
│  * 找到 oo（/opt/homebrew/bin/oo，0.3.0）          [更换…]       │
│  * Docker 已安装（27.3）并在运行                                 │
│  * 磁盘可用 82 GB                                                │
│ ─ 组件 ──────────────────────────────────────────────────────── │
│  数据库        正常                                              │
│  解析服务      正常（docling 2.x）                               │
│  模型          未配置 · 只用确定性规则          [配置模型]        │
│  内容存储      正常 · 3.2 GB / 原件缓存上限 10 GB                │
│ ─ 队列 ──────────────────────────────────────────────────────── │
│  排队 9 · 运行 4 · 需手动 3 · 已隔离 1 · 等待中 1                │
│ ─ 选项 ──────────────────────────────────────────────────────── │
│  [ ] 退出 App 后保持引擎运行                                     │
│  状态目录 ~/Library/Application Support/PuppyOne/super-ontology  [在访达中显示] │
│  [查看日志]                                                      │
└─────────────────────────────────────────────────────────────────┘
```

- 入口：分区导航“引擎”（整页）与工具栏 `EngineStatusPill`（点开右侧抽屉，内容相同的精简版）。
- 数据：`EngineSnapshot`（`preflight`、`health.components`、`health.queue`、`health.disk`）。
- 不健康时页面顶部换成对应状态的说明卡（§4.4），主操作按状态变化（安装 Docker、启动 Docker 后重试、启动引擎、重启、升级 oo）。
- “手动启动命令”折叠区用 `CloudCommandBlock` 显示 `oo serve --state-dir <…>`，给需要在终端排查的用户。

### 3.9 设置：Drive 登记、模型开关、用量

```
┌ 设置 ───────────────────────────────────────────────────────────┐
│ Drive                                            [登记新 Drive]  │
│  我的资料   本地文件夹 ~/Documents/资料   1,284 个文件  [编辑][...]│
│  团队桶     S3 · team-docs/shared         5,210 个文件  [编辑][...]│
│ ─ 我的资料 ──────────────────────────────────────────────────── │
│  名称          [我的资料                ]                        │
│  暂停处理      [ ]                                               │
│  排除          [*.bak] [drafts/**] [+ 添加]                      │
│  模型补抽      [x] 使用模型（未配置时只跑规则）                   │
│  每日调用上限  [ 2000 ]                                          │
│  自动发布阈值  [ 0.80 ]（低于它的进入待确认）                     │
│  [立即同步] [深度重扫]                        [移除此 Drive]     │
│ ─ 模型 ──────────────────────────────────────────────────────── │
│  引擎模型      未配置                         [配置…]             │
│ ─ 用量（近 7 天）──────────────────────────────────────────────── │
│  我的资料   今日 312 / 2000 次 · 缓存命中 41% · 3 个文件触顶     │
│  ▁▂▃▅▂▁▃（按天）                                                  │
└─────────────────────────────────────────────────────────────────┘
```

| 功能 | 数据与行为 |
|---|---|
| 登记 fs Drive | 对话框：名称 + `pick-path { purpose: "drive-root" }` + 可选排除；`drive-create`；成功后快照进入“正在应用 Drive 变更”，完成后自动切到新 Drive；`DOCS_SOURCE_DRIVE_EXISTS` → “这个文件夹已登记为 {name}”并给“打开它” |
| 登记 s3 Drive（S4） | 字段：endpoint（`https://` 或 loopback http）、region、bucket、prefix、path style、Access Key ID、Secret Access Key、Session Token（可选）；密钥只写不读（保存后显示“已保存”与“替换…”）；主进程写成 `file:` secret_ref（§1.2.3 `secret-files.mjs`）；登记时引擎做一次只读探测，错误按 `DOCS_SOURCE_AUTH_FAILED/BUCKET_NOT_FOUND/CREDENTIAL_MISSING/ENDPOINT_INVALID` 显示在对应字段下 |
| 编辑 | `drive-update { revision, patch }`：`name`、`paused`、`exclude`、`model_enabled`、`llm_daily_calls`、`review.auto_publish_bp`（显示为 0.50–1.00）；乐观更新 + 412 回滚（§1.7.5） |
| 移除 | 确认框说明“只取消登记，不会删除你的文件；该 Drive 的索引与证据会被删除”；`drive-delete`；`DOCS_SOURCE_DRIVE_HAS_ACTIVE_SHARES` → 列出 `details.share_ids` 对应的 Share，给“先撤销这些 Share” |
| 立即同步 / 深度重扫 | `drive-sync { deep }`；`DOCS_SOURCE_SYNC_RUNNING` 视为成功 |
| 模型 | 引擎模型配置目前不在控制 API 中（10 §2.8 的 `docs.json.model`），见 §8.2 Q5；M1 先显示状态（`health.components.model`）与“配置…”占位（打开引擎状态目录说明），接口落地后接入 |
| 用量 | `usage-read { driveId, days: 7 }`：今日调用 / 上限、缓存命中、`files_limited`；`usage.changed` 实时更新 |

### 3.10 首次使用引导

触发：Super Drive 第一次打开且没有任何 Drive。

```
┌──────────────────────────────────────────────────────────────┐
│                       [Drive 插图]                             │
│              把资料放进 Drive，交给 Agent 用                   │
│   Super Drive 会读懂你的文件，按任务把需要的部分共享给 Agent。 │
│                                                                │
│   1  启动本地引擎        * 健康                                │
│   2  选择一个文件夹      [选择文件夹…]   或  [导入示例文件夹]   │
│   3  等待处理完成，然后共享给 Agent                            │
└──────────────────────────────────────────────────────────────┘
```

- 第 1 步按引擎状态显示（未就绪时这一步就是主操作：安装 Docker / 启动引擎），不就绪时第 2 步禁用并说明原因。
- “导入示例文件夹”（60 P3、§4 第 4 步）：把应用资源里的示例资料复制到用户选择的位置（默认 `~/Documents/Super Drive 示例`），再登记为 Drive。示例资料来源与打包方式见 §8.2 Q11。
- 完成第 2 步后引导消失，列表接管；第 3 步以队列面板与横幅体现，不另做弹窗。

---

## 4. 状态页面矩阵

所有矩阵都实现在 `superDriveStateModel.ts`（单一真相源），本章是它的规格。图标名只用 Puppyone 源码里已经从 `lucide-react` 引入过的图标（在 `src/`、`packages/` 中检索 `from "lucide-react"` 得到的集合），避免引入当前版本不存在的名字。tone 对应 `StatusBadge` 的 tone 与颜色 token：`neutral`→`--po-text-muted` 底 `--po-control`；`info`→`--po-info`；`success`→`--po-success`；`warning`→`--po-warning`；`danger`→`--po-danger`（底色均为 `color-mix(… 13–14%, transparent)`，见 `shared-ui-patterns.css:92-118`）。文案 key 省略公共前缀 `super-drive.`。

### 4.1 文件状态（00 §5.1、10 §4.2、`FileState` 枚举）

| 状态 | 分组 | 图标 | tone | 文案 key | 中文 | English | 可执行操作 | 可重试 |
|---|---|---|---|---|---|---|---|---|
| `uploading`（仅客户端） | 处理中 | `CloudUpload` | info | `file.state.uploading` | 上传中 {percent} | Uploading {percent} | 取消；失败后重试 | 客户端重试 |
| `stored` | 处理中 | `File` | neutral | `file.state.stored` | 已存入 | Stored | 打开详情 | 否 |
| `queued` | 处理中 | `Clock3` | neutral | `file.state.queued` | 排队中 | Queued | 查看队列 | 否 |
| `queued` + `blocked_reason` | 处理中 | `Clock3` | warning | `file.state.blocked.<reason>` | 等待中：{原因} | Waiting: {reason} | 按原因（见下表） | 否（原因解除后自动继续） |
| `parsing` | 处理中 | `PulseGridLoader` | info | `file.state.parsing`、`file.progress.<unit>` | 解析中 · 第 {done}/{total} 页 | Parsing · page {done} of {total} | — | 否 |
| `parsed` | 处理中 | `FileText` | info | `file.state.parsed` | 已解析 | Parsed | 查看解析结果 | 否 |
| `extracting` | 处理中 | `PulseGridLoader` | info | `file.state.extracting` | 抽取中 · 第 {done}/{total} 批 | Extracting · batch {done} of {total} | — | 否 |
| `indexed` | 处理中 | `ListTree` | info | `file.state.indexed` | 已建索引，等待发布 | Indexed, publishing | 查看实体 | 否 |
| `published` | 已发布 | `CheckCircle2` | success | `file.state.published` | 已发布 | Published | 查看实体；共享 | 否 |
| `published` + `pending_review>0` | 已发布 | `CheckCircle2` + `ListTodo` 计数 | success + 计数点 `--po-notification` | `file.state.publishedWithReview` | 已发布 · {count} 条待确认 | Published · {count} to review | 去确认 | 否 |
| `published` + `state_code=DOCS_EXTRACT_LLM_LIMIT_*` | 已发布 | `CheckCircle2` | success（附 info 注） | `file.state.publishedLimited` | 已发布 · 模型调用已达上限，部分内容只用规则 | Published · model limit reached, rules only for part | 查看用量 | 否 |
| `parse_failed`（自动重排中，`retryable=true` 且 `attempt<3`） | 失败 | `RotateCw` | warning | `file.state.parseRetrying` | 解析失败，将自动重试（第 {attempt}/3 次） | Parse failed, retrying automatically ({attempt} of 3) | 查看原因 | 自动 |
| `parse_failed`（需手动） | 失败 | `CircleAlert` | danger | `file.state.parse_failed` + `errors.<code>` | 解析失败 · {原因} | Parse failed · {reason} | 重试；安全模式重试（OOM/崩溃）；在访达中显示；查看原因 | 是（`retryable` 为真时） |
| `parse_failed` + `DOCS_PARSE_QUARANTINED` | 失败 | `ShieldAlert` | danger | `errors.DOCS_PARSE_QUARANTINED` | 已隔离：多次导致解析服务崩溃 | Quarantined: repeatedly crashed the parser | 安全模式重试（仅手动） | 仅手动 |
| `parse_failed` + `DOCS_SOURCE_PERMISSION_DENIED` | 失败 | `LockKeyhole` | danger | `errors.DOCS_SOURCE_PERMISSION_DENIED` | 没有读取权限 | No permission to read | 在访达中显示；重试（下次同步也会自动重试） | 是 |
| `extract_partial` | 失败 | `TriangleAlert` | warning | `file.state.extract_partial` | 部分抽取 · {n} 批失败，已成功的内容照常发布 | Partly extracted · {n} batches failed; the rest is published | 重试失败部分；查看 | 是 |
| `unsupported` | 跳过 | `CircleSlash2` | neutral | `file.state.unsupported` + `errors.<code>` | 暂不支持 · {原因} | Not supported · {reason} | 了解支持的格式 | 否 |
| `too_large` | 跳过 | `FileWarning` | warning | `file.state.too_large` | 超过处理上限（{limit}） | Over the size limit ({limit}) | 在访达中显示 | 否 |
| `encrypted` | 跳过 | `LockKeyhole` | warning | `file.state.encrypted` | 已加密，无法读取 | Encrypted, can't be read | 在访达中显示 | 否（解密后替换文件即重新处理） |
| `duplicate` | 已发布 | `CopyPlus` | neutral | `file.state.duplicate` | 与 {name} 内容相同，已复用结果 | Same as {name}; results reused | 打开原文件 | 否 |
| `superseded`（旧版本） | 历史 | `History` | neutral | `file.state.superseded` | 已被新版本取代 | Replaced by a newer version | 查看当前版本 | 否 |
| `deleted` | 历史 | `Trash2` | neutral（名称删除线） | `file.state.deleted` | 已从 Drive 删除 · 证据已撤回 | Removed from the Drive · evidence withdrawn | 查看最后状态 | 否 |
| `outdated` | 历史 | `RefreshCw` | info | `file.state.outdated` | 可更新 · 解析器或规则已升级，后台重跑中 | Update available · reprocessing in the background | （S10）立即更新所选 | 否 |
| 未知值 | — | `CircleHelp` | neutral | `file.state.unknown` | 未知状态（{state}） | Unknown state ({state}) | 查看详情 | 否 |

`blocked_reason`（10 §8.2 `BlockedReason`）：

| reason | 中文 | English | 操作 |
|---|---|---|---|
| `parser_unavailable` | 解析服务暂不可用 | Parser unavailable | 打开引擎页 |
| `disk_low` | 磁盘空间不足 | Low disk space | 打开引擎页（清理建议） |
| `db_degraded` | 数据库暂不可用 | Database unavailable | 打开引擎页 |
| `llm_limit` | 模型限流或额度用尽，稍后继续 | Model rate-limited, will resume | 查看用量 |
| `drive_paused` | Drive 已暂停 | Drive paused | 恢复处理 |

列表筛选的分组：处理中（`stored queued parsing parsed extracting indexed`）、已发布（`published duplicate`）、失败（`parse_failed extract_partial`）、跳过（`unsupported too_large encrypted`）、历史（`superseded deleted outdated`）。分组外的未知状态单独列在“其他”。

### 4.2 Share 状态（00 §5.4、10 §7.6、`ShareState` 枚举）

| 状态 | 图标 | tone | 文案 key | 中文 | English | 可执行操作 | 可重试 |
|---|---|---|---|---|---|---|---|
| `active` | `Share2` | success | `share.state.active` | 生效中 | Active | 查看 BRIEF；在访达中打开；复制命令；撤销 | — |
| `active` 且由刷新产生（`refreshed_from` 非空） | `Share2` | success + 链接注 | `share.state.activeRefreshed` | 生效中 · 由 {date} 的 Share 刷新而来 | Active · refreshed from {date} | 同上；查看刷新链 | — |
| `active` 且 `mount.present=false` | `FolderOpen` | warning | `share.mount.missing` | 生效中 · 本地目录已不存在 | Active · local folder missing | 重新挂载到…；撤销 | 是 |
| `drifted` | `TriangleAlert` | warning | `share.state.drifted` | 需刷新 · {facts} 条事实、{files} 个文件有变化 | Needs refresh · {facts} facts, {files} files changed | 刷新（主操作）；查看变化（S10）；撤销 | — |
| `refreshed`（终态） | `RotateCw` | neutral | `share.state.refreshed` | 已刷新 · 已由新 Share 替代 | Refreshed · replaced by a newer share | 查看新 Share；查看 BRIEF（只读） | 否 |
| `expired`（终态） | `Clock3` | neutral | `share.state.expired` | 已过期 · {date} | Expired · {date} | 再共享（同参数）；查看 BRIEF | 否 |
| `revoked`（终态） | `Unlink` | neutral | `share.state.revoked` | 已撤销 · {date} | Revoked · {date} | 再共享；查看 BRIEF | 否 |
| 进行中：挂载 | `LoaderCircle` | info | `share.pending.mounting` | 正在挂载… | Mounting… | — | 失败可重试（同 `operationId`） |
| 进行中：刷新 / 撤销 | `LoaderCircle` | info | `share.pending.refreshing` / `share.pending.revoking` | 正在刷新… / 正在撤销… | Refreshing… / Revoking… | — | 同上 |
| 未知值 | `CircleHelp` | neutral | `share.state.unknown` | 未知状态（{state}） | Unknown state ({state}) | 查看详情 | 否 |

到期提醒：`active/drifted` 且 `expires_at` 在 24 小时内 → 到期列显示 warning 色“{n} 小时后到期”，行操作多一个“延长”（= 以同参数刷新，因为契约没有单独的延期接口；见 §8.3 G6）。

### 4.3 待确认种类（00 §6.2、10 §8.2 `ReviewKind`）

| 种类 | 首次 | 图标 | tone | 文案 key | 中文 | English | 可用决定 |
|---|---|---|---|---|---|---|---|
| `low_confidence_fact` | M1 | `CircleHelp` | info | `review.kind.low_confidence_fact` | 低置信事实 | Low-confidence fact | 接受 / 拒绝 / 编辑（值、有效期） |
| `entity_merge` | M1 | `Link2` | info | `review.kind.entity_merge` | 可能是同一个 | Possible duplicate | 合并（accept）/ 保持分开（reject）/ 改合并目标（edit） |
| `alias_conflict` | M1 | `ArrowLeftRight` | warning | `review.kind.alias_conflict` | 别名冲突 | Alias conflict | 选择归属（edit）/ 拒绝该别名（reject） |
| `orphaned_overlay` | M1 | `PencilLine` | warning | `review.kind.orphaned_overlay` | 人工修改失去出处 | Manual edit lost its source | 保留（accept）/ 删除（reject） |
| `type_promotion` | M2（S9） | `PackagePlus` | info | `review.kind.type_promotion` | 建议新增类型 | Suggested new type | 升格（accept，可改名 edit）/ 拒绝 |
| `suggestion` | M2（S7） | `MessageSquare` | info | `review.kind.suggestion` | Agent 的建议 | Agent suggestion | 接受 / 拒绝（修订模式，S8） |
| `reprocess_diff` | M2（S10） | `RefreshCw` | info | `review.kind.reprocess_diff` | 重跑后的差异 | Changes after reprocessing | 接受 / 拒绝（差异视图） |
| 未知值 | — | `CircleHelp` | neutral | `review.kind.unknown` | 未知种类（{kind}） | Unknown kind ({kind}) | 只读，可拒绝 |

逐项结果（`ReviewDecideResult.results[].status`）：

| status | 中文 | English | 处理 |
|---|---|---|---|
| `ok` | — | — | 移除 |
| `already_decided` | 已被处理 | Already handled | 移除 |
| `conflict`（`DOCS_REVIEW_STALE_REVISION` / `MERGE_CONFLICT`） | 内容已变化，请再看一次 | Changed since loaded; review again | 重拉该项并恢复 |
| `not_found` | 已不存在 | No longer exists | 移除 |
| `invalid` | 这个决定不适用于此项 | This decision doesn't apply | 恢复并提示 |

### 4.4 引擎状态（`EngineSnapshot.status`）

| 状态 | 判定 | 图标 | tone | 文案 key | 中文 | English | 可执行操作 | 可重试 |
|---|---|---|---|---|---|---|---|---|
| `idle` | 服务尚未初始化（S1 骨架固定返回此值），或 IPC 尚未连上 | `Server` | neutral | `engine.status.idle` | 引擎未连接 | Engine not connected | 检查环境（触发 preflight） | 是 |
| `probing` | 检查中 | `LoaderCircle` | neutral | `engine.status.probing` | 正在检查本地环境… | Checking your setup… | — | — |
| `unsupported_platform` | Windows（`oo` 发布平台只有 darwin/linux，OO `packages/cli/package.json` `ooPlatforms`） | `Monitor` | neutral | `engine.status.unsupported_platform` | 这台电脑的系统暂不支持本地引擎 | Local engine isn't supported on this system yet | 了解更多 | 否 |
| `oo_missing` | PATH 与手动指定都找不到 `oo` | `SquareTerminal` | warning | `engine.status.oo_missing` | 没有找到 oo 命令行 | The oo command line isn't installed | 查看安装方法（复制 `npm install -g …`）；手动指定位置…；重试 | 是 |
| `docker_missing` | 找不到 `docker` 可执行文件 | `Box` | warning | `engine.status.docker_missing` | 需要先安装 Docker | Docker is required | 打开 Docker 下载页；我已安装，重试 | 是 |
| `docker_stopped` | `docker info` 失败，或 `oo serve` 退出码 3 | `Box` | warning | `engine.status.docker_stopped` | Docker 没有在运行 | Docker isn't running | 我已启动 Docker，重试 | 是 |
| `stopped` | 检查通过，`oo serve` 未运行 | `Server` | neutral | `engine.status.stopped` | 本地引擎未启动 | Local engine is stopped | 启动引擎（主操作） | — |
| `starting` | 已拉起，等待 `serve.json`/`livez`/health，或 `health.status=starting` | `LoaderCircle` | info | `engine.status.starting` | 正在启动本地引擎…（首次约需 1 分钟） | Starting the local engine… (about a minute the first time) | 查看日志 | — |
| `healthy` | `health.status=ok` | `CheckCircle2` | success | `engine.status.healthy` | 健康 | Healthy | 停止；重启 | — |
| `degraded` | `health.status=degraded`（某组件 `degraded/unavailable/limited/blocked`） | `TriangleAlert` | warning | `engine.status.degraded` | 部分功能受限：{组件} | Partly limited: {component} | 查看组件；重启 | 是 |
| `restarting`（`reason=process_exit`） | Desktop 拉起的 `oo serve` 意外退出，退避重启中 | `RotateCw` | warning | `engine.status.restarting` | 引擎意外退出，正在重启（第 {n} 次，{s} 秒后） | Engine stopped unexpectedly; restarting ({n}, in {s}s) | 立即重启；查看日志 | 自动 |
| `restarting`（`reason=engine_container`） | 守护进程活着、引擎容器不可达（`DOCS_ENGINE_UNAVAILABLE`） | `RotateCw` | warning | `engine.status.restartingContainer` | 引擎已断开，正在恢复… | Engine disconnected, recovering… | 查看日志 | 自动 |
| `restarting`（`reason=drive_change`） | 登记/删除本地 Drive 后容器重建 | `RotateCw` | info | `engine.status.applyingDriveChange` | 正在应用 Drive 变更… | Applying Drive changes… | — | 自动 |
| `crashed` | 10 分钟内 3 次重启失败，或退出码 4/5/6/7，或凭证文件校验失败 | `AlertCircle` | danger | `engine.status.crashed` + `errors.<code>` | 引擎无法启动：{原因} | The engine can't start: {reason} | 重试；查看日志；打开状态目录 | 手动 |
| `incompatible` | `OO-Docs-Api` 主版本 ≠ 1、`DOCS_ENGINE_VERSION_UNSUPPORTED`、`DOCS_ENGINE_SCHEMA_MISMATCH` | `ShieldAlert` | danger | `engine.status.incompatible` | 引擎版本与 App 不兼容（引擎 {engine}，需要接口 1.x） | Engine version isn't compatible (engine {engine}, needs API 1.x) | 查看升级方法；手动指定其他 oo | 否 |
| `stopping` | 正在停止 | `LoaderCircle` | neutral | `engine.status.stopping` | 正在停止… | Stopping… | — | — |

`degraded` 的组件明细（`ComponentStatus`，10 §8.2）：

| 组件 \ 状态 | `ok` | `starting` | `degraded` | `unavailable` | `not_configured` | `limited` | `blocked` |
|---|---|---|---|---|---|---|---|
| `db`（数据库） | 正常 | 启动中 | 变慢 | 不可用：处理暂停，读写返回错误 | — | — | — |
| `parser`（解析服务） | 正常 | 启动中 | 变慢 | 不可用：PDF/Word/PPT 暂停，MD/TXT/CSV/XLSX 照常 | — | — | — |
| `model`（模型） | 正常 | — | 变慢 | 不可达：退回规则 | 未配置：只用规则（不算降级，不变黄） | 已达上限：退回规则 | 暂停 |
| `cas`（内容存储） | 正常 | — | 余量低 | 不可用 | — | — | 只读（磁盘满） |
| `engine`（容器内引擎） | 正常 | 启动中 | 变慢 | 不可达 | — | — | — |

文案 key：`engine.component.<name>.<status>`。`model=not_configured` 不让整体变成 `degraded` 的视觉（由“未配置模型”横幅单独表达，D18 不配模型也必须能用）；10 §4.12 写的 `cas.status=low` 不在 `ComponentStatus` 枚举里，前端按 `degraded` 呈现，见 §8.2 Q12。

### 4.5 连接状态（SSE）

| 状态 | 呈现 | 文案（中 / 英） |
|---|---|---|
| `connected` | 无提示 | — |
| `reconnecting` | 横幅 warning；状态列右上角细字“可能不是最新” | 与引擎的连接已断开，正在重连（第 {n} 次）· Connection lost, reconnecting (attempt {n}) |
| `resyncing`（410 / 缺口 / 引擎重启） | 横幅 info；列表顶部细进度条；重拉中的目录 `aria-busy` | 正在重新同步… · Resyncing… |
| 补齐完成 | `role=status` 通知 3 s | 已同步 {count} 条变化 · Caught up on {count} changes |
| `offline`（引擎非 healthy/degraded） | 不单独提示，由引擎状态横幅表达 | — |

### 4.6 页面级状态

| 场景 | 判定 | 呈现 | 文案（中 / 英） | 操作 |
|---|---|---|---|---|
| flag 开但桥缺失 | `superOntology` 未暴露（`SO_BRIDGE_UNAVAILABLE`） | 整页 `SuperDriveEntryState` | Super Drive 在此版本不可用 · Super Drive isn't available in this build | — |
| 首次使用 | 没有 Drive | §3.10 引导 | 把资料放进 Drive，交给 Agent 用 · Put your files in a Drive and share them with agents | 启动引擎；选择文件夹；导入示例文件夹 |
| 引擎不可用（任何非 healthy/degraded 状态） | §4.4 | 有已加载数据时：保留列表（只读）+ 顶部引擎横幅；没有数据时：整页 `SuperDriveEntryState` 对应变体 | 见 §4.4 | 见 §4.4 |
| 空 Drive | `files_total=0` 且最近同步 `complete` | 主区 `EmptyState` + 拖拽区常显 | 这个 Drive 还是空的 · This Drive is empty；副文：把文件拖到这里，或在访达里放进 {path} | 上传文件…；在访达中打开 |
| 同步中且尚无文件 | `last_sync.status=running` 且 `files_total=0` | 骨架 + 文字 | 正在读取文件列表… · Reading the file list… | — |
| 空目录 / 筛选无结果 | 当前目录 0 项 / 筛选后 0 项 | `EmptyState` | 这个文件夹是空的 · This folder is empty / 没有符合条件的文件 · No files match | 清除筛选 |
| 全部处理中 | Drive 内无终态文件，处理中 > 0 | 列表正常；队列面板展开；顶部 info 横幅（可关） | 正在读懂 {count} 个文件，完成后就能共享 · Reading {count} files; you can share once they're done | 查看队列 |
| 部分失败 | `parse_failed + extract_partial > 0` | warning 横幅（§4.7） | {count} 个文件处理失败 · {count} files failed | 只看失败；全部重试 |
| 待确认 > 0 | `pending_review > 0` | 左栏“待确认”计数；顶栏导航项点（`getDesktopNavigationBadge`）；列表行计数 | {count} 条待确认 · {count} to review | 去确认 |
| Share 漂移 | 有 `drifted` | 左栏“共享”计数（warning）；共享列表置顶；info 横幅（可关） | {count} 个共享的内容有变化，需要刷新 · {count} shares need a refresh | 打开共享 |
| Share 即将过期 / 已过期 | 24 h 内到期 / 新进入 `expired` | 行内到期提示；`share.state_changed→expired` 时 `role=status` 通知 | “跟进方案二”已过期 · "…" has expired | 再共享 |
| 未配置模型 | `health.components.model.status=not_configured` 或当前 Drive `model_enabled=false` | info 横幅（按 Drive 可关，关后记忆） | 未配置模型：只用确定性规则，能识别的内容较少 · No model configured: using deterministic rules only | 配置模型；不再提示 |
| 模型达上限 | `usage.changed.limited=true` | info 横幅 | 今日模型调用已达上限，其余文件只用规则 · Today's model limit is reached; rules only for the rest | 查看用量；调整上限 |
| 断线重连 | §4.5 | §4.5 | §4.5 | — |
| 大目录（1 万文件） | `total > 200`（`SIDEBAR_VIRTUALIZATION_THRESHOLD`） | 虚拟滚动：挂载行 ≤ 120（`SIDEBAR_VIRTUALIZATION_MAX_MOUNTED_ROWS`）；按 `limit=500` 键集分页；未加载区间用骨架行占位（`aria-rowcount=total`）；滚动条按 `total` 计算高度；快速拖到底部时顺序加载并显示“正在加载 {loaded}/{total}” | — | — |
| 权限不足（Drive 级） | `DOCS_SOURCE_ROOT_MISSING` 或根目录不可读 | danger 横幅 | 无法读取 {path}：文件夹不存在或没有权限 · Can't read {path}: missing or no permission | 在访达中显示；重新选择文件夹；立即同步 |
| 权限不足（文件级） | `state_code=DOCS_SOURCE_PERMISSION_DENIED` | 行内 §4.1 | — | — |
| 磁盘不足（低） | `DOCS_ENGINE_DISK_LOW` / `cas` 余量低 / `blocked_reason=disk_low` | warning 横幅 | 磁盘空间不足，处理已暂停（剩余 {free}） · Low disk space; processing paused ({free} left) | 打开引擎页（占用明细） |
| 磁盘不足（满） | `507 DOCS_ENGINE_DISK_FULL` | danger 横幅；所有写操作按钮禁用并说明原因 | 磁盘已满，引擎进入只读模式 · Disk full; the engine is read-only | 打开引擎页 |
| 上传时磁盘满 | `SO_DISK_FULL` | 队列条目失败 | 磁盘空间不足，未能复制 · Not enough disk space to copy | 重试 |
| 大规模删除被挂起 | `drive.sync_held` / `DOCS_SOURCE_MASS_DELETE_HELD` | danger 横幅（不可关） | 检测到将移除 {count} 个文件，已暂停清理。是你删的吗？ · {count} files would be removed; cleanup is paused. Did you delete them? | 确认移除（`confirm_mass_delete`）；先不处理（打开文件夹检查） |
| 同步失败 | `drive.sync_failed` | warning 横幅 | 上次同步失败：{原因} · Last sync failed: {reason} | 立即同步 |
| 列举不完整 | `DOCS_SOURCE_LIST_INCOMPLETE`（health） | info 横幅 | 部分文件夹没能读完，已暂停清理 · Some folders couldn't be listed; cleanup paused | 立即同步 |
| Drive 已暂停 | `drive.status=paused` | info 横幅 | 这个 Drive 已暂停处理 · Processing is paused for this Drive | 恢复 |
| Drive 删除中 | `drive.status=deleting` | 整页只读 + 进度文字 | 正在移除这个 Drive… · Removing this Drive… | — |
| 路径冲突 | `DOCS_SOURCE_PATH_COLLISION` | 行内（unsupported）+ 详情里列出 `other_file_id` | 与 {other} 只有大小写或编码不同 · Differs from {other} only by case or encoding | 打开另一个文件 |
| 版本不兼容 | `incompatible` | 整页 | §4.4 | §4.4 |

### 4.7 横幅优先级（同时最多显示 2 条）

1. 引擎不可用（`crashed`、`incompatible`、`docker_*`、`oo_missing`、`stopped`，有已加载数据时）
2. 磁盘已满 / 大规模删除被挂起（不可关）
3. 断线重连 / 重新同步
4. 权限不足（Drive 级）、同步失败、列举不完整
5. 部分失败
6. 磁盘低、引擎 `degraded`、`restarting`
7. Share 漂移
8. 模型达上限、未配置模型、全部处理中、Drive 已暂停

同级按出现时间先后。被挤下去的横幅在第二条右侧显示“+{n}”，点开展开全部。可关闭的横幅关闭后只在其条件“消失后再次出现”时重新显示（按 Drive 记忆）。

### 4.8 错误码 → 文案

规则：key `errors.<CODE>`；没有专门文案时用 `errors.family.<SOURCE|PARSE|EXTRACT|REVIEW|PACK|SHARE|ENGINE>`，再不行用 `errors.unknown`（“出了点问题（{code}）”）。`contracts/docs-api/errors.v1.json` 中**每个**码都必须有 `errors.<CODE>` key（架构测试强制），下表给出用户最常见到的部分与建议动作；其余码的文案在 F-S1-04 / F-S3-02 一次补齐。

| code | 中文 | English | 建议动作 |
|---|---|---|---|
| `DOCS_SOURCE_ROOT_MISSING` | 文件夹不存在或已移动 | The folder is missing or was moved | 重新选择文件夹 |
| `DOCS_SOURCE_PERMISSION_DENIED` | 没有读取权限 | No permission to read | 在访达中检查权限 |
| `DOCS_SOURCE_AUTH_FAILED` | 访问凭证被拒绝 | The credentials were rejected | 检查 Access Key |
| `DOCS_SOURCE_CREDENTIAL_MISSING` | 找不到保存的凭证 | Saved credentials can't be found | 重新输入凭证 |
| `DOCS_SOURCE_BUCKET_NOT_FOUND` | 找不到这个桶 | Bucket not found | 检查桶名与区域 |
| `DOCS_SOURCE_ENDPOINT_INVALID` | 地址必须是 https（本机地址除外） | The endpoint must use https (except localhost) | 修改地址 |
| `DOCS_SOURCE_UNSTABLE` | 文件还在写入，稍后自动处理 | The file is still being written | 等待 |
| `DOCS_SOURCE_TOO_LARGE` | 超过处理上限（{limit}） | Over the size limit ({limit}) | — |
| `DOCS_SOURCE_DRIVE_FILE_LIMIT` | 超过单个 Drive 1 万个文件的上限 | Over the 10,000-file limit for one Drive | 拆分为多个 Drive |
| `DOCS_SOURCE_SYMLINK_SKIPPED` | 快捷方式（符号链接）不会被读取 | Symbolic links aren't followed | — |
| `DOCS_PARSE_UNSUPPORTED_FORMAT` | 不支持这种格式 | This format isn't supported | 了解支持的格式 |
| `DOCS_PARSE_LEGACY_OFFICE` | 旧版 Office 格式（.doc/.xls/.ppt），请另存为新格式 | Legacy Office format; save as .docx/.xlsx/.pptx | — |
| `DOCS_PARSE_SCANNED_PDF` | 扫描件，需要文字识别（后续版本支持） | Scanned PDF; needs OCR (coming in a later version) | — |
| `DOCS_PARSE_IMAGE_OCR_M2` | 图片，需要文字识别（后续版本支持） | Image; needs OCR (coming in a later version) | — |
| `DOCS_PARSE_ENCRYPTED` | 文件已加密 | The file is encrypted | — |
| `DOCS_PARSE_CORRUPT` | 文件已损坏 | The file is damaged | 用原程序打开检查 |
| `DOCS_PARSE_ZIP_BOMB` | 文件结构异常（压缩比过高），已停止读取 | Unsafe file structure; reading stopped | — |
| `DOCS_PARSE_LIMIT_PAGES` | 页数超过上限（{limit}） | Too many pages (limit {limit}) | — |
| `DOCS_PARSE_TIMEOUT` | 解析超时 | Parsing timed out | 重试 |
| `DOCS_PARSE_OOM` | 解析时内存不足 | Ran out of memory while parsing | 安全模式重试 |
| `DOCS_PARSE_CRASH` | 解析服务崩溃 | The parser crashed | 安全模式重试 |
| `DOCS_PARSE_QUARANTINED` | 已隔离：多次导致解析服务崩溃 | Quarantined after repeated crashes | 安全模式重试 |
| `DOCS_PARSE_ENCODING` | 文本编码无法识别 | Unrecognized text encoding | 另存为 UTF-8 |
| `DOCS_PARSE_NOT_READY` | 解析完成后可查看 | Available after parsing | — |
| `DOCS_EXTRACT_MODEL_NOT_CONFIGURED` | 未配置模型，只用规则 | No model; rules only | 配置模型 |
| `DOCS_EXTRACT_LLM_UNAVAILABLE` | 模型暂不可达 | The model is unreachable | 稍后重试 |
| `DOCS_EXTRACT_LLM_LIMIT_FILE` / `_DRIVE` | 模型调用已达上限，其余部分只用规则 | Model limit reached; rules only for the rest | 调整上限 |
| `DOCS_EXTRACT_PARTIAL` | 部分内容抽取失败 | Some parts failed to extract | 重试 |
| `DOCS_REVIEW_STALE_REVISION` | 内容已变化，请再看一次 | Changed since loaded | — |
| `DOCS_REVIEW_ALREADY_DECIDED` | 已被处理 | Already handled | — |
| `DOCS_PACK_NOTHING_VISIBLE` | 范围内还没有已发布的内容 | Nothing published in this scope yet | 查看处理进度 |
| `DOCS_PACK_SCOPE_INVALID` | 有路径不在 Drive 里 | Some paths aren't in the Drive | 修改范围 |
| `DOCS_PACK_BUDGET_TOO_SMALL` | 预算太小，至少 {min_budget} | Budget too small; at least {min_budget} | 调大预算 |
| `DOCS_PACK_TASK_TOO_LONG` | 任务描述太长（最多 {max_chars} 字） | Task description too long (max {max_chars}) | — |
| `DOCS_PACK_LEAK_GUARD` | 编译被安全检查拦下（这是缺陷，请报告） | Blocked by the safety check (a bug; please report) | 复制诊断信息 |
| `DOCS_PACK_PROVENANCE_GAP` | 有内容缺少出处，已停止（这是缺陷，请报告） | Some lines lack sources (a bug; please report) | 复制诊断信息 |
| `DOCS_SHARE_TARGET_OCCUPIED` | 这个文件夹里已有不是本引擎创建的 .context | The folder already has a .context not created here | 换一个文件夹 |
| `DOCS_SHARE_TARGET_INVALID` | 不能挂载到这个位置（{reason}） | Can't mount here ({reason}) | 换一个文件夹 |
| `DOCS_SHARE_TARGET_WRITE_FAILED` | 写入失败，已回滚 | Write failed and was rolled back | 重试 |
| `DOCS_SHARE_ROLE_UNAVAILABLE` | 可编辑角色在后续版本开放 | The editor role isn't available yet | — |
| `DOCS_SHARE_INVALID_TRANSITION` | 这个共享已结束，不能再操作 | This share has ended | 再共享 |
| `DOCS_SHARE_READ_CONTENT_IRREVOCABLE` | Agent 已读内容无法收回 | Content the agent already read can't be taken back | — |
| `DOCS_ENGINE_UNAVAILABLE` | 引擎已断开，正在恢复 | Engine disconnected, recovering | — |
| `DOCS_ENGINE_STARTING` | 引擎正在启动 | The engine is starting | 稍候 |
| `DOCS_ENGINE_DB_UNAVAILABLE` | 数据库暂不可用 | Database unavailable | 稍后重试 |
| `DOCS_ENGINE_DISK_FULL` | 磁盘已满，引擎只读 | Disk full; read-only | 清理磁盘 |
| `DOCS_ENGINE_PRECONDITION_FAILED` | 已在其他地方被修改，已载入最新值 | Changed elsewhere; latest loaded | — |
| `DOCS_ENGINE_RATE_LIMITED` | 操作太频繁，稍后再试 | Too many requests; try again shortly | 自动重试 |
| `DOCS_ENGINE_SCHEMA_MISMATCH` | 引擎数据版本不兼容 | Engine data version mismatch | 升级 oo |
| `DOCS_ENGINE_INTERNAL` | 引擎内部错误（{request_id}） | Internal engine error ({request_id}) | 重试；复制诊断信息 |
| `SO_DOCKER_MISSING` / `SO_DOCKER_STOPPED` / `SO_OO_MISSING` / `SO_PLATFORM_UNSUPPORTED` / `SO_ENGINE_INCOMPATIBLE` | 同 §4.4 | 同 §4.4 | 同 §4.4 |
| `SO_SERVE_PORT_IN_USE` | 端口被占用，已换端口仍失败 | Port in use | 重试 |
| `SO_SERVE_IMAGE_INVALID` | 引擎镜像缺失或校验失败 | Engine images missing or invalid | 查看日志 |
| `SO_SERVE_STATE_DIR_INVALID` / `SO_SERVE_CREDENTIALS_INVALID` | 引擎状态目录权限异常 | Engine state folder has wrong permissions | 打开状态目录 |
| `SO_SERVE_START_TIMEOUT` | 引擎启动超时 | The engine took too long to start | 重试；查看日志 |
| `SO_TIMEOUT`（写操作） | 结果待确认 | Result not confirmed yet | 重新查询（同一操作） |
| `SO_UPLOAD_SOURCE_MISSING` | 找不到源文件 | Source file not found | — |
| `SO_UPLOAD_UNSUPPORTED_BACKEND` | 这个 Drive 不支持从 App 上传 | Uploading isn't supported for this Drive | — |
| `SO_PREVIEW_UNAVAILABLE` | 这个 Drive 的文件暂不能在 App 内预览 | Preview isn't available for this Drive | — |

---

## 5. i18n

### 5.1 namespace 与文件

| 项 | 值 |
|---|---|
| 新 namespace | `super-drive`（消息 ID = `super-drive.<key>`，`packages/localization/src/core/catalog.ts:5-9` 用文件名作前缀） |
| 文件 | `locales/renderer/en/super-drive.json`、`es/`、`pt-BR/`、`fr/`、`de/`、`ja/`、`ko/`、`zh-Hans/`（顺序与 `locales/manifest.json` 一致，`scripts/check-localization.mjs:28`） |
| 装载 | `src/localization/catalog-loaders/{en,es,pt-BR,fr,de,ja,ko,zh-Hans}.ts` 各加 `import superDrive from "../../../locales/renderer/<locale>/super-drive.json";` 与 `"super-drive": superDrive` |
| 原生文案 | `locales/native/<locale>.json` 加 `native.superDrive.*`（原生选择框标题；主进程不返回其他人话文案，只返回码） |
| 实验开关文案 | `locales/renderer/<locale>/settings.json` 加 `experimental.superDrive.title/description`（随 `ExperimentalSettingsView.tsx` 现有约定，**未确认**确切 key 前缀，F-S1-02 以该文件为准） |
| 不改 | `locales/manifest.json`（主线程独占且无需改） |

`check-localization.mjs` 的约束（实现时必须满足）：8 种语言 key 集合完全一致；每条消息是合法 ICU 且各语言占位符集合相同；禁止富文本标签；`t("super-drive.xxx")` 的字面量必须存在；模板字面量 `t(\`super-drive.file.state.${state}\`)` 至少匹配一条；`.tsx` 里禁止硬编码可见文字与 `aria-label/placeholder/title/alt` 字面量（允许列表见该脚本第 13-25 行，`PDF`、`CLI` 等）。

### 5.2 key 命名规则

```
<区域>.<对象>.<属性或变体>[.<子项>]
区域：navigation | toolbar | file | folder | list | grid | queue | upload | banner | details | blocks | entity
      | review | share | engine | settings | onboarding | errors | a11y | common
```

- 区域与对象用 camelCase；**枚举值保持契约原样**（snake_case），例如 `file.state.parse_failed`、`review.kind.low_confidence_fact`、`engine.component.model.not_configured`，这样可以用模板字面量按枚举取 key，并让“每个枚举值都有 key”可以机械检查。
- 错误码保持原样：`errors.DOCS_PARSE_CORRUPT`、`errors.SO_DOCKER_MISSING`；族回退 `errors.family.PARSE`。10 §8.4 末段建议 `errors.v1.json` 的 `i18n_key` 写作 `superOntology.errors.<code>`，与本仓库的消息 ID 规则不符，见 §8.2 Q13。
- 可访问名称专用文案放 `a11y.*`（例如 `a11y.dropZone`、`a11y.fileRow`），不复用可见标签，便于在两者不同时独立翻译。
- 同一含义只有一条 key；按钮与菜单项共享 `common.action.*`（如 `common.action.retry`）。
- 品牌与协议词不翻译：`Agent`、`BRIEF`、`Docker`、`oo`、`MCP`、`S3`（`.tsx` 里出现时必须来自消息，或加入允许列表——加入允许列表需主线程同意）。

### 5.3 ICU 示例

```json
{
  "navigation.label": "Super Drive",
  "file.state.parsing": "Parsing · {unit, select, page {page {done, number} of {total, number}} sheet {sheet {done, number} of {total, number}} line {line {done, number}} batch {batch {done, number} of {total, number}} other {{done, number}}}",
  "file.state.publishedWithReview": "Published · {count, plural, one {# to review} other {# to review}}",
  "queue.summary": "{uploading, plural, =0 {} one {# uploading · } other {# uploading · }}{processing, plural, one {# processing} other {# processing}}{failed, plural, =0 {} one { · # failed} other { · # failed}}",
  "banner.partialFailure.title": "{count, plural, one {# file failed} other {# files failed}}",
  "review.group.acceptAll": "Accept all {count, number}",
  "share.role": "{role, select, viewer {Read only} suggester {Can suggest} editor {Can edit} other {Unknown role}}",
  "share.expiresIn": "Expires {relative}",
  "share.revoke.irrevocable": "Content the agent has already read can't be taken back.",
  "engine.status.restarting": "Engine stopped unexpectedly; restarting (attempt {attempt, number}, in {seconds, number}s)",
  "errors.DOCS_PACK_BUDGET_TOO_SMALL": "Budget too small; at least {min_budget, number} tokens",
  "a11y.fileRow": "{name}, {state}"
}
```

```json
{
  "navigation.label": "Super Drive",
  "file.state.parsing": "解析中 · {unit, select, page {第 {done, number}/{total, number} 页} sheet {第 {done, number}/{total, number} 个工作表} line {第 {done, number} 行} batch {第 {done, number}/{total, number} 批} other {{done, number}}}",
  "file.state.publishedWithReview": "已发布 · {count, plural, other {# 条待确认}}",
  "queue.summary": "{uploading, plural, =0 {} other {上传 # · }}{processing, plural, other {处理中 #}}{failed, plural, =0 {} other { · 失败 #}}",
  "banner.partialFailure.title": "{count, plural, other {# 个文件处理失败}}",
  "review.group.acceptAll": "整体接受 {count, number} 条",
  "share.role": "{role, select, viewer {只读} suggester {可建议} editor {可编辑} other {未知角色}}",
  "share.expiresIn": "{relative}到期",
  "share.revoke.irrevocable": "Agent 已读内容无法收回。",
  "engine.status.restarting": "引擎意外退出，正在重启（第 {attempt, number} 次，{seconds, number} 秒后）",
  "errors.DOCS_PACK_BUDGET_TOO_SMALL": "预算太小，至少 {min_budget, number} token",
  "a11y.fileRow": "{name}，{state}"
}
```

要点：数字一律 `{x, number}` 或 `formatNumber`；相对时间由 `formatRelativeTime` 生成后作为 `{relative}` 传入；文件名经 `bidiIsolate` 后作为变量传入，不拼接到 key 外；德语与法语的长文案在工具栏、徽标、按钮上要做截断检查（P16）。

---

## 6. 测试方案

### 6.1 分层总览

| 层 | 位置 | 运行 | 后端 | 说明 |
|---|---|---|---|---|
| unit | `tests/unit/super-drive/<capability>/*.test.ts` | `npm run test:unit` / `npx vitest run tests/unit/super-drive` | 无 | 纯函数与 store |
| component | `tests/component/super-drive/<capability>/*.test.tsx`（`/** @vitest-environment happy-dom */`，`createRoot` + `withTestLocalization`，同 `tests/component/model-connections/settings/modelConnectionsSettings.test.tsx`） | `npm run test:component` | mock 桥（生成） | 渲染、交互、无障碍 |
| integration | `tests/integration/super-drive/<capability>/*.test.mjs` | `npm run test:integration` | 契约 mock 服务器（schema 生成 + ajv 校验） | 主进程服务、IPC、SSE |
| architecture | `tests/architecture/super-drive/<capability>/*.test.ts(x)` | `npm run test:architecture` | 无 | 边界、token、覆盖率 |
| contract | 同 integration，`contract/` capability | 同上 | 无 | 生成枚举 ↔ 状态矩阵、错误码 ↔ i18n、mock ↔ schema |
| T1 | `tests/e2e/super-drive/<capability>/*.smoke.mjs` | `electron <file>`（`--native` 用 CGEvent），注册进 `run-desktop-e2e.mjs` | **真实 `oo serve`** | 60 §3.1 |
| T2 | `tests/e2e/super-drive/packaged/`（路径见 §8.2 Q9） | 本地门禁脚本 | 真实 | 60 §3.2，Peekaboo 操作 + CDP 断言 |
| 视觉 | T1 内 `capturePage()` 截图矩阵 | 同 T1 | 真实或 mock | 仓库现有做法是截图写入证据目录并断言计算样式（`tests/integration/appearance/themes/appearance-visual-matrix.smoke.mjs:540-544`），**没有像素基线比对**；本分册沿用，不引入新库 |
| 性能 | `tests/performance/benchmarks/super-drive/<capability>/*.bench.ts` | `npm run bench:performance` | 无 | 1 万文件归并与渲染 |

测试域：`tests/config/discovery.mjs` 的 `testDomains` 需要加 `super-drive`（`scripts/check-test-layout.mjs` 会拒绝未知域，§1.3）。

### 6.2 unit

| 文件 | 覆盖 |
|---|---|
| `events/applyEvents.test.ts` | 按 `seq` 去重与乱序丢弃；`file.state_changed` 插入、更新、计数修正；`file.renamed`、`file.removed`；`files.bulk_changed` 标 stale；`review.changed`、`share.*`（含 `refreshed_from` 链）、`usage.changed`；瞬时事件不推进游标 |
| `events/resync.test.ts` | 410 / 缺口 / 引擎实例变化 → 全量重拉顺序与新游标取最大 `as_of_seq`；进行中的上传与乐观标记保留 |
| `store/optimistic.test.ts` | 重试与审核的乐观、确认、逐项回滚；PATCH 412 回滚 |
| `store/folderPages.test.ts` | 键集分页、`as_of_seq` 对齐、骨架区间、1 万条 |
| `state/stateModel.test.ts` | `FILE_STATE_PRESENTATION` 覆盖生成的 `FileState` 全部值 + `uploading` + `unknown`；`SHARE_STATE_PRESENTATION`、`REVIEW_KIND_PRESENTATION`、`BlockedReason`、`ComponentStatus` 同理 |
| `errors/errorMapping.test.ts` | 每个 `errors.v1.json` 的码映射到存在的 key；族回退；`SO_*` 覆盖 |
| `share/mountCommand.test.ts` | 依 10 §7.9 生成 argv；POSIX 与 Windows 转义；含空格、中文、引号的任务描述；与 `Share.command` 样例逐字一致 |
| `banners/bannerModel.test.ts` | §4.7 的优先级与“最多 2 条” |
| `main/requests.test.mjs` | `parseSuperOntologyRequest` 对每个通道的合法与非法输入（`../`、超长、非 UUID、非绝对路径） |
| `main/engineStateMachine.test.mjs` | §1.4.1 状态机：每个退出码、每个 health 状态、退避与 10 分钟 3 次规则 |

### 6.3 component 与 integration

| 文件 | 覆盖 |
|---|---|
| component `browser/fileTable.test.tsx` | 列表渲染全部文件状态（以 `superDriveStateModel` 的 fixture 驱动）；排序、筛选标签；多选与键盘（§3.2.4）；右键菜单项随状态变化；`aria-rowcount/rowindex/selected` |
| component `browser/dropZone.test.tsx` | 拖拽进入/离开/放下；s3 Drive 禁用原因；键盘等价按钮 |
| component `queue/queuePanel.test.tsx` | 汇总数字与 `aria-live`；折叠规则；上传条目取消/重试；交接到 `stored` |
| component `banners/banners.test.tsx` | §4.6 每个页面级状态出现对应横幅与动作 |
| component `details/detailsPane.test.tsx` | 概览、失败原因卡、解析结果分页、证据片段高亮、实体推入与返回、已撤回事实开关 |
| component `review/reviewQueue.test.tsx` | 按来源分组、整体接受只发一次 `review-decide`（`by_source` + `as_of_seq`）；行内决定无弹窗；逐项结果五种状态；键盘 J/K/A/D/X |
| component `share/shareDialog.test.tsx` | 三步流程；默认角色 suggester、有效期 7 天、预算 800；editor 禁用；预览防抖；每个 `DOCS_PACK_*`、`DOCS_SHARE_*` 错误呈现；一键执行时不可关闭；命令复制内容 |
| component `share/shareList.test.tsx` | 五种状态 + 挂载缺失；刷新链；撤销确认框包含“已读内容无法收回”；`local_removed=false` 提示 |
| component `engine/enginePage.test.tsx` | §4.4 每个状态的整页与胶囊呈现、主操作；组件表 7 种状态 |
| component `onboarding/firstRun.test.tsx` | 首次引导三步、引擎未就绪时第 2 步禁用 |
| integration `main/controlClient.test.mjs` | 对契约 mock 服务器：Bearer、`OO-Docs-Api-Min`、Idempotency-Key（同键重放、`OO-Idempotent-Replay`）、If-Match/412、切批 ≤ 500、429 退避、超时、错误归一与 `message` 丢弃、token 不出现在任何返回值与日志 |
| integration `main/eventBridge.test.mjs` | SSE 解析（`id/event/data`、`: ping`、`retry:`）；断线退避与 `since`；410 → `resync-required`；`stream.lagging` 后重连；环形缓冲 `events-since` 与 `SO_EVENTS_GAP`；50 ms 合并 |
| integration `main/engineLifecycle.test.mjs` | 假 `docker`/`oo` 可执行文件（脚本）模拟：未安装、daemon 停止、`oo serve` 各退出码、复用已有实例、`serve.json` 与 token 权限校验失败、退出时 SIGTERM/`--keep-engine` |
| integration `main/ipcHandlers.test.mjs` | 通过 `createTrustedIpcMain` 的假 `ipcMain` 注册全部通道；非主框架被拒；信封形状；`details` 白名单 |
| integration `main/uploadService.test.mjs` | 临时名 + 原子改名、重名、隐藏文件跳过、路径穿越拒绝、取消、磁盘满（模拟 `ENOSPC`）、触发 `:sync` |
| integration `contract/contract.test.mjs` | `contracts/docs-api/SOURCE.json` 的版本与生成代码一致；mock 的每个响应都通过 ajv 校验；生成的 `FileState`/`ShareState`/`ReviewKind` 与 `superDriveStateModel` 逐项相等（L3 横切门禁“状态机契约测试”的 Puppyone 半边，10 §12.4） |

### 6.4 architecture（边界）

`npm run check:boundaries` 现有 30 多个检查全部适用（`package.json` 中的串联）；新代码必须满足的现有规则摘录：

| 来源 | 规则 |
|---|---|
| `check-data-ui-boundaries.mjs` | `src/` 不得引用 `@/`、`next`、`@supabase/*`、`swr`、`@puppyone/data-ui` 等；不得在 `src/styles.css` 里写 `ExplorerTree` 的内部选择器（`.tree-row` 等），只能覆盖 `--po-tree-*` 变量 |
| `check-repository-layout.mjs` | `src/` 不得深引用 `packages/shared-ui/src`，只能 `@puppyone/shared-ui` |
| `check-renderer-style-architecture.mjs` | 样式必须在命名 cascade layer 内；不得声明 `app-region`；`shared-ui` 不得出现 `data-interface-style`；不得新增全局 Tailwind 入口 |
| `check-sidebar-architecture.mjs` | 文件名含 `Sidebar` 的 `.tsx`（例如 `DriveSidebar.tsx`）不得有 `<style>` 与字面量 `style={{…}}`（只能用 CSS 或运行时自定义属性变量）；不得深引用其他 feature 的 `sidebar/sections/rows/hooks/styles` 目录；注册表保留 `data/git/cloud/settings` |
| `check-cloud-frontend-architecture.mjs` | 若改动 `cloud/share`：组件 CSS 在 `@layer features { … }` 内、Cloud CSS 类必须有渲染层使用者 |
| `check-localization.mjs` | §5.1 |
| `check-desktop-platform-architecture.mjs` | `electron/main.mjs` 不得出现 `process.platform`；渲染层不得用 `navigator.platform` |
| `check-native-surface-layering.mjs` | 原生菜单面与对话框必须持有遮挡租约（用 `DesktopMenuSurface`、`DesktopDialogRoot`、`DesktopOverlayLayer` 即满足） |
| `check-test-layout.mjs` | 测试在 `tests/<layer>/<domain>/<capability>/`；域必须登记 |
| `check-test-fixture-privacy.mjs` | 夹具不得含宿主绝对路径与真实账号 |
| `check-product-reference-hygiene.mjs` | 不得引用外部产品名作为自有标识 |
| `check-window-layout-architecture.mjs` | 不得覆盖 640px 最小宽度合同 |

新增的 Super Drive 架构测试（`tests/architecture/super-drive/`）：

| 测试 | 规则 |
|---|---|
| `boundaries/bridgeIsolation.test.ts` | 只有 `superDriveApi.ts` 出现 `puppyoneDesktop`；`src/features/super-drive/**` 不引用 `src/lib/cloudApi.ts` 的请求函数（类型除外）；不引用 `data-workspace` |
| `boundaries/dataWorkspaceUntouched.test.ts` | `git diff --quiet <base> -- src/features/data-workspace packages/shared-ui/src/data src/features/app-shell/DesktopDataWorkspaceSurface.tsx`（`<base>` 取集成分支创建点，记录在 `tests/support/super-drive/baseline.json`） |
| `boundaries/noSecrets.test.ts` | 渲染层与 preload 源码不得出现 `oodt_`、`oodr_`、`serve.token`；主进程快照与信封序列化后不含这些子串 |
| `styles/tokensOnly.test.ts` | `src/features/super-drive/**/*.{css,tsx}` 不得出现十六进制颜色、`rgb(`、`hsl(`、命名颜色；除 `super-drive-tokens.css` 外不得出现 `\d+px`、`\d+ms`、`\d+s` 字面量；所有 CSS 在 `@layer features` 内 |
| `structure/fileBudgets.test.ts` | `.tsx` ≤ 400 行、`.css` ≤ 450 行 |
| `accessibility/accessibleNames.test.tsx` | A1/A5：渲染每个页面全部 fixture 状态，可交互元素可访问名称覆盖率 100%，8 种语言下非空 |
| `accessibility/testIdUsage.test.ts` | A4：T2 场景的操作步骤不得用 `data-testid`；`data-testid` 命名 `sd-*` |
| `flags/flagOffParity.test.ts` | flag 关闭时：`getAvailableWorkspaceSurfaces` 结果、导航项、`ExperimentalSettingsView` 列表与基线一致；preload 不暴露 `superOntology`（读 `preload.cjs` 源码结构） |

### 6.5 T1：仓库内场景（60 §3.1）

| 文件 | 场景 | `--native` 必做的真实点击 | 闭环 |
|---|---|---|---|
| `tests/e2e/super-drive/flags/flag-off-regression.smoke.mjs` | flag 关：无 Super Drive 导航、实验开关不出现；现有 `project-sidebar-stability` 行为不变 | — | L1 |
| `tests/e2e/super-drive/skeleton/walking-skeleton.smoke.mjs` | 启动引擎 → 登记 20 个 md 的文件夹 → 全部 `published`（≤ 60 s）→ 关掉 `oo serve` → “引擎已断开” → 自动重连 → `since` 补齐不丢状态 → 修改一个 md → 30 s 内重走状态 | 打开 Super Drive、点“启动引擎” | L2 |
| `tests/e2e/super-drive/processing/formats-and-failures.smoke.mjs` | 拖入评测集各格式 + 不支持/超大/加密/毒丸；每个文件进入正确状态；失败横幅 → 只看失败 → 全选 → 批量重试 | **原生拖拽上传**（参考 `tests/support/electron/native-resource-acceptance.mjs`）、**多选与右键菜单**、**横幅 → 筛选 → 批量重试** | L3 |
| `tests/e2e/super-drive/processing/parser-outage.smoke.mjs` | 处理中停掉 parser 容器 → `degraded` 与“等待中：解析服务不可用” → 恢复后自动继续 | 一次原生点击（引擎页） | L3 |
| `tests/e2e/super-drive/processing/details-anchor.smoke.mjs` | 打开文件 → 解析结果 → 点锚点 → 片段高亮（PDF 翻页如已支持） | 一次原生点击 | L3 |
| `tests/e2e/super-drive/sync/s3-incremental.smoke.mjs` | RustFS 桶登记 → 新增/修改/重命名/删除 → 状态与证据正确 | 一次原生点击 | L3 |
| `tests/e2e/super-drive/review/accept-by-source.smoke.mjs` | 配测试模型与不配模型各一遍；待确认按来源整体接受 100 条 = 1 次操作；5 s 内新版本 | **按来源整体接受** | L4 |
| `tests/e2e/super-drive/share/mount-and-revoke.smoke.mjs` | 右键目录 → 共享 → 选范围 → 默认角色 → 改预算 → 填任务 → 一键执行 → 目标目录 `.context/` 与 CLI 编译逐字节一致 → BRIEF 每行有出处 → 红队文件金丝雀 0 命中 → 撤销确认 → 目录删除、状态 `revoked` | **选范围、选角色、改预算、一键执行、撤销确认** | L4 |
| `tests/e2e/super-drive/suggest/revision-mode.smoke.mjs` | Agent 写 `suggest/` → 修订模式批量接受 → 下一次 BRIEF 变化 | 一次原生点击 | L5 |
| `tests/e2e/super-drive/drift/refresh.smoke.mjs` | 修改被引用文件 → `drifted` → 刷新 → 新 Share `refreshed_from` 链 → BRIEF 头部差异 | 一次原生点击 | L6 |
| `tests/e2e/super-drive/enterprise/buckets.smoke.mjs` | oss/cos/r2 登记与增删改 | 一次原生点击 | L7 |
| `tests/e2e/super-drive/scale/ten-thousand.smoke.mjs` | 1 万文件 Drive：首屏 < 1 s、滚动无掉帧（`requestAnimationFrame` 帧间隔统计）、免 Docker 路径 | 一次原生点击 | L8 |

前置：`tests/support/super-drive/real-engine.mjs` 用独立 `--state-dir` 拉起 `oo serve`，容器打 `oo.lane=super-ontology-e2e`，结束全部删除；测试 Drive 复制自 OO 的 `testing/super-ontology/golden/`（路径由环境变量 `SUPER_ONTOLOGY_GOLDEN` 指定，不进 Puppyone 仓库，满足 `check-test-fixture-privacy.mjs`）；Desktop 以 `--puppyone-user-data-dir=<临时目录>` 启动。

### 6.6 T2：打包 App 黑盒（60 §3.2）

| 资产 | 路径（建议，见 §8.2 Q9） | 内容 |
|---|---|---|
| 运行脚本 | `tests/e2e/super-drive/packaged/run-packaged.mjs` | 校验 `.app` sha256 → 以 §1.10.2 的命令启动（独立用户数据目录 + `--remote-debugging-port=0`）→ 从 DevToolsActivePort 读端口 → `chromium.connectOverCDP` → 逐个跑 Peekaboo 场景 → 每步截图、整轮录屏 → 失败时收集 Peekaboo 快照、渲染 console、主进程日志、`oo serve` 日志 |
| 场景 | `tests/e2e/super-drive/packaged/scenarios/P01-no-docker.peekaboo.json` … `P18-drift-refresh.peekaboo.json` | 只用可访问名称定位（A4）；名称取英文与中文两套（P16 切英文） |
| CDP 断言 | `tests/e2e/super-drive/packaged/assertions/*.mjs` | 只读：`data-testid`、`data-file-state`、store 快照（经 `window.__superDriveTestProbe`，仅 `acceptance`/`dev` 渠道注入） |
| 证据 | `$RUN/evidence/`（不进仓库），按 60 §3.4 汇总进验收包 |

P1–P18 与前端功能的对应和负责的卡见 §7（每个闭环最后一个 Sprint 的 T 卡）。Playwright 的引入（`playwright-core` 仅 `connectOverCDP`）属于新增开发依赖，需要主线程改 `package.json`（§8.2 Q14）；若不允许新依赖，用 Electron 自带的 `chrome-remote-interface` 等价实现**未确认**（仓库无该依赖），备选是直接用 Node 的 WebSocket 讲 CDP 协议。

### 6.7 每个闭环的前端验收清单

| 闭环 | 前端验收项（全部要有证据：命令输出、截图或录屏） |
|---|---|
| L1 | flag 关：`npm test`、`test:typecheck`、`check:boundaries` 全绿，`flagOffParity` 通过，T1 `flag-off-regression` 通过；flag 开 + 实验开关开：侧栏出现 Super Drive，页面显示“引擎未连接”空态；`super-drive.json` 8 种语言齐全；契约 `1.0-draft` 已复制并生成类型 |
| L2 | 引擎生命周期四种 preflight 失败态与启动/健康/断开/重连呈现正确；登记 fs Drive；列表状态实时更新到 `published`；断线后 `since` 补齐、410 全量重拉（用测试钩子把 since 设到保留下限之前）；A1–A5 通过；T1 `walking-skeleton` 与 T2 P2–P4（只 md）、P8、P11（命令复制路径，可选一键执行）通过 |
| L3 | §4.1 每个文件状态都有呈现（状态矩阵逐项截图）；上传队列、失败横幅、筛选、批量重试、详情栏解析结果与锚点；s3 Drive 登记；1 万文件虚拟滚动与骨架；磁盘与权限状态；契约测试（枚举逐项相等）通过；T1 L3 场景 + T2 P1–P8、P10、P15、P16 通过；T3 无阻断问题 |
| L4 | 待确认队列（按来源整体接受 1 次操作）；实体详情与证据跳转；未配置模型横幅与用量；分享面板（默认 suggester、7 天、预算、任务、命令、一键执行）；共享列表五种状态与刷新链；撤销文案；BRIEF 预览 `[src:…]` 可点；T1 L4 场景 + T2 P1–P16 通过；T4 验收包交付 |
| L5 | `suggestion` 种类与当日摘要；修订模式批量接受；`editor` 角色可选；T2 + P17 |
| L6 | OCR 低置信区域标记；类型升格卡片；`reprocess_diff`；漂移提示、刷新前差异预览；“可更新”批量处理；T2 + P18；T4 验收包 |
| L7 | oss/cos/r2 登记；Cloud 代理模式切换（不放宽 SSRF 白名单）；审计视图；远端 Worker 挂载命令 |
| L8 | 1 万文件首屏 < 1 s、滚动无掉帧；免 Docker 运行时的 preflight 与引导（从安装到第一份 BRIEF ≤ 10 分钟，owner 不开终端）；T2 全量；T4 验收包 |

---

## 7. 前端任务卡

> 每张卡所属的并行板块（WS-*）、目录所有权和排期见 [70 分册](70-parallel-workstreams.md) §2、§5；与本分册协议对应的形式化模型、Kani 证明和 `V-S*` 卡见 [80 分册](80-formal-verification.md) §3、§7。实现被形式化模型覆盖的代码时，提交带 `Formal:` trailer，并在同一个 PR 里同步更新模型。

### 7.0 约定（50 分册）

| 项 | 规则 |
|---|---|
| 卡号 | `F-S<n>-<nn>`（两位序号）；提交 trailer `Plan: SO-F-S<n>-<nn>`，一个提交只写一个 `Plan:` |
| 分支 | `codex/super-drive-F-S<n>-<nn>-<短描述>`，从集成分支 `codex/super-drive` 拉出；卡头的英文短描述即分支后缀 |
| 提交 | Conventional Commits，scope 只用 `super-drive`（渲染层）/ `super-drive-main`（主进程服务与 IPC）/ `super-drive-i18n` / `super-drive-test`；一卡通常 2–6 个提交（类型与接口 → 实现 → 测试 → 接入界面） |
| 跨仓 | 依赖 OO 提交时写 `Requires: open-ontology@<短 sha>` 与 `Contract-Version: docs-api/1.x`（S1 为 `1.0-draft`，S2 会合后 `1.0`，S7 起 `1.1`） |
| 身份 | 只用 owner 身份，不加 `Co-Authored-By` 或任何 agent 署名 |
| 主线程独占 | `package.json`、`package-lock.json`、`electron/main.mjs` 的服务组装、trusted-ipc 注册表、`workspaceSurfaceRegistry.ts`、`flags.json`、`locales/manifest.json`。卡里标 **[主线程]** 的子项，执行者在报告里写明需求（精确到要加的行），由主线程统一提交 |
| 建议增列为主线程独占 | `tests/config/discovery.mjs`、`tests/support/runners/run-desktop-e2e.mjs`、`scripts/release-checks/checks.json`、`shared/desktop/application-identity.mjs`、`electron/main/build-info-service.mjs`（§8.2 Q8、Q10） |
| 无障碍验收 | 从 F-S2 起，凡含界面的卡都必须满足 §2.6 的 **A1–A5**（卡里写“A1–A5”即指这五条全部适用） |
| 通用验证 | 每张卡都要跑：`npm run lint`、`npm run test:typecheck`、`npm run check:boundaries`、本卡相关的 `npx vitest run <目录>`；含文案的卡再跑 `node scripts/check-localization.mjs`；含契约的卡再跑 `node scripts/generate-super-drive-contract.mjs --check` |

### 7.1 M0 · L1 规格与评测基线（S1）

**F-S1-01 · flags** — scope `super-drive`
- 目标：两级开关（§1.10.1）就位，默认全关。
- 文件：`src/features/flags/registry.ts`（`superDrive: false`）、`src/preferences.ts`（`enableSuperDrive`，解析与默认值）、`src/features/settings/main/ExperimentalSettingsView.tsx`（flag 为真时显示开关）、`locales/renderer/*/settings.json`（8 种语言）。**[主线程]** `src/features/flags/flags.json` 加 `"superDrive": false`；`package.json` 加 `puppyoneCapabilities.superDrive: false`。
- 依赖：无。
- 完成定义：flag 关时设置页无新开关；flag 开时出现“Super Drive（实验）”开关，默认关；偏好持久化与旧数据兼容（缺字段 = false）。
- 验证：`npx vitest run tests/unit/settings tests/component/settings`；新增 `tests/unit/super-drive/flags/experimentalSetting.test.ts`。

**F-S1-02 · surface-shell** — scope `super-drive`
- 目标：注册 `super-drive` surface 与侧栏导航项；页面是空壳，显示“引擎未连接”。
- 文件：`workspaceSurfaceTypes.ts`、`useWorkspaceSurfaceContent.tsx`、`navigation/navigationModel.tsx`、`navigation/types.ts`、`src/features/super-drive/{index.ts,SuperDriveSurface.tsx,super-drive.css,super-drive-tokens.css,onboarding/SuperDriveEntryState.tsx,onboarding/onboarding.css}`。**[主线程]** `workspaceSurfaceRegistry.ts` 增加条目（§1.3 给出完整代码）。
- 依赖：F-S1-01、F-S1-04。
- 完成定义：flag 与实验开关都开 → 侧栏出现 Super Drive，点击进入显示 `SuperDriveEntryState`（文案 `engine.status.idle`“引擎未连接”）；切回“数据”时数据工作区状态不丢；`check-sidebar-architecture.mjs` 通过。
- 验证：`npx vitest run tests/component/super-drive/shell`；`npm run check:boundaries`。

**F-S1-03 · main-skeleton** — scope `super-drive-main`
- 目标：主进程服务与 IPC 骨架，`engine-read` 返回固定 `{status:"idle", connection:{state:"offline"}}`；preload 暴露 `superOntology`（仅能力开启时）。
- 文件：`electron/main/super-ontology/{service.mjs,snapshot.mjs}`、`electron/main/ipc/super-ontology-ipc.mjs`、`shared/super-ontology/{requests.mjs,requests.d.mts,errors.mjs,bridge.d.ts}`、`electron/preload.cjs`、`src/types/electron.d.ts`、`src/features/super-drive/superDriveApi.ts`。**[主线程]** `electron/main.mjs`：创建服务、`registerSuperOntologyIpcHandlers({ ipcMain: trustedIpcMain, service })`、能力开启时给 preload 追加 `--puppyone-super-drive=1`。
- 依赖：F-S1-01。
- 完成定义：能力关时 `window.puppyoneDesktop.superOntology === undefined` 且主进程未注册任何 `super-ontology:*` 通道；能力开时 `engine-read` 走通信封；非主框架调用被 trusted-ipc 拒绝。
- 验证：`npx vitest run tests/integration/super-drive/main/ipcHandlers.test.mjs`。

**F-S1-04 · i18n-namespace** — scope `super-drive-i18n`
- 目标：建 `super-drive` namespace 与装载；先放 S1–S2 需要的 key（导航、空态、引擎状态、`errors.family.*`、`errors.unknown`）。
- 文件：`locales/renderer/*/super-drive.json`（8 个）、`src/localization/catalog-loaders/*.ts`（8 个）、`locales/native/*.json`（`native.superDrive.*`）。
- 依赖：无。
- 完成定义：`check-localization.mjs` 通过；中文、英文人工校对，其余 6 种语言由译者或模型初译并标注待复核（在 PR 描述列出）。
- 验证：`node scripts/check-localization.mjs`。

**F-S1-05 · contract-vendor** — scope `super-drive-test`
- 目标：复制 OO 的 `contracts/docs-api/*.json`（`1.0-draft`）并生成类型、枚举、守卫与 mock 夹具。
- 文件：`contracts/docs-api/{docs-api.v1.schema.json,errors.v1.json,events.v1.schema.json,SOURCE.json}`、`scripts/generate-super-drive-contract.mjs`、`src/features/super-drive/contract/docsApi.generated.ts`、`tests/support/super-drive/contract-mock.mjs`、`tests/integration/super-drive/contract/contract.test.mjs`。**[主线程]** `package.json` 加 `check:super-drive-contract` 并串入 `check:boundaries`。
- 依赖：**B-S1-01**（schema 冻结）。trailer：`Requires: open-ontology@<B-S1-01 sha>`、`Contract-Version: docs-api/1.0-draft`。
- 完成定义：生成物与 schema 一致（`--check` 通过）；`SOURCE.json` 记录 sha；mock 的每个样例响应过 ajv 校验。
- 验证：`node scripts/generate-super-drive-contract.mjs --check`；`npx vitest run tests/integration/super-drive/contract`。

**F-S1-06 · arch-baseline** — scope `super-drive-test`
- 目标：架构测试基线与 L1 的 T1（flag 关回归）。
- 文件：`tests/architecture/super-drive/{boundaries,styles,structure,flags}/*.test.ts`（§6.4 中除可访问性外的全部）、`tests/support/super-drive/baseline.json`、`tests/e2e/super-drive/flags/flag-off-regression.smoke.mjs`。**[主线程（建议）]** `tests/config/discovery.mjs` 加 `"super-drive"`；`run-desktop-e2e.mjs` 注册该场景。
- 依赖：F-S1-02、F-S1-03。
- 完成定义：flag 关时 `flagOffParity` 与 T1 回归通过；`dataWorkspaceUntouched` 以集成分支创建点为基线通过。
- 验证：`npx vitest run tests/architecture/super-drive`；`npx electron tests/e2e/super-drive/flags/flag-off-regression.smoke.mjs`。

**F-S1-07 · pointer-doc** — scope `super-drive`（仅文档）
- 目标：D7 要求的指针文档。由于 Puppyone `AGENTS.md` 规定当前规格只放 `puppy-issues/document/puppyone-desktop`、`docs/` 只是迁移说明，建议放 `puppy-issues/document/puppyone-desktop/super-drive/README.md`（§8.2 Q2）。
- 依赖：无。
- 完成定义：指针文档列出 OO `specs/super-ontology/` 的路径与本分册的 sha。
- 验证：人工评审。

### 7.2 M0 · L2 行走骨架（S2）

**F-S2-01 · engine-preflight** — scope `super-drive-main`
- 目标：`engine-locator.mjs`、`docker-preflight.mjs`；`engine-preflight` 通道；`pick-path { purpose: "oo-executable" }` 与 `engine-set-executable`。
- 文件：`electron/main/super-ontology/{engine-locator.mjs,docker-preflight.mjs,grants.mjs}`、IPC 与 requests 增补。
- 依赖：F-S1-03。
- 完成定义：`unsupported_platform`、`oo_missing`、`docker_missing`、`docker_stopped`、通过 五种结果可由假可执行文件稳定复现；探测全部有超时；不在 `electron/main.mjs` 里判断平台。
- 验证：`npx vitest run tests/integration/super-drive/main/engineLifecycle.test.mjs -t preflight`。

**F-S2-02 · engine-lifecycle** — scope `super-drive-main`
- 目标：§1.4.1 状态机、§1.4.4 凭证读取、日志文件、退出钩子、“退出后保持引擎运行”。
- 文件：`electron/main/super-ontology/{engine-lifecycle.mjs,serve-credentials.mjs}`。**[主线程]** `electron/main.mjs` 的 `before-quit` 调 `superOntologyService.shutdown()`。
- 依赖：F-S2-01；**B-S2-10**（`oo serve`）。
- 完成定义：复用已有实例、启动、健康、退出码 3/4/5/6/7 映射、崩溃退避与 10 分钟 3 次规则、凭证权限校验失败、SIGTERM 与 `--keep-engine` 全部有集成测试；token 不出现在日志与 IPC。
- 验证：`npx vitest run tests/integration/super-drive/main/engineLifecycle.test.mjs`；`npx vitest run tests/architecture/super-drive/boundaries/noSecrets.test.ts`。

**F-S2-03 · control-client** — scope `super-drive-main`
- 目标：§1.4.2 客户端；接通 `health`、`drives-*`、`drive-sync`、`drive-tree`、`files-list` 通道。
- 文件：`electron/main/super-ontology/control-client.mjs`、`secret-files.mjs`（fs 用不到，先建空壳）、IPC 增补。
- 依赖：F-S2-02；**B-S2-09**。trailer `Contract-Version: docs-api/1.0`。
- 完成定义：Bearer、`OO-Docs-Api-Min`、Idempotency-Key 重放、If-Match/412、429、超时、错误归一、`details` 白名单全部有测试；不经 `cloud:api-request`；不改 `shared/cloudEndpoint.js`。
- 验证：`npx vitest run tests/integration/super-drive/main/controlClient.test.mjs`。

**F-S2-04 · event-bridge** — scope `super-drive-main`
- 目标：§1.4.3 全部规则（含 410、`stream.lagging`、`file.progress` 不补、环形缓冲、`events-since`、50 ms 合并、连接状态推送）。
- 文件：`electron/main/super-ontology/event-bridge.mjs`、IPC 增补、preload `subscribe`。
- 依赖：F-S2-03；**B-S2-09**。
- 完成定义：§6.3 `eventBridge.test.mjs` 全部通过；断开 → 重连 → 补齐 的时序可在测试里断言。
- 验证：`npx vitest run tests/integration/super-drive/main/eventBridge.test.mjs`。

**F-S2-05 · renderer-store** — scope `super-drive`
- 目标：`superDriveApi.ts`（完整）、`superDriveStore.ts`、`superDriveEvents.ts`、`superDriveSelectors.ts`、`superDriveRoute.ts`、`superDriveErrors.ts`、`operationIds.ts`；§1.7 的缓存、游标、断线与全量重拉。复制 `copyText/formatBytes/formatRelativeTime/shellQuote` 的必要部分到 `super-drive/format.ts`，不深引用 `cloud/utils`。
- 依赖：F-S2-04。
- 完成定义：§6.2 中 `applyEvents`、`resync`、`folderPages` 单测通过；1 万条归并基准建立（`tests/performance/benchmarks/super-drive/store/applyEvents.bench.ts`）。
- 验证：`npx vitest run tests/unit/super-drive`；`npm run bench:performance -- tests/performance/benchmarks/super-drive`。

**F-S2-06 · engine-ui-and-onboarding** — scope `super-drive`
- 目标：引擎页、状态胶囊、首次引导、fs Drive 登记对话框、设置页 Drive 列表（最小）。
- 文件：`engine/*`、`onboarding/*`、`settings/{DriveSettingsPage.tsx,DriveRegisterDialog.tsx,settings.css}`；对应 i18n。
- 依赖：F-S2-05。
- 完成定义：§4.4 全部引擎状态有整页与胶囊呈现；首次引导三步；登记 fs Drive 走 `pick-path` → `drive-create`，显示“正在应用 Drive 变更”；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/engine tests/component/super-drive/onboarding`；`npx vitest run tests/architecture/super-drive/accessibility`。

**F-S2-07 · browser-basic** — scope `super-drive`
- 目标：左栏（Drive 切换、分区导航、`ExplorerTree` 文件夹树）、工具栏、面包屑、列表视图（基础排序与目录进入）、`FileStateBadge`（到 `published` 为止的状态 + 未知）、`superDriveDataPort.ts`、`useDriveFolderPages.ts`。
- 依赖：F-S2-05；**B-S2-09**（`tree` 端点）。
- 完成定义：20 个 md 的 Drive 实时从 `stored` 走到 `published`；列表 `role="grid"` 语义完整；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/browser`。

**F-S2-08 · t1-skeleton** — scope `super-drive-test`
- 目标：T1 基础设施与 L2 场景。
- 文件：`tests/support/super-drive/real-engine.mjs`（独立 state-dir、`oo.lane` 标签、清理）、`tests/e2e/super-drive/skeleton/walking-skeleton.smoke.mjs`。**[主线程（建议）]** `run-desktop-e2e.mjs`、`scripts/release-checks/checks.json` 注册。
- 依赖：F-S2-06、F-S2-07；**B-S2-12**。
- 完成定义：30 L2 验收 1、4、5 在 T1 里自动化通过（含 `--native` 原生点击“启动引擎”）；410 路径用测试钩子覆盖。
- 验证：`SUPER_ONTOLOGY_GOLDEN=<路径> npx electron tests/e2e/super-drive/skeleton/walking-skeleton.smoke.mjs --native`。

**F-S2-09 · t2-l2** — scope `super-drive-test`（L2 最后一张 T 卡）
- 目标：T2 打包黑盒基础设施与 L2 场景 P2、P3、P4（只 md）、P8、P11（复制命令路径；一键执行在 S6 前不要求）。
- 文件：`tests/e2e/super-drive/packaged/{run-packaged.mjs,scenarios/P02-start-engine.peekaboo.json,P03-empty-drive.peekaboo.json,P04-register-and-drop.peekaboo.json,P08-relaunch.peekaboo.json,P11-share-command.peekaboo.json,assertions/*.mjs}`。**[主线程]** `package.json` 增加 `test:super-drive:packaged` 与 CDP 客户端依赖（§8.2 Q14）。
- 依赖：F-S2-08、F-S2-10。
- 完成定义：在验收渠道的 `.app` 上通过，证据目录完整，交付文件 sha256 与被测一致（60 §7）；场景操作只用可访问名称（A4）。
- 验证：`node tests/e2e/super-drive/packaged/run-packaged.mjs --app "<…>/PuppyOne Acceptance.app" --scenarios P02,P03,P04,P08,P11`。

**F-S2-10 · acceptance-identity** — scope `super-drive-main`
- 目标：§1.10.2 验收渠道与用户数据目录覆盖。
- 文件：**[主线程（建议）]** `shared/desktop/application-identity.mjs`、`shared/desktop/release-identity.mjs`、`electron/main/build-info-service.mjs`、`scripts/prepare-desktop-build.mjs`（接受 `--channel acceptance`）；测试 `tests/unit/release/build-identity/acceptanceChannel.test.mjs`。
- 依赖：无（可与 S2 其他卡并行）。
- 完成定义：`acceptance` 渠道产物名、appId、userData 与 dev/stable 不同；`--puppyone-user-data-dir` 只在 acceptance/dev 生效；`npm run check:boundaries` 中 `check-desktop-build-identity-architecture.mjs`、`check-macos-release-config.mjs` 通过。
- 验证：`npx vitest run tests/unit/release tests/integration/release`；`node scripts/prepare-desktop-build.mjs --channel acceptance && npm run dist:mac:prepared`。

**F-S2-11 · no-project-entry** — scope `super-drive`
- 目标：全新安装、没打开任何项目时也能进入 Super Drive（00 D33、O5）；Windows 构建强制关闭 `superDrive`（O6）。
- 文件：`src/components/MinimalOnboarding.tsx` 一带的项目入口页（只加一个受 flag 与实验开关双重控制的入口，进入后打开一个不绑定项目的 Super Drive 窗口或面板；具体挂载方式在本卡开始时读代码确认，**未确认**）；**[主线程]** `src/features/flags/registry.ts` 的平台条件（Windows 恒为关）。测试 `tests/component/super-drive/entry/noProjectEntry.test.tsx`。
- 依赖：F-S1 的 flag 与 surface 注册。
- 完成定义：flag 或实验开关任一为关时入口不出现；两者都开时，全新用户数据目录启动后两次点击内进入 Super Drive；Windows 构建中实验开关不出现；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/entry`；T2 场景 P3 从全新用户数据目录通过。

**F-S2-13 · local-data-statement** — scope `super-drive`（WS-FE2）
- 目标：D79。Super Drive 页面底部常驻一行说明：“您的文件和处理结果始终保存在本机，不会上传到 puppyone 云端，除非您选用 puppyone Cloud。”引擎与设置页完整说明一次，并列出当前实际生效的数据去向：
  - 默认：只有本机；
  - 登记了用户自己的 S3 桶：“文件保存在您的存储桶【桶名】，处理结果保存在本机”；
  - 配置了第三方模型：追加“已启用【服务商 · 地区】：补充抽取所需的文档片段会从本机直接发送给该服务商”；
  - 选用 puppyone Cloud（L7 起，且律师放行后）：改为说明哪些内容存放在 puppyone Cloud。
- 文案由数据流状态计算得出（`superDriveDataFlow.ts`，纯函数，输入为 Drive 后端、模型配置、Cloud 状态），不允许写死。
- 文件：`src/features/super-drive/browser/LocalDataStatement.tsx`、`src/features/super-drive/superDriveDataFlow.ts`、i18n `super-drive.dataFlow.*`。
- 依赖：F-S2-05。
- 完成定义：每种数据流组合都有单元测试；文字与实际配置不一致时测试失败；不可关闭；**A1–A5**。
- 验证：`npx vitest run tests/unit/super-drive/dataFlow tests/component/super-drive/browser`；T2 场景 P26。

**F-S2-12 · freshness-basic** — scope `super-drive`
- 目标：顶栏新鲜度指示的基础版（00 §5.5，D48）：`up_to_date`、`processing`、`publishing` 三种状态，显示本体版本与 `as_of`；点开即队列面板。
- 文件：`src/features/super-drive/browser/FreshnessIndicator.tsx`、`superDriveStateModel.ts`（新增 `FRESHNESS_PRESENTATION`）、i18n `super-drive.freshness.*`。
- 依赖：F-S2-05；**B-S3-09**（L2 期间先用 mock，字段按 10 §8.8 G12）。
- 完成定义：`role="status"` + `aria-live="polite"`；状态切换不闪烁（最短停留 600 ms）；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/freshness`。

### 7.3 M1 · L3 读懂文件（S3、S4）

**F-S3-01 · upload-queue** — scope `super-drive-main` + `super-drive`（分提交）
- 目标：上传服务（§1.2.3）、preload `upload`、`DriveDropZone`、`ProcessingQueuePanel`、`uploadQueue.ts`。
- 依赖：F-S2-07；**B-S3-07**（progress 事件）。
- 完成定义：§3.2.6 与 §3.3 的行为全部实现；临时名与原子改名；重名；隐藏文件跳过；磁盘满与权限错误；上传 → `stored` 交接；**A1–A5**（拖拽区可访问名称、队列汇总 `aria-live`）。
- 验证：`npx vitest run tests/integration/super-drive/main/uploadService.test.mjs tests/component/super-drive/queue tests/component/super-drive/browser/dropZone.test.tsx`。

**F-S3-02 · state-matrix** — scope `super-drive` + `super-drive-i18n`
- 目标：`superDriveStateModel.ts` 完整实现 §4.1（全部 16 个契约状态 + `uploading` + 未知 + `blocked_reason` + 附属计数）、状态筛选、§4.8 全部错误码文案（`errors.v1.json` 每个码一个 key）。
- 依赖：F-S2-07；**B-S3-04**。
- 完成定义：`stateModel.test.ts`、`errorMapping.test.ts` 通过；8 种语言齐全；**A1–A5**。
- 验证：`npx vitest run tests/unit/super-drive/state tests/unit/super-drive/errors`；`node scripts/check-localization.mjs`。

**F-S3-03 · selection-banners-retry** — scope `super-drive`
- 目标：选择模型、键盘（§3.2.4）、右键菜单与“更多”菜单（§3.2.5）、选择条、横幅栈与 §4.7 优先级、批量重试（切批、乐观、逐项回滚）、“安全模式重试”。
- 依赖：F-S3-02；**B-S3-07**（`files:retry`）。
- 完成定义：§6.3 `fileTable`、`banners` 组件测试通过；全选跨页（`allMatching`）重试走主进程分页取 id；**A1–A5**（右键菜单项在辅助功能树可见，有键盘等价）。
- 验证：`npx vitest run tests/component/super-drive/browser tests/component/super-drive/banners tests/unit/super-drive/store/optimistic.test.ts`。

**F-S3-04 · details-blocks** — scope `super-drive` + `super-drive-main`
- 目标：详情栏（概览、失败原因卡、解析结果）、`EvidenceSnippet`、`drive-file-protocol.mjs` 与 `file-preview-url`、只读 `FilePreview` 嵌入、§3.4.3 的两级定位（实测 PDF `#page=` 是否生效并记录结论）。
- 文件：`details/*`、`electron/main/super-ontology/drive-file-protocol.mjs`。**[主线程]** `electron/main.mjs` 注册 `puppyone-drive` 为特权协议并在 ready 后 `registerDriveFileProtocol`。
- 依赖：F-S3-02；**B-S3-07**（`files/{fid}`、`blocks`）。
- 完成定义：P10 的“点锚点 → 定位到原文位置”至少以片段高亮达成；协议能力 token 只对 fs Drive 生效、Range 正常、`no-store`；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/details`；`npx vitest run tests/integration/super-drive/main/driveFileProtocol.test.mjs`。

**F-S3-05 · grid-view** — scope `super-drive`
- 目标：网格视图与视图偏好记忆（按 Drive）。
- 依赖：F-S3-03。
- 完成定义：网格与列表共用选择、键盘、右键；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/browser/fileGrid.test.tsx`。

**F-S3-06 · state-contract-and-visual** — scope `super-drive-test`
- 目标：L3 横切门禁“状态机契约测试”的前端半边；状态矩阵截图矩阵（明暗两种外观 × 全部文件状态）。
- 文件：`tests/integration/super-drive/contract/stateContract.test.mjs`、`tests/e2e/super-drive/visual/state-matrix.smoke.mjs`（mock 引擎驱动，截图写证据目录）。
- 依赖：F-S3-02；**B-S3-08**。
- 完成定义：生成枚举与状态矩阵逐项相等；截图覆盖 §4.1 每一行。
- 验证：`npx vitest run tests/integration/super-drive/contract`；`npx electron tests/e2e/super-drive/visual/state-matrix.smoke.mjs`。

**F-S4-01 · s3-drive** — scope `super-drive-main` + `super-drive`
- 目标：s3 Drive 登记（§3.9）、`secret-files.mjs`（`file:` secret_ref、0600、删除 Drive 时清理）、上传禁用说明、预览不可用说明。
- 依赖：F-S2-06；**B-S4-01**、**B-S4-02**。
- 完成定义：字段级错误呈现；密钥只写不读；RustFS 上登记成功并能列出文件；**A1–A5**。
- 验证：`npx vitest run tests/integration/super-drive/main/secretFiles.test.mjs tests/component/super-drive/settings`。

**F-S4-02 · large-directory** — scope `super-drive`
- 目标：1 万文件的虚拟滚动、键集分页（`limit=500`）、骨架占位、`aria-rowcount=total`、快速拖到底部的顺序加载提示。
- 依赖：F-S3-03。
- 完成定义：基准：1 万条目录首屏渲染 < 200 ms（不含网络）、滚动时挂载行 ≤ 120、事件归并 1 万条 < 50 ms；**A1–A5**。
- 验证：`npm run bench:performance -- tests/performance/benchmarks/super-drive`；`npx vitest run tests/component/super-drive/browser/largeDirectory.test.tsx`。

**F-S4-03 · lifecycle-states** — scope `super-drive`
- 目标：`superseded`、`deleted`、`duplicate`、`DOCS_SOURCE_PATH_COLLISION` 的呈现；详情栏版本历史；`file.renamed`/`file.removed` 的列表行为。
- 依赖：F-S3-04；**B-S4-02**。
- 完成定义：fs 与 s3 两个后端的“新增 → 修改 → 重命名 → 删除”在界面上状态与证据正确（30 L3 验收 5 的前端部分）。
- 验证：`npx vitest run tests/unit/super-drive/events tests/component/super-drive/details`。

**F-S4-04 · disk-permission-banners** — scope `super-drive`
- 目标：`StorageMeter`、磁盘低/满、Drive 级权限不足、同步失败、列举不完整、大规模删除挂起与确认、Drive 暂停与删除中（§4.6）。
- 依赖：F-S3-03；**B-S4-02**、**B-S4-05**。
- 完成定义：§4.6 中这些行都有组件测试；mass delete 确认调用 `drive-sync { confirmMassDelete }`；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/banners tests/component/super-drive/engine`。

**F-S4-05 · t1-t2-l3** — scope `super-drive-test`（L3 最后一张 T 卡）
- 目标：T1 L3 场景（§6.5 中 L3 四个）与 T2 P1、P2、P3、P4、P5、P6、P7、P8、P10、P15、P16；T3 探索式测试的任务描述与问题清单模板。
- 文件：`tests/e2e/super-drive/processing/*.smoke.mjs`、`tests/e2e/super-drive/sync/s3-incremental.smoke.mjs`、`tests/e2e/super-drive/packaged/scenarios/P01…P16`（L3 子集）、`tests/support/super-drive/t3-task-L3.md`（任务描述，交给 Agent，不进门禁）。
- 依赖：F-S3-01 至 F-S4-04；**B-S3-08**、**B-S4-04**。
- 完成定义：T1 全绿（含 `--native`：原生拖拽上传、多选与右键菜单、横幅 → 筛选 → 批量重试）；T2 全绿且证据完整；T3 无阻断问题；P16 英文界面无缺失 key、无截断。
- 验证：`npx electron tests/e2e/super-drive/processing/formats-and-failures.smoke.mjs --native`；`node tests/e2e/super-drive/packaged/run-packaged.mjs --app <…> --scenarios P01,P02,P03,P04,P05,P06,P07,P08,P10,P15,P16`。

**F-S3-07 · fs-watcher** — scope `super-drive-main`（WS-FE1）
- 目标：D46 文件系统监听。主进程对每个已登记的 fs Drive 起 `fs.watch(root, {recursive: true})`，按 300 ms 去抖合并路径，过滤隐藏与垃圾文件（与 10 §4.3.2 同一规则表），调用 `:sync` 带 `paths`（D47）；监听出错或丢事件（macOS 的 `kFSEventStreamEventFlagMustScanSubDirs` 语义）时退回一次全量 `:sync`。
- 文件：`electron/main/super-ontology/drive-watcher.mjs`、`tests/integration/super-drive/main/driveWatcher.test.mjs`。
- 依赖：F-S2-03；**B-S3-09**。
- 完成定义：访达中放入一个 md，`published` 达到 D49；1 万文件的 Drive 监听不让主进程 CPU 持续高于 5%；App 退出或 Drive 删除时监听释放。
- 验证：`npx vitest run tests/integration/super-drive/main/driveWatcher.test.mjs`；T2 场景 P19。

**F-S3-08 · freshness-full** — scope `super-drive`
- 目标：新鲜度七种状态全部呈现，文件夹层级汇总，`degraded`、`held` 带原因与操作。
- 文件：`FreshnessIndicator.tsx`、`browser/DriveSidebar.tsx`、`superDriveStateModel.ts`。
- 依赖：F-S2-12、F-S3-02。
- 完成定义：状态矩阵 4.x 新增“新鲜度”一节逐项截图；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/freshness`。

### 7.4 M1 · L4 本体与共享（S5、S6）

**F-S5-01 · review-queue** — scope `super-drive`
- 目标：待确认页（§3.5.1–§3.5.3）：按来源/按种类分组、行内决定、批量、按来源整体接受（`as_of_seq`）、逐项结果、`publish_pending` 与版本号、侧栏与顶栏计数。
- 依赖：F-S3-03；**B-S5-06**、**B-S5-07**。
- 完成定义：100 条按来源整体接受 = 1 次用户操作、1 次 IPC；不出现逐条弹窗；键盘全覆盖；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/review`。

**F-S5-02 · entities** — scope `super-drive`
- 目标：详情栏“实体”标签页、`EntityDetailView`（事实、双时间、别名、取代、已撤回开关、合并重定向）、实体搜索（工具栏搜索框的“实体”页签）。
- 依赖：F-S3-04；**B-S5-05**、**B-S5-08**。
- 完成定义：60 §4 第 8 步“搜索别名 小王 → 同一个人”在组件测试与 T1 中可复现；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/details/entities.test.tsx`。

**F-S5-03 · model-and-usage** — scope `super-drive`
- 目标：未配置模型横幅、模型达上限横幅、设置页模型开关/日上限/自动发布阈值、用量（§3.9）；引擎模型配置入口（接口落地前显示状态与说明，§8.2 Q5）。
- 依赖：F-S3-03；**B-S5-03**。
- 完成定义：不配模型时全流程可用且横幅可关；`usage.changed` 实时；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/settings tests/component/super-drive/banners`。

**F-S5-04 · review-contract** — scope `super-drive-test`
- 目标：待确认相关契约与性能：`by_source` 请求形状、逐项五种结果、批量切批、`review.changed` 合并。
- 依赖：F-S5-01。
- 完成定义：契约测试通过；1,000 项分组渲染 < 100 ms。
- 验证：`npx vitest run tests/integration/super-drive/contract tests/unit/super-drive/review`。

**F-S6-01 · share-parts-extract** — scope `super-drive`
- 目标：把 `ShareStepper`、`ScopeOption`、`ModeOption`、`CopyField` 从 `src/features/cloud/share/ShareWizardDialog.tsx` 移到 `ShareWizardParts.tsx` 并从 `share/index.ts` 导出（纯移动）。
- 依赖：无；需要 §8.2 Q4 确认允许改 `cloud/share`。
- 完成定义：`tests/component/cloud/share/*.test.tsx`、`tests/unit/cloud/share/*.test.ts` 全绿；`check-cloud-frontend-architecture.mjs` 通过；diff 只有移动与 import。
- 验证：`npx vitest run tests/component/cloud/share tests/unit/cloud/share`。

**F-S6-02 · share-dialog** — scope `super-drive` + `super-drive-main`
- 目标：分享面板三步（§3.6）、`mountCommand.ts`、`pack-compile` 预览、`resolve` 提示、`pick-path { purpose: "mount-target" }`、`share-mount` 一键执行。
- 依赖：F-S6-01、F-S5-02；**B-S6-02**、**B-S6-04**、**B-S6-06**。
- 完成定义：默认 suggester / 7 天 / 800；`editor` 禁用并说明；§3.6.2 每个错误呈现；本地命令与 `Share.command` 一致；一键执行产物与 CLI 产物逐字节一致（T1 验证）；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/share/shareDialog.test.tsx tests/unit/super-drive/share`。

**F-S6-03 · share-list** — scope `super-drive`
- 目标：共享列表、刷新链、挂载缺失、到期提醒、撤销确认（“Agent 已读内容无法收回”）、`local_removed=false` 提示、刷新（M1 直接刷新）。
- 依赖：F-S6-02；**B-S6-04**。
- 完成定义：§4.2 全部状态有呈现；`refreshed_from` 链正确；撤销确认框文案与 `notice_code` 对应；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/share/shareList.test.tsx`。

**F-S6-04 · details-shares** — scope `super-drive`
- 目标：详情栏“共享”标签页（`FileDetail.shares`）。
- 依赖：F-S6-03。
- 完成定义：点击跳转并定位 Share；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/details/shares.test.tsx`。

**F-S6-05 · brief-preview** — scope `super-drive`
- 目标：`BriefPreview`（`brief_preview` 只读渲染，`[src:…]` 可点并在详情栏显示对应证据）。
- 依赖：F-S6-03、F-S3-04。
- 完成定义：30 L4 验收 3“每个 `[src:…]` 都能在 Desktop 里点开并定位到原文”在组件测试与 T1 中逐行验证；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/share/briefPreview.test.tsx`。

**F-S6-06 · t1-t2-l4** — scope `super-drive-test`（L4 最后一张 T 卡）
- 目标：T1 L4 场景（`review/accept-by-source`、`share/mount-and-revoke`，有模型与无模型各一遍）；T2 P1–P16 全量；T3；T4 验收包（`ACCEPTANCE.md` 按 60 §4 模板逐步可执行，`sample-drive/` 来源见 §8.2 Q11）。
- 依赖：F-S5-01 至 F-S6-05；**B-S6-07**、**B-S6-08**。
- 完成定义：T1 `--native` 覆盖“按来源整体接受、选范围、选角色、改预算、一键执行、撤销确认”；红队金丝雀 0 命中；T2 全绿；T4 包内 dmg sha256 与 T2 被测一致。
- 验证：`npx electron tests/e2e/super-drive/share/mount-and-revoke.smoke.mjs --native`；`node tests/e2e/super-drive/packaged/run-packaged.mjs --app <…> --scenarios P01-P16`。

**F-S6-07 · drift-badge** — scope `super-drive`（WS-FE3）
- 目标：D50 轻量漂移。共享列表与详情栏中 `drifted` 的 Share 显示“有更新”与受影响事实数，提供“刷新”（调用 `:refresh`，按 refresh 语义生成新 Share）；顶栏新鲜度在有漂移的 Share 时附一个计数。
- 文件：`src/features/super-drive/share/ShareList.tsx`、`details/DetailsShares.tsx`。
- 依赖：F-S6-03；**B-S6-10**。
- 完成定义：发布新版本后 5 秒内列表更新；刷新后旧 Share 显示为 `refreshed` 并链接到新 Share；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/share`；T2 场景 P18（提前到 L4）。

**F-S5-05 · entity-correction** — scope `super-drive`（WS-FE3）
- 目标：D59 用户纠错最小版。实体详情里每条事实提供“标记为错误”“修改值”；实体级提供“合并到…”“拆分”。提交后显示“已提交，正在进入本体（vN → vN+1）”，发布完成后更新。
- 文件：`src/features/super-drive/entities/{FactCorrectionMenu.tsx,EntityMergeDialog.tsx,EntitySplitDialog.tsx}`。
- 依赖：F-S5-02；**B-S5-12**。
- 完成定义：带 `If-Match`，412 时提示“内容已变化，请刷新后重试”；纠错后被影响的 Share 出现“有更新”；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/entities`。

**F-S5-06 · constraint-conflict** — scope `super-drive`（WS-FE3）
- 目标：待确认队列新增 `constraint_conflict` 种类：并排展示冲突双方的事实、证据与来源，操作为“采用甲”“采用乙”“两者都不对（去纠错）”；实体详情顶部显示“冲突未决”横幅。
- 文件：`src/features/super-drive/review/ConstraintConflictCard.tsx`、`superDriveStateModel.ts`。
- 依赖：F-S5-01；**B-S5-11**。
- 完成定义：状态矩阵 4.3 增加该种类；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/review`。

**F-S6-08 · consistency-surface** — scope `super-drive`（WS-FE3）
- 目标：D54–D56、D62 在界面上的呈现：
  - 分享面板：显示“将覆盖到 drive_rev N”，等待覆盖时的进度（最多 20 秒），超时后提示“3 个文件尚未处理完，包里会写明”；高级选项“要求全部处理完”（`require_complete`）、任务组（ticket id）、刷新策略（手动 / 检查点自动 / 固定）。
  - 共享列表与详情：每个 Share 显示覆盖情况、最近一条更新的级别（info / warn / block 三种徽标）、`UPDATES.md` 条目预览；任务组以分组形式显示，组内只提供“整组刷新”。
- 文件：`src/features/super-drive/share/{ShareDialog.tsx,ShareList.tsx,ShareUpdates.tsx,ShareGroupRow.tsx}`。
- 依赖：F-S6-02、F-S6-03、F-S6-07；**B-S6-11**、**B-S6-12**。
- 完成定义：block 级更新在共享列表中最醒目（颜色 token 用 danger 语义）；整组刷新失败时显示具体是哪个成员失败；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/share`；T2 场景 P21。

**F-S6-09 · aigc-and-licenses** — scope `super-drive`（WS-FE3）
- 目标：D66、90 §5。BRIEF 预览与 Pack 文件预览顶部显示“AI生成”角标（文字 + 图标，不只靠颜色）；“关于”页增加“开源许可”入口，打开随包的 `THIRD_PARTY_NOTICES/`（这是许可证要求的署名，不是用户须知；App 内不展示用户须知，D68）。
- 文件：`src/features/super-drive/share/BriefPreview.tsx`；“关于”页入口位置在本卡开始时读代码确认（**未确认**）。
- 依赖：F-S6-05；**B-S6-13**、**B-S3-11**。
- 完成定义：角标不可关闭；开源许可页能列出全部组件；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/share`。

### 7.5 M2 · L5 Suggest 回流（S7、S8）

**F-S7-01 · suggestion-kind** — scope `super-drive`
- 目标：`suggestion` 种类、当日摘要分组、事后撤销（接口存在时）。
- 依赖：F-S5-01；**B-S7-01**、**B-S7-03**。trailer `Contract-Version: docs-api/1.1`。
- 完成定义：契约 1.1 复制与再生成；摘要视图按天合并；**A1–A5**。
- 验证：`node scripts/generate-super-drive-contract.mjs --check`；`npx vitest run tests/component/super-drive/review`。

**F-S7-02 · editor-role** — scope `super-drive`
- 目标：分享面板开放 `editor`（说明“接受后由 Desktop 写回文件”）；suggester 的 Share 行显示“Agent 可写建议”与 `suggest/` 收集计数。
- 依赖：F-S6-02；**B-S7-05**。
- 完成定义：角色说明准确，不承诺未实现的能力；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/share`。

**F-S8-01 · revision-view** — scope `super-drive`
- 目标：`RevisionView`（§3.5.4），按建议或按来源接受；删除/插入用 diff token，并带文字前缀与可访问名称。
- 依赖：F-S7-01；**B-S8-01**。
- 完成定义：50 条建议的典型一天人工操作 ≤ 5 次（30 L5 验收 3 的界面部分，组件测试计数）；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/review/revisionView.test.tsx`。

**F-S8-02 · impact-hint** — scope `super-drive`
- 目标：“影响哪些正在进行的任务”提示（受影响 Share 列表与跳转）。
- 依赖：F-S8-01；**B-S7-03**。
- 完成定义：接受后对应 Share 标 `drifted` 的提示与实际一致；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/review`。

**F-S8-03 · editor-writeback** — scope `super-drive-main`
- 目标：`editor` 角色的建议被接受后，由主进程把文件级修改写回 fs Drive（临时文件 + 原子改名 + `:sync`）；若 Drive 也是打开的 Git 工作区且开启自动提交，由现有 `git-auto-commit` 流程产生提交（只调用其公开入口，不改其实现）。
- 依赖：F-S8-01；**B-S8-01**。
- 完成定义：写回可追溯到建议、Share 与证据（30 L5 验收 4）；写回冲突（文件已被改）时拒绝并提示重新审核。
- 验证：`npx vitest run tests/integration/super-drive/main/writeback.test.mjs`。

**F-S8-04 · t1-t2-l5** — scope `super-drive-test`（L5 最后一张 T 卡）
- 目标：T1 `suggest/revision-mode`；T2 加 P17；T3。
- 依赖：F-S7-01 至 F-S8-03；**B-S8-02**。
- 完成定义：理念文档六步跑通，第 5 步“点接受”≤ 2 s；T2 全绿。
- 验证：`npx electron tests/e2e/super-drive/suggest/revision-mode.smoke.mjs --native`；`run-packaged.mjs --scenarios P01-P17`。

### 7.6 M2 · L6 多模态与演进（S9、S10、S10b）

**F-S9-01 · ocr-presentation** — scope `super-drive`
- 目标：OCR 块在解析结果中的呈现（bbox 锚点、`low_text_confidence` 标记）；扫描件从“暂不支持”转 `outdated` 重跑的呈现。
- 依赖：F-S3-04；**B-S9-01**。
- 完成定义：低置信区域有文字与图标标记（不只靠颜色）；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/details/blocks.test.tsx`。

**F-S9-02 · type-promotion** — scope `super-drive`
- 目标：`type_promotion` 审核卡（实例数、文件数、示例、改名后升格）。
- 依赖：F-S5-01；**B-S9-02**。
- 完成定义：只有人能升格（无自动选项）；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/review`。

**F-S10-01 · drift-refresh** — scope `super-drive`
- 目标：漂移提示、刷新前差异预览（新 BRIEF 头部差异节）、刷新后链更新。
- 依赖：F-S6-03；**B-S10-01**。
- 完成定义：30 L6 验收 2 的界面部分；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/share`。

**F-S10-02 · outdated-and-reprocess** — scope `super-drive`
- 目标：“可更新”文件的批量入口、重跑进度、`reprocess_diff` 审核种类（差异视图复用 `RevisionView`）。
- 依赖：F-S8-01；**B-S10-02**。
- 完成定义：重跑期间旧事实可用的说明准确；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/review tests/component/super-drive/browser`。

**F-S10-03 · puppyone-share-handoff** — scope `super-drive`
- 目标：交付步骤增加 `puppyone share …` 的等价命令（`puppyone share` 是 puppyone-cloud 侧的 CLI，Desktop 只生成与复制）。
- 依赖：F-S6-02；**B-S10-04**。
- 完成定义：两条命令产物一致的说明与测试样例。
- 验证：`npx vitest run tests/unit/super-drive/share`。

**F-S10-04 · t1-t2-l6** — scope `super-drive-test`（L6 最后一张 T 卡）
- 目标：T1 `drift/refresh`；T2 加 P18；T3；T4 验收包（L6 版 `ACCEPTANCE.md` 追加 Suggest、漂移刷新、OCR 步骤）。
- 依赖：F-S9-01 至 F-S10-03。
- 完成定义：T1、T2 全绿；T4 包 sha256 一致。
- 验证：`npx electron tests/e2e/super-drive/drift/refresh.smoke.mjs --native`；`run-packaged.mjs --scenarios P01-P18`。

**F-S10-05 · host-media-decoder** — scope `super-drive-main`（WS-FE1）
- 目标：D65 macOS 宿主机解码。新增一个随 App 构建的原生 helper（Swift，AVFoundation；构建方式参照现有 `build:native-drag`、`build:native-templates`）：输入媒体文件路径，输出 16 kHz 单声道 WAV 与 1 fps 帧序列 PNG（含 pts 清单）和 manifest（源文件 BLAKE3、helper 版本），写入引擎 scratch 的交换目录；主进程按引擎请求调用，超时与取消可控。HEIC 图片同样经此 helper 转 PNG。
- 文件：`native/super-drive-media-decoder/**`、`scripts/build-native-media-decoder.mjs`、`electron/main/super-ontology/media-decoder.mjs`；**[主线程]** `package.json` 构建脚本与签名配置。
- 依赖：F-S2-02；**B-S10-05**。
- 完成定义：H.264/AAC 的 mp4、m4a、mov 能产出与源时长一致（误差 ≤ 50 ms）的 WAV 与帧序列；helper 不联网、不写 scratch 以外的位置；App 退出时子进程被回收；Windows 构建不包含该 helper（D33 已关闭功能）。
- 验证：`npx vitest run tests/integration/super-drive/main/mediaDecoder.test.mjs`；T2 场景 P22。

**F-S10-06 · speaker-identity** — scope `super-drive`（WS-FE3）
- 目标：D74。录音详情里列出说话人（`S1`、`S2`…）、各自发言时长与代表片段（点击播放）、候选身份及其依据（参会人元数据、自我介绍原文）；操作为“确认为…”“改为…”“不是任何已知的人”；待确认队列新增 `speaker_identity` 卡片。
- 文件：`src/features/super-drive/details/SpeakerPanel.tsx`、`review/SpeakerIdentityCard.tsx`。
- 依赖：F-S5-01、F-S10-07；**B-S10-08**。
- 完成定义：未确认的说话人旁显示“未确认，相关内容暂不进入本体”；不出现任何声纹库或跨录音识别的入口；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/details tests/component/super-drive/review`。

**F-S10-07 · media-presentation** — scope `super-drive`（WS-FE2）
- 目标：音视频在 Super Drive 中的呈现：处理进度按时间（“已转写 23:10 / 1:42:00”）；解析结果以带时间戳的发言列表展示；点击 `#t=` 出处在内置播放器中从该时间点播放；视频显示关键帧条带与帧文字；`DOCS_PARSE_CODEC_UNAVAILABLE` 显示编码名与配置方法；媒体并发池排队状态单独显示。
- 文件：`src/features/super-drive/details/{MediaTranscript.tsx,MediaPlayer.tsx,KeyframeStrip.tsx}`、`superDriveStateModel.ts`。
- 依赖：F-S3-04；**B-S10-07**、**B-S10-09**。
- 完成定义：播放器复用现有媒体查看器（`editor/viewers` 中的媒体预览，若可复用；**未确认**）；时间戳跳转误差 ≤ 0.5 秒；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/details`。

**F-S10-08 · t1-t2-media** — scope `super-drive-test`（S10b，L6 最后一张 T 卡的媒体部分）
- 目标：T1 媒体场景；T2 场景 P22–P25；L6 owner 验收清单追加音视频步骤。
- 依赖：F-S10-05 至 F-S10-07。
- 完成定义：T1、T2 全绿。
- 验证：`run-packaged.mjs --scenarios P22,P23,P24,P25`。

### 7.7 M3 · L7 企业与云（S11、S12）

**F-S11-01 · enterprise-buckets** — scope `super-drive` + `super-drive-main`
- 目标：oss/cos/r2 登记流程（R2 走 s3 形状）、只读说明、Drive 访问策略（允许/禁止前缀）的展示。
- 依赖：F-S4-01；**B-S11-01**、**B-S11-03**。
- 完成定义：三家各能登记并列出文件；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/settings`。

**F-S12-01 · cloud-proxy** — scope `super-drive-main`
- 目标：Cloud 代理模式：`control-client` 的第二种基址经 puppyone-cloud（用 `normalizeCloudApiBaseUrl` 校验，白名单不变）与 Cloud 会话；Cloud 模式下分享范围同步为 `DesktopCloudScope`（`createCloudScope`）。
- 依赖：F-S6-02；**B-S12-01**。
- 完成定义：`shared/cloudEndpoint.js` 无改动（架构测试比对）；本地与 Cloud 模式切换有明确界面与状态；**A1–A5**。
- 验证：`npx vitest run tests/integration/super-drive/main/controlClient.test.mjs -t cloud`；`git diff --exit-code <base> -- shared/cloudEndpoint.js`。

**F-S12-02 · audit-and-remote** — scope `super-drive`
- 目标：审计视图（谁在何时把什么共享给了哪个 Agent）、远端 Worker 的 `oo context mount --remote` 命令生成。
- 依赖：F-S12-01；**B-S12-02**。
- 完成定义：审计记录只读、可筛选；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/share`。

**F-S12-03 · t1-t2-l7** — scope `super-drive-test`（L7 最后一张 T 卡）
- 目标：T1 `enterprise/buckets` 与 Cloud 代理场景；T2 追加对应场景；T3。
- 依赖：F-S11-01 至 F-S12-02。
- 完成定义：三家对象存储“新增 → 修改 → 删除”界面正确；远端挂载产物与本地逐字节一致的证据。
- 验证：`npx electron tests/e2e/super-drive/enterprise/buckets.smoke.mjs --native`。

### 7.8 M3 · L8 规模与轻量（S13、S14）

**F-S13-01 · scale-10k** — scope `super-drive`
- 目标：1 万文件首屏 < 1 s、滚动无掉帧（帧间隔 p95 ≤ 16.7 ms）；冷启动大量 `file.state_changed` 时关闭行级过渡、合并渲染。
- 依赖：F-S4-02；**B-S13-03**。
- 完成定义：`smoke:renderer-performance` 同类测量写入证据；基准不回退。
- 验证：`npm run bench:performance -- tests/performance/benchmarks/super-drive`；`npx electron tests/e2e/super-drive/scale/ten-thousand.smoke.mjs`。

**F-S13-02 · related-marker** — scope `super-drive`
- 目标：BRIEF 预览中标出来自语义召回的“其他相关”条目（只读标记）；设置页显示向量能力状态（若后端在 health 中暴露）。
- 依赖：F-S6-05；**B-S13-02**。
- 完成定义：标记不影响确定性展示；**A1–A5**。
- 验证：`npx vitest run tests/component/super-drive/share/briefPreview.test.tsx`。

**F-S14-01 · docker-free-runtime** — scope `super-drive-main` + `super-drive`
- 目标：免 Docker 运行时的 preflight 分支、引导与引擎页（不再出现 Docker 检查项；显示内嵌数据库与 sidecar 状态）。
- 依赖：F-S2-02；**B-S14-01**（评审结论）。
- 完成定义：无 Docker 的 macOS 从安装到第一份 BRIEF ≤ 10 分钟，owner 不开终端；**A1–A5**。
- 验证：`npx vitest run tests/integration/super-drive/main/engineLifecycle.test.mjs -t lite`。

**F-S14-02 · t1-t2-l8** — scope `super-drive-test`（L8 最后一张 T 卡）
- 目标：T1 `scale/ten-thousand` 与免 Docker 场景；T2 全量（含 1 万文件 Drive）；T3；T4 验收包。
- 依赖：F-S13-01 至 F-S14-01。
- 完成定义：T1、T2 全绿；T4 包 sha256 一致。
- 验证：`run-packaged.mjs --scenarios all --drive-size 10000`。

### 7.9 卡片总览

| Sprint | 卡 | 含主线程独占子项 |
|---|---|---|
| S1 | F-S1-01 … F-S1-07 | 01、02、03、05、06 |
| S2 | F-S2-01 … F-S2-10 | 02、08、09、10 |
| S3 | F-S3-01 … F-S3-06 | 04 |
| S4 | F-S4-01 … F-S4-05 | — |
| S5 | F-S5-01 … F-S5-04 | — |
| S6 | F-S6-01 … F-S6-06 | — |
| S7 | F-S7-01、F-S7-02 | — |
| S8 | F-S8-01 … F-S8-04 | — |
| S9 | F-S9-01、F-S9-02 | — |
| S10 | F-S10-01 … F-S10-04 | — |
| S11 | F-S11-01 | — |
| S12 | F-S12-01 … F-S12-03 | — |
| S13 | F-S13-01、F-S13-02 | — |
| S14 | F-S14-01、F-S14-02 | — |

每个 Sprint 都有前端工作，没有“无”的 Sprint。

---

## 8. 风险、开放问题与待总纲裁决

### 8.1 风险

| # | 风险 | 对策 | 归属卡 |
|---|---|---|---|
| R1 | 集成分支从 `qubits` 拉出，但 `qubits` 落后 `main` 98 个提交且没有 ShareWizard；后续同步 `qubits` 时与 app-shell、cloud、locales 的改动冲突 | 先按 `AGENTS.md` 把 `qubits` 同步到 0.3.27 再拉分支（Q1）；Super Drive 的改动集中在新目录，对现有文件只做 §1.3 列出的最小改动 | F-S1-02 |
| R2 | macOS GUI 应用不继承登录 shell 的 PATH，找不到 `docker`/`oo` | 用 `readUserCommandEnvironment` 读用户 shell 环境；仍找不到时允许手动指定 `oo`；引导里给出常见安装位置 | F-S2-01 |
| R3 | 首次 `oo serve` 拉镜像耗时长（分钟级），界面只能显示“启动中” | 显示已等待时长与“首次约需 1 分钟”；日志入口；请求后端在 `--json` 模式输出阶段行（§8.3 G5） | F-S2-02 |
| R4 | 登记/删除本地 Drive 会重建引擎容器，SSE 断开几秒，用户误以为崩溃 | 主动标记 `restarting(drive_change)`，文案“正在应用 Drive 变更” | F-S2-06 |
| R5 | 键集分页不能随机跳页，1 万文件快速拖到底部时要顺序翻 20 页 | 骨架占位 + “正在加载 x/y”；`limit=500`；后端列表 p95 < 100 ms（10 §11.3）；必要时请求后端提供 `offset` 近似跳转（不在 M1） | F-S4-02 |
| R6 | DOCX/PPTX/XLSX 无法在原文预览里精确定位，只能片段高亮 | M1 以片段高亮满足验收；精确定位需要 shared-ui 提供公开 reveal 接口，另开卡并单独评审（不改数据工作区） | F-S3-04 |
| R7 | 虚拟列表只挂载可见行，T2 用 Peekaboo 按名称找不到屏幕外的行 | T2 场景先用搜索/筛选把目标行带到可见区再点；列表 `aria-rowcount` 正确 | F-S2-09 |
| R8 | 6 种非中英语言的译文质量 | 中英人工校对；其余标注待复核；P16 只要求英文完整无截断；德法长文案截断检查 | F-S1-04 |
| R9 | 复制了 `cloud/utils` 的少量工具函数，日后两处漂移 | 只复制纯函数并在注释里注明来源；领域迁移时合并到 `src/lib/` | F-S2-05 |
| R10 | 审核疲劳（00 §8） | 按来源整体接受、键盘全覆盖、确认率统计（是否接入遥测未确认） | F-S5-01 |
| R11 | 渲染层目录规范的目标布局是 `src/<domain>/`，日后整体搬迁会移动 `src/features/super-drive/` | 搬迁按该文档“只移动不改逻辑”的规则单独提交；本分册的路径届时随之更新 | — |
| R12 | `puppyone-drive:` 新协议扩大了渲染层可读的本地文件面 | 能力 token、一次一文件、只读、只对已登记 fs Drive、`no-store`；集成测试覆盖穿越与过期 | F-S3-04 |
| R13 | 写回（M2 editor）与 Git 自动提交交互复杂 | 只调用 `git-auto-commit` 的公开入口；写回冲突一律拒绝并回到审核 | F-S8-03 |

### 8.2 待总纲裁决

> 2026-11-16 交叉校对后，本节各条的裁决结果统一记在 00 §9（Q1 已改为从本地 `main` 拉分支、不同步 `qubits`，见 D43；Q17、Q18 已由 owner 确认按建议执行，见 00 D33、D36）；缺口 G1–G10 的契约增补见 10 §8.8。下表保留为原始提问。

| # | 涉及 | 问题 | 建议 |
|---|---|---|---|
| Q1 | D36、50 §2.2 | `codex/super-drive` 从 `qubits` 拉出，但 `qubits`（`d2c9f079`）没有 `src/features/cloud/share/`，D33 要求复用的 ShareWizard 只在 `main`（0.3.27）上 | 拉分支前先按 `AGENTS.md` “发版后把 `qubits` 同步到 `main`”完成同步；若暂不同步，F-S6-01 依赖顺延到同步之后 |
| Q2 | D7 | Puppyone `AGENTS.md` 规定当前规格只放 `puppy-issues/document/puppyone-desktop`，`docs/` 只是迁移说明 | 指针文档放 `puppy-issues/document/puppyone-desktop/super-drive/README.md`，Puppyone 仓库内不放 |
| Q3 | D29、D32 | “一键执行”是执行那行 CLI，还是调用控制 API | 调用 `POST /v1/docs/shares`（与 CLI 等价、产物逐字节一致，60 P11 校验）；不在 shell 里执行，CLI 子进程只作兜底 |
| Q4 | D6、D33 | 复用 ShareWizard 需要从 `cloud/share/ShareWizardDialog.tsx` 抽出部件（改 cloud feature，但不是数据工作区）；本地 fs Drive 的 Share 不涉及 Cloud 项目，`DesktopCloudScope` 只能复用形状 | 允许 F-S6-01 的纯移动重构；M1 只复用 `DesktopCloudCreateScopeRequest` 形状，Cloud 代理模式（L7）才真正调用 `createCloudScope` |
| Q5 | D18、D19、10 §2.8 | 引擎模型（provider、base_url、key）只在 `<state>/docs.json` 里，控制 API 没有读写接口；Desktop 无处配置模型 | 后端在 1.x 增加 `GET/PUT /v1/docs/config/model`（`If-Match`）；key 由 Desktop 主进程写成 0600 文件并以 `file:` secret_ref 传入；是否允许从 Desktop 已有的 Model Connections 一键导入另议 |
| Q6 | D3、D8 | s3 Drive 的上传与 App 内预览：Desktop 没有 S3 客户端依赖，控制 API 没有读原件的端点（refs 端点只对 Share 凭证开放） | M1：s3 Drive 不支持从 App 上传、不支持 App 内预览（明确说明）；预览需要后端增加 `GET /v1/docs/files/{fid}/content`（控制 token、Range）；上传放到 L7 Cloud 代理（云端预签名） |
| Q7 | D24、10 §11.1 | 审核决定没有撤销接口；10 §11.1 写“自动接受的建议事后可撤销”但未给端点 | M1 不提供撤销；L5 前后端补 `POST /v1/docs/review:undo`（或 overlay 回退）并冻结进 1.1 |
| Q8 | D40、60 §3.2、§3.4 | 60 的启动命令 `--user-data-dir` 在现有代码里不生效（`build-info-service.mjs:53-54` 无条件 `setPath("userData")`）；验收版需要独立应用名 | 新增 `acceptance` 构建渠道 + 仅在 acceptance/dev 渠道生效的 `--puppyone-user-data-dir`（§1.10.2）；`shared/desktop/application-identity.mjs`、`release-identity.mjs`、`build-info-service.mjs` 列入 50 §4 主线程独占 |
| Q9 | 60 §3.2 | `tests/e2e/packaged/super-drive/` 会被 `scripts/check-test-layout.mjs` 拒绝（e2e 下第二段 `packaged` 不是登记的测试域） | 改为 `tests/e2e/super-drive/packaged/`；或在 `testDomains` 里加 `packaged`（不推荐，语义不是域） |
| Q10 | 50 §4 | `tests/config/discovery.mjs`（测试域）、`tests/support/runners/run-desktop-e2e.mjs`、`scripts/release-checks/checks.json` 是多张卡都要改的共享文件 | 列入主线程独占 |
| Q11 | 60 P3、§4 第 4 步 | “导入示例文件夹”的示例资料从哪来、怎么打包（离线 P15 要求不能下载） | 从 OO 评测集挑 10–15 份合成文档（不含红队文件），经私有词表检查后作为 `extraResources` 打进验收渠道的包（改 builder 配置与 `package.json`，主线程）；正式渠道不带 |
| Q12 | 10 §4.12 与 §8.2 | 降级矩阵写 `cas.status=low`，但 `ComponentStatus` 枚举没有 `low` | 后端改用 `degraded`（带 `code=DOCS_ENGINE_DISK_LOW`），或把 `low` 加进枚举（minor）；前端暂按 `degraded` 呈现 |
| Q13 | 10 §8.4 末段 | `errors.v1.json` 的 `i18n_key` 写作 `superOntology.errors.<code>`，与 Puppyone 消息 ID（`<文件名 namespace>.<key>`）不符 | 改为 `super-drive.errors.<CODE>`，或去掉该字段由前端按码推导；前端以 `super-drive.errors.<CODE>` 为准 |
| Q14 | D35、60 §2 | T2 的 `connectOverCDP` 需要 `playwright-core`（开发依赖），D35 只禁止新的 UI 库与状态库 | 允许以 devDependency 引入 `playwright-core`（仅测试用，不进打包产物）；由主线程改 `package.json` |
| Q15 | 00 §5.1、10 §8.2 | `FileSummary` 没有源文件修改时间，“类百度网盘”的“修改时间”列无法提供 | 后端加可选字段 `modified_at`（源 `last_modified`/mtime）与 `sort=modified_at`（minor） |
| Q16 | D28、10 §7.6 | 没有“延长有效期”的接口；用 refresh 代替会生成新版本、内容可能变化 | M1 用“以同参数再共享”代替并说明会更新内容；如需真正延期，后端加 `PATCH /shares/{sid}`（`expires_at`，If-Match） |
| Q17 | D30、OO `packages/cli/package.json` `ooPlatforms` | `oo` 只发布 darwin/linux，Windows 版 Desktop 无法用 Super Drive | M1 在 Windows 上显示 `unsupported_platform`；是否把 flag 在 Windows 构建中强制关闭由总纲定 |
| Q18 | D33、60 §4 | workspace surface 只在打开了项目的窗口里存在；全新安装（T2 从零开始）时停在 onboarding，没有项目就进不了 Super Drive（**未确认** onboarding 页能否进入设置与 surface） | 在 onboarding / 项目入口页（`src/components/MinimalOnboarding.tsx` 一带）为开启了实验开关的用户加“打开 Super Drive”入口，或把 Super Drive 定为不依赖项目的应用级页面（需改 `DesktopWorkspaceContent`，影响更大）；需要总纲在 L2 前定 |

### 8.3 对后端契约的依赖清单

#### 8.3.1 已在 10 §8 中、前端必须依赖的部分

| 类别 | 内容 |
|---|---|
| 进程与凭证 | `oo serve --state-dir --port 0 --json [--keep-engine]`、`--status --json`、`--stop`；退出码 0/2/3/4/5/6/7；`serve.json`（`protocol`、`pid`、`port`、`api_version`、`token_file`、`state_dir`、`started_at`）；`serve.token` 0600 每次启动轮换；`GET /livez` |
| 通用 | 头 `OO-Docs-Api` / `OO-Docs-Api-Min`；`Idempotency-Key` 与 `OO-Idempotent-Replay`；`ETag`/`If-Match`、`412` 的 `details.current_revision`；分页 `{items,next_cursor,as_of_seq}`；批量 ≤ 500 与逐项结果；错误 `{code,message,retryable,details}`；`429` 的 `Retry-After` |
| 端点 | §1.9 表中标为使用的全部端点 |
| 字段 | `Health`（`status`、`components.{db,parser,model,cas,engine}`、`queue`、`disk`、`events_head`、`engine_version`）；`Drive`（`revision`、`status`、`counts`、`pending_review`、`files_total`、`last_sync`、`model_enabled`、`llm_daily_calls`）；`FileSummary`（`state`、`state_since`、`state_code`、`retryable`、`blocked_reason`、`progress`、`pending_review`、`duplicate_of`、`revision`）；`FileDetail`（`history`、`versions`、`parse`、`entities`、`facts`、`shares`、`llm`）；`Block.src`；`Evidence`（`anchor`、`src`、`quote`、`withdrawn_at`）；`Fact`（双时间四列、`confidence_bp`、`origin`、`status`、`superseded_by`）；`Entity`（`status`、`merged_into`、`redirect_from`、`aliases`、`history`）；`ReviewItem`（`kind`、`revision`、`source`、`subject`、`evidence`）；`ReviewGroup`；`ReviewDecideRequest.by_source.as_of_seq`；`ReviewDecideResult`（逐项五种 status、`publish_pending`）；`ContextPack`（`tokens`、`trimmed`、`blocked`、`maybe`、`brief_preview`）；`Share`（`state`、`drift`、`mount.present`、`mount.target_dir`、`refreshed_from`、`expires_at`、`revision`、`command`）；`RevokeResult`（`local_removed`、`notice_code`）；`Usage` |
| 枚举 | `FileState`（16）、`BlockedReason`（5）、`ShareState`（5）、`ShareRole`（3）、`ReviewKind`（7，含 `reprocess_diff`，需要 schema 同步加入）、`EntityType`（9）、`Backend`（5）、`ComponentStatus`（7） |
| 事件 | 持久：`engine.health_changed`、`drive.created/updated/deleted`、`drive.sync_started/completed/failed/held`、`file.state_changed`、`file.renamed`、`file.removed`、`files.bulk_changed`、`review.changed`、`review.decided`、`ontology.version_published`、`share.created/state_changed`、`share.mount_removed`、`pack.compiled`、`usage.changed`、`refs.accessed`；瞬时：`file.progress`、`stream.lagging`；`410 DOCS_ENGINE_CURSOR_EXPIRED` 的 `details.oldest_seq/head_seq` |
| 错误码 | `contracts/docs-api/errors.v1.json` 全部（前端为每个码提供 i18n key，架构测试强制） |
| 契约文件 | `contracts/docs-api/{docs-api.v1.schema.json,errors.v1.json,events.v1.schema.json}`，每个 `$def` 至少一个 `examples`（前端 mock 由它生成，B-S1-01） |

#### 8.3.2 缺口（需后端以 minor 版本补充，或在总纲裁决）

| # | 需要 | 用途 | 对应 |
|---|---|---|---|
| G1 | `FileSummary.modified_at`（可选）与 `sort=modified_at` | “修改时间”列与排序 | Q15 |
| G2 | `GET /v1/docs/files/{fid}/content`（控制 token、Range） | s3 Drive 的 App 内预览 | Q6 |
| G3 | `GET/PUT /v1/docs/config/model` | 配置引擎模型 | Q5 |
| G4 | `POST /v1/docs/review:undo`（L5） | 撤销审核决定、撤销自动接受的建议 | Q7 |
| G5 | `oo serve --json` 启动阶段行（`docker_info`、`images`、`compose_up`、`engine_wait`、`ready`，带耗时） | “启动中”的进度呈现 | R3 |
| G6 | `PATCH /v1/docs/shares/{sid}`（`expires_at`） | 延长有效期 | Q16 |
| G7 | `ComponentStatus` 与降级矩阵一致（`cas` 的“低”） | 组件状态呈现 | Q12 |
| G8 | `errors.v1.json` 的 `i18n_key` 与 Puppyone 消息 ID 规则一致 | i18n 覆盖检查 | Q13 |
| G9 | `tree` 的 `dirs[].counts` 包含失败数与 `pending_review` 汇总 | 左栏目录徽标 | §3.1 |
| G10 | `ReviewKind` 枚举加入 `reprocess_diff`（00 已改，10 §8.2 的 schema 草案还没有） | M2 审核种类 | §4.3 |

---

## 附：自查记录

| 检查项 | 结论 |
|---|---|
| 与 00 §6 / 10 §8 契约逐项对齐 | IPC（§1.5.3）、store（§1.7）、DataPort（§1.8）、对接表（§1.9）均按 10 §8.3 的端点与 §8.2 的字段写；幂等、If-Match、410、瞬时事件不补齐均已覆盖；需要补充的列在 §8.3.2 |
| 00 修订同步 | 只调 `oo serve`、不调 `oo up`（§1.4.1）；refresh 生成新 active 且旧的转终态 `refreshed`，列表显示 `refreshed_from` 链（§3.7、§4.2）；`reprocess_diff`（§4.3）；md/txt 64 MiB（§3.2.6、§4.1）；POST 带 Idempotency-Key、PATCH/DELETE 带 If-Match（§1.4.2）；410 全量重拉后重新订阅、`file.progress`/`stream.lagging` 不补齐（§1.4.3、§1.7.4、§4.5） |
| 状态矩阵无遗漏 | 文件状态 16 个契约值 + `uploading` + 未知 + 5 个 `blocked_reason` + 附属计数（§4.1）；Share 5 个 + 进行中 + 挂载缺失 + 未知（§4.2）；待确认 7 种 + 未知 + 5 种逐项结果（§4.3）；引擎 14 个状态（`idle`、`probing` 分列），覆盖要求的 8 个（§4.4）；组件 5 × 7（§4.4）；连接 4 个（§4.5）；页面级 30 行，覆盖要求的 11 种场景（§4.6） |
| 现有组件、token、脚本引用 | 均给出路径；未能在代码中确认的写“未确认”：PDF `#page=` 片段是否传到内嵌查看器、实验开关 i18n 的确切 key 前缀、确认率是否接入遥测、onboarding 能否进入 Super Drive、`chrome-remote-interface` 替代方案 |
| 50 / 60 约定 | 卡号两位序号、trailer、分支名、scope、主线程独占标注（§7.0）；A1–A5 从 F-S2 起进入每张界面卡；T1/T2 场景卡放在每个闭环最后一个 Sprint（F-S2-09、F-S4-05、F-S6-06、F-S8-04、F-S10-04、F-S12-03、F-S14-02）；验收版独立应用名与用户数据目录（§1.10.2） |
