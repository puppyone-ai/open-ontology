# Super Ontology 总纲与决策记录

> 状态：Proposed（2026-11-16 落版，待评审）
> 范围：Open Ontology 仓库（`feat/super-ontology`）与 Puppyone Desktop 仓库（`codex/super-drive`，从本地 `main` 拉出；开发期间不碰 `main`/`qubits`，最后一次性提 PR）
> 分册：[10 后端](10-backend.md) · [20 前端](20-frontend.md) · [30 闭环与 Sprint](30-closed-loops-and-sprints.md) · [40 开源借鉴](40-open-source-references.md) · [50 提交策略与追溯](50-commit-and-traceability.md) · [60 端到端测试与 owner 验收](60-e2e-and-owner-acceptance.md) · [70 并行板块与排期](70-parallel-workstreams.md) · [80 形式化验证](80-formal-verification.md)
> 产品理念输入：`research/strategy/2026-11-08-context-drive-product-design.md`（在旧主工作区，未进 master；本文 §1 已摘要其中与实现相关的部分）

本文是另外四个分册共同的前提。分册之间有冲突时，以本文的决策记录（§4）和跨仓契约（§6）为准；改动这两节要先改本文，再改分册。

---

## 1. 一句话与产品形态

> **Super Ontology 是 Agent 的网盘：文件放进 Drive，系统自动读懂它们（Index），按任务把一个装好上下文的文件夹共享给 Agent（Share + BRIEF），Agent 以“建议”的方式把新知识写回（Suggest）。**

四个用户可见的动词，加一层用户看不见的索引：

| Drive 的概念 | Super Ontology 里 | 落在哪个仓库 |
|---|---|---|
| 我的云端硬盘 | **Drive**：文件的家，本地文件夹或 S3 兼容桶 | Puppyone（存储与界面）；OO 只读 |
| 搜索与智能推荐 | **Index**：从文件里自动建出“人、项目、决定、别名、取代关系”的本体 | OO |
| 共享给某人 | **Share**：把 Drive 的一个切片挂载进任务目录 `.context/`，带角色、范围、版本、有效期 | 范围与凭证在 Puppyone；编译与挂载在 OO |
| 文件夹首页 | **BRIEF.md**：挂载时编译的一页简报，有 token 预算，每行带出处 | OO |
| 建议模式 | **Suggest**：Agent 只能提建议，人在客户端批量审 | OO（存储与规则）+ Puppyone（审核界面），M2 |

和现有 Open Ontology 的关系：同一个仓库、同一个 `oo` CLI、同一套“草稿 → overlay → 门禁 → 发布 → Pack”纪律。数据库流水线不动；新增一条**文档流水线**，两条在“已发布本体”这一层汇合。对外暂不改名，Super Ontology 是内部产品名和本轮大版本的代号。

## 2. 目标用户与第一版边界

- **主线用户**：个人和小团队。在 Puppyone Desktop 里拖文件进 Drive，自动建 Index，一键把某个文件夹共享给 Agent 任务。
- **第二种接入**：企业已有的 S3/OSS/COS/R2 桶，只读挂载，产出同样的 Index 和 Context Pack（M3）。
- **第一版（M1 结束）包含**：Drive 浏览与上传、文件处理与状态、Index（核心类型 + 开放候选 + 待确认队列）、Context Pack 编译、`oo context mount`、Super Drive 页面与分享面板。
- **第一版不包含**：Suggest 写回（M2）、OCR（M2）、类型升格（M2）、向量检索（M3）、云端 Worker（M3）、免 Docker 运行时（M3）。
- **永远不做**：自动发布低置信事实；把共享范围外的任何字面内容放进 Pack；静默跳过失败的文件。

## 3. 总体架构

```
┌──────────────────────── Puppyone Desktop（codex/super-drive）────────────────────────┐
│ Renderer                                                                             │
│  Super Drive 页面（workspaceSurfaceRegistry 注册，flag: superDrive，默认关）          │
│   ├─ 浏览器：复用 ExplorerTree + 预览器，接 objectstore 版 DataPort                    │
│   ├─ 处理状态列 / 队列面板 / 失败横幅 / 详情栏（实体、证据、被哪些 Share 引用）        │
│   ├─ 待确认队列（Index 审核；M2 复用为 Suggest 审核）                                  │
│   └─ 分享面板（复用 ShareWizard + DesktopCloudScope，新增“可建议”角色与预算）          │
│        │ window.puppyoneDesktop.superOntology.*（preload，经 trusted-ipc 注册）         │
│ Main process                                                                         │
│  super-ontology 服务：引擎生命周期（preflight / oo serve / 健康检查 / 重启）           │
│                      控制 API 客户端（loopback + token，token 不出主进程）            │
│                      事件流转发（SSE → IPC 推送）                                     │
└───────────────────────────────┬──────────────────────────────────────────────────────┘
                                │ HTTP，仅 127.0.0.1，Bearer token（§6）
┌───────────────────────────────┴──────── Open Ontology（feat/super-ontology）──────────┐
│ oo serve：宿主守护进程（Docs Control API，axum）+ 独立 compose 项目里的容器内引擎     │
│ 文档流水线：                                                                         │
│  objectstore provider（OpenDAL：fs / s3 / oss / cos / r2）                            │
│   → ingest（列举 + 指纹 + BLAKE3 内容寻址 + 对账）                                     │
│   → parse（parser sidecar：Docling；calamine 在 Rust 内；PaddleOCR 于 M2）             │
│   → normalize → oo.doc-ir/v1（块 + 稳定锚点）                                         │
│   → extract（确定性规则 → LLM 补抽，经 model-egress，块级缓存）                        │
│   → resolve（别名、实体合并、双时间取代）                                              │
│   → review（待确认队列，阈值自动发布）                                                 │
│   → publish（本体版本 vN，门禁）                                                       │
│   → compile（oo.context-pack/v1：BRIEF.md + SHARE.json + …，证据决定可见性）           │
│ 存储：PG `_oo_docs` 迁移（实体/事实/证据/状态/队列） + 内容寻址目录（IR、原件缓存）     │
│ 与现有共用：overlay、门禁框架、model-egress + secret_ref、Gateway 鉴权、lite 运行时    │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

## 4. 决策记录

每条决策都经过 owner 确认（2026-11-16 三轮 grilling）。编号在分册里引用，不得复用。

### 4.1 产品与边界

| # | 决策 | 理由 |
|---|---|---|
| D1 | 在 OO 仓库内开发，内部名 Super Ontology，对外暂不改名 | 共享流水线与纪律；不动 npm 包名和 hash 锁定的 spec 基线 |
| D2 | 主线做个人和小团队；同一个 provider 以只读方式接入企业已有的桶（M3） | 两种接入只差“上传”与“挂已有桶” |
| D3 | 文件的真相源是 Puppyone Drive（本地文件夹或 Cloud 的 S3 桶）；OO 只读，永不写回 Drive | 职责清楚，满足“非侵入” |
| D4 | 本机优先；接口按可远程部署设计（带鉴权的 HTTP） | 为云端 Worker 留路 |
| D5 | 第一版范围 = Drive + Index + Context Pack + Share/BRIEF；Suggest 在 M2 | 上传完就能给 Agent 用的最短闭环 |
| D6 | Share 的范围、凭证、挂载入口由 Puppyone 负责；BRIEF 与 Pack 编译由 OO 负责 | Puppyone 已有 ShareWizard 与 `DesktopCloudScope` |
| D7 | spec 放在 OO 仓库 `specs/super-ontology/`，正文中文，标识符英文；Puppyone 侧的指针文档按其 `AGENTS.md` 放在 `puppy-issues/document/puppyone-desktop/super-drive/README.md`，Puppyone 仓库内不放 | 不进 hash 锁定的 `docs/*.md`；遵守 Puppyone 文档约定 |

### 4.2 后端

| # | 决策 | 理由 |
|---|---|---|
| D8 | 新增通用对象存储 provider `objectstore`，基于 OpenDAL，后端 `fs`/`s3`/`oss`/`cos`/`r2`；本地用户直接只读指向 Drive 文件夹 | 本地模式不多起服务、不多存数据 |
| D9 | 默认 S3 测试实现用 RustFS；MinIO 只作兼容性测试对象 | MinIO 社区版已归档 |
| D10 | 增量同步：事件只做加速，定期列举对账；变更指纹 = (etag, size, last_modified, version_id)；内容主键 = 本地流式计算的 BLAKE3 | S3 事件至少一次、无序；multipart 的 ETag 不是 MD5 |
| D11 | 新增 trait `DocumentSource`（`list`/`stat`/`read`/`changes`），不改 `SourceEngine`；新增独立的文档流水线 | 现有抽象是“表 → 分块 → 水位线” |
| D12 | 两条流水线在已发布本体层汇合，文档实体与数据库实体可互相链接；文档本体存在独立的 docs-db，链接在 **L7** 落地：把已发布的文档实体以只读投影写入 Agent DB 后建链接，M1、M2 不承诺（O1） | 合同 PDF ↔ 客户表的一行 |
| D13 | 解析器可插拔；默认 Python parser sidecar（Docling），Excel 用 Rust 内的 calamine，M2 加 PaddleOCR；MinerU 默认关、待法务；Marker、Unstructured 不用 | 许可与质量（见 40 分册） |
| D14 | 自定义中间表示 `oo.doc-ir/v1`：文档 = 块序列，每块有稳定锚点（PDF：页 + bbox + 字符区间；表格：`Sheet!A1:C9`；Markdown：标题路径 + 行号；Word：标题路径 + 正文段落序号） | 锚点是出处与隔离的地基，必须自己掌控 |
| D15 | 本体 schema = 固定核心类型 + 开放候选（A 带一点 B）。核心实体：`Person`、`Organization`、`Project`、`Decision`、`Document`、`Topic`、`Event`、`Term`；`Alias`；核心关系：`owns`、`member_of`、`decides`、`supersedes`、`rejects`、`mentions`、`about`、`tried_and_failed`。核心类型受 schema 约束抽取 | BRIEF 固定章节需要确定的类型。“主人的偏好”用 `Decision{subkind=preference}` 表示（来自 `Preferences/` 目录或 frontmatter `type: preference`），不另增核心类型 |
| D16 | 开放候选存为 `Thing{label}`，可搜索、可 resolve，只进 BRIEF 的“其他相关”且预算封顶；同一候选类型 ≥5 个实例、≥3 个文件时进待确认队列提议升格；**只有人能升格**（M2） | 开放但不失控 |
| D17 | 双时间模型：`valid_from`/`valid_to`（现实）+ `recorded_at`/`retracted_at`（系统）；取代只标失效不删除；查询默认过滤失效事实 | Graphiti 的模型，修掉其默认不过滤的缺陷 |
| D18 | LLM 只做补抽：确定性规则优先，LLM 结果必带锚点与置信度，进草稿；不配模型也必须能用（只跑确定性规则） | 和 OO 现有纪律一致 |
| D19 | LLM 成本：复用 `model-egress` 与 `secret_ref`；按“块哈希 + prompt 版本”缓存；每文件、每 Drive 设调用上限；超限退回确定性规则，不中断 | 可控、可预测 |
| D20 | 派生数据：IR 与解析产物放内容寻址目录（BLAKE3）；实体、事实、证据、别名、状态、队列放 PG `_oo_docs` 迁移；第一版不用向量（M3） | BRIEF 是编译出来的，不是检索出来的 |
| D21 | 文件处理状态机见 §5.1；幂等键 = 内容哈希 + 解析器版本 + 抽取 prompt 版本 | 不重复处理 |
| D22 | 文件删除或被新版本取代时撤回其证据；证据全部消失的事实标 `retracted_at`；人工 overlay 永不自动撤回，出处消失时进待确认队列；被撤回事实正被活跃 Share 使用时，该 Share 标 `drifted` | 可追溯、不丢人工 |
| D23 | 解析器或 prompt 升级：受影响文件标“可更新”，后台低优先级重跑；完成前旧事实继续有效；完成后按 diff 走门禁；全量重建只有手动入口 | 不让整个 Drive 同时抖动 |
| D24 | 待确认队列：确定性事实与置信度 ≥ 阈值的事实自动发布；低置信事实、实体合并、别名冲突、开放类型升格进队列；支持批量与“按来源文件整体接受”，不做逐条弹窗 | 吸取 Cursor Memories 的教训 |
| D25 | Context Pack 协议 `oo.context-pack/v1`，与 `role-pack-manifest/v1` 并存；目录结构见 §6.3；token 估算沿用确定性估算器并对中文校准；超预算按“全文 → 摘要 → 只列标题”分级降精度，并写明裁掉了什么 | Repomix 与 Agent Skills 的做法 |
| D26 | **可见性跟着证据走**：只纳入至少有一条证据在共享范围内的事实，且只附范围内的证据；只在范围外出现的实体和别名不得出现，只计入“已拦截”数量；红队文件测试写进门禁 | 物理隔离是产品承诺 |
| D27 | BRIEF 选材全程确定性、不调 LLM、相同输入逐字节可重放；算法见 §5.3 | 可审计、可回放 |
| D28 | Share 生命周期 `active → drifted`，refresh 时旧 Share 转为终态 `refreshed` 并生成新的 `active` Share（`refreshed_from` 链接，旧凭证作废），或 `expired`（默认 7 天）、`revoked`；挂载为物理复制、只读权限；撤销 = 作废 `refs/` 拉取凭证 + 删除本地挂载目录，并在 UI 上写明“已读内容无法收回” | Agent 沙箱和云端 Worker 都能读普通文件 |
| D29 | 第一版命令为 OO CLI 的 `oo context mount`；Desktop 分享面板显示这一行命令供复制，“一键执行”调用等价的 `POST /v1/docs/shares`（产物与 CLI 逐字节一致，不在 shell 里执行）；`puppyone share` 在 M2 作为薄封装 | 第一版不改 puppyone-cloud |
| D30 | 第一版要求 Docker；Desktop 只调用 `oo serve`，由它管理文档引擎的容器生命周期（独立 compose 项目、`oo.docs/v1` 配置），**不走 `oo up`**：`oo up` 依赖的 `oo.stack/v1` 要求至少一个数据库源且模型必填，与纯文档用户和 D18 冲突；免 Docker 在 M3 单独立项 | lite 运行时现状；10 分册 §15 Q1 |
| D31 | 格式：第一版 MD、TXT、CSV、DOCX、XLSX、PPTX、电子版 PDF；扫描件与图片 OCR 在 M2。上限：单文件 ≤200MB（md/txt ≤64MiB），单 Drive ≤1 万文件；超限显式报错 | 范围可控 |

### 4.3 前端

| # | 决策 | 理由 |
|---|---|---|
| D32 | 客户端与 OO 的通信：Electron 主进程新增 `super-ontology` 服务，渲染层只经 IPC 调用；主进程调 OO `oo serve`（只监听 loopback，Bearer token）；Cloud 模式由 puppyone-cloud 代理；不放宽 `cloudEndpoint.js` 的 SSRF 白名单；`oo` CLI 子进程只作兜底 | token 不进渲染层，非侵入 |
| D33 | 新顶层页面“Super Drive”，注册到 `workspaceSurfaceRegistry`，flag `superDrive` 默认关；复用 `ExplorerTree`、预览器、ShareWizard、`DesktopCloudScope`；**现有数据工作区一行不改**。没打开项目时，在 onboarding / 项目入口页给打开了实验开关的用户加“打开 Super Drive”入口（M1，O5），改成应用级页面放到 M3 评估；Windows 构建强制关闭 flag，直到 `oo` 有 Windows 版本（O6） | 非侵入；全新安装也能进入 |
| D34 | 必须覆盖的空态与错态见 20 分册（引擎未启动、缺 Docker、Drive 为空、全部处理中、部分失败、待确认 > 0、Share 漂移或过期、未配置模型） | 每个状态都要有设计 |
| D35 | 严格遵循 Puppyone 现有 design token、组件、i18n（8 种语言）、`check:boundaries` 架构检查；不引入新的 UI 库或状态库 | 与现有 App 一致 |

### 4.4 工程与交付

| # | 决策 | 理由 |
|---|---|---|
| D36 | 分支：OO `feat/super-ontology`（基于 `origin/master` c0aee3b50，worktree `oo-v02/super-ontology`），闭环签收后视情况合入 `master`；Puppyone 的 `codex/super-drive` 从本地 `main`（含 ShareWizard）拉出，**不同步、不推送 `qubits`，不向 `main` 推送或合并任何东西**，见 D43 | owner 要求：Puppyone `main` 零影响 |
| D37 | 评测集：`testing/super-ontology/golden/` 合成约 50 份中文团队文档（docx 纪要、md 决策、xlsx 人员表、pdf 方案、pptx 周报），人工标注实体、关系、别名、取代，另含红队文件；不得使用任何客户数据；提交前跑私有词表检查 | 可量化验收 |
| D38 | 交付按 4 个里程碑、8 个闭环推进，每个闭环都是一次完整的功能验收，见 30 分册 | owner 要求 |
| D39 | 提交身份只用 owner，不加 agent 署名；每个提交带 `Plan: SO-…` trailer；集成分支 + 一卡一枝的任务分支，闭环签收时才进主干并打 `super-ontology/L<n>` 标签；只用普通 merge，不 squash、不 force push；全绿才合并。细则见 50 分册 | owner 约定；代码可追溯 |
| D40 | 最终验收 = 开发者本地 `dist:mac` 打包一个 App 交给 owner 直接使用；交付前必须通过四层测试：T1 源码构建 + 真实后端 + CGEvent 真实点击、T2 打包 App 黑盒（Peekaboo 按可访问名称真实点击，CDP 只做断言）、T3 Agent 探索式测试；交付的文件必须就是 T2 测过的那个（sha256 一致）。owner 验收包在 L4、L6、L8 必须交付。细则见 60 分册 | 测的就是要交付的东西；点击是真的 |
| D41 | L4 签收后，在发布构建中打开 `oo` 的 `docs` feature 并标为实验功能；Puppyone 侧 flag 仍默认关；npm 发布由 owner 本人操作（O2） | 第一版可内测，发布权在 owner |
| D42 | L5 开始时走一次 spec 重新定基线流程（`specs/scripts/rebaseline.py`），把 MCP `suggest` 工具写进 hash 锁定的 `docs/08`（O3） | 不绕过锁定 |
| D43 | Puppyone 侧开发期间零影响：所有工作留在本地 `codex/super-drive` 及其任务分支；闭环签收只打本地标签；功能分支可经授权推送远端备份，但不开 PR。owner 在某次验收包上确认“效果完全 OK”后（最早 L4），集中提**一个** PR 给上级审核，保留逐卡提交，附全部证据与形式化验证报告。OO 侧照常按闭环视情况合并。细则见 50 §2.2 | owner 2026-11-16 要求 |
| D44 | 开发按 11 个并行板块组织：契约先行、夹具解耦、目录所有权互斥，闭环可以交叠；排期与关键路径见 70 分册 | 开发效率与排期 |
| D45 | 验证加入形式化验证：Quint（TLA 语义，Apalache 有界验证 + 随机模拟）为并发协议建模，quint-connect 把模型生成的 ITF 轨迹回放到 Rust 实现、渲染层 store 用同一批轨迹做一致性测试，Kani 对 Rust 核心纯函数做有界证明；各闭环有对应的形式化门禁，见 80 分册 | 并发协议与隔离承诺需要比测试更强的保证 |

## 5. 核心机制（分册的共同前提）

### 5.1 文件处理状态机

```
uploading ─▶ stored ─▶ queued ─▶ parsing ─▶ parsed ─▶ extracting ─▶ indexed ─▶ published
                                     │                     │              │
                                     ▼                     ▼              └─ pending_review: N（附属计数，不是独立状态）
                               parse_failed          extract_partial
                         （自动退避 3 次后转手动）    （已成功的块照常发布）

终态或旁路：unsupported · too_large · encrypted · duplicate（复用已有结果）
生命周期：superseded（出了新版本）· deleted · outdated（解析器或 prompt 升级，待重跑）
```

- `uploading` 只存在于客户端（上传到 Drive 的过程）；从 `stored` 开始由 OO 维护。
- 每个状态都带：进入时间、可选进度（例如 `parsing` 带“第 n 页 / 共 m 页”）、可选错误码与人话说明、是否可重试。
- 所有状态转换写入事件流，客户端订阅后实时更新，不轮询（断线后用 `since` 游标补齐）。

### 5.2 Index 的数据模型（摘要）

- `entity`：稳定 key（类型 + 规范名哈希，合并后保留重定向）、类型（核心类型或 `Thing{label}`）、规范名。
- `alias`：实体 → 别名，每个别名有证据。
- `fact`：主语、谓语（核心关系或属性）、宾语或值、双时间四列、置信度、来源（`rule` / `llm` / `human`）、状态（`draft` / `published` / `retracted`）。
- `evidence`：事实 → 文件版本（内容哈希）+ IR 块 + 锚点 + 原文片段哈希。
- `overlay`：人工编辑，优先级最高，不被重建覆盖。

### 5.3 BRIEF 选材算法

1. 输入：共享范围（路径集合）、可选任务描述 `--for`、预算、角色。
2. 用别名解析器把任务描述解析成种子实体；低置信候选不硬选，写进 BRIEF 头部“可能指的是”。无 `--for` 时，按范围内的中心度与事实新旧选种子。
3. 在共享范围内从种子扩展 1 跳，得到候选事实（D26 过滤）。
4. 按固定章节填充，各有最低配额：你在做什么 / 相关的人 / 当前状态与决定 / 已试过但走不通 / 主人的偏好 / 其他相关 / 已拦截（只写数量）。
5. 章节内排序：命中种子 > 有效时间新 > 证据多；平局按实体 key 字典序，保证确定性。
6. 超预算按 D25 降精度，末尾注明裁掉了什么。
7. “你在做什么”直接取任务描述原文加解析出的实体，不改写。

### 5.4 Share 生命周期

```
            mount
 (none) ───────────▶ active ──源变化──▶ drifted ──refresh──▶ refreshed（旧 Share 终态）
                                              └──同时生成──▶ 新的 active Share（refreshed_from = 旧 id）
                       │                   │
                       ├──到期──▶ expired ◀┘
                       └──撤销──▶ revoked
```

## 6. 跨仓契约

这是前后端两本分册必须一致的部分。字段的完整 JSON Schema 由后端分册定义，前端分册按此消费；新增字段只能向后兼容。

### 6.1 Docs Control API（`oo serve`）

- 只监听 `127.0.0.1`；token 由 `oo serve` 启动时写入 0600 文件，Desktop 主进程读取，渲染层拿不到。
- 契约版本 `docs-api/1.0`，完整定义见 10 分册 §8，schema 文件在 `contracts/docs-api/`；本表是摘要，10 分册新增的端点（tree、blocks、pack 文件、单个 share、ontology/versions、metrics、refs、livez）一并冻结进 1.0。
- 版本前缀 `/v1/docs/`；错误统一为 `{code, message, retryable, details}`，`code` 是稳定的机器码，`message` 只给日志，界面文案由客户端按 `code` 做 i18n。

| 资源 | 方法与路径 | 用途 |
|---|---|---|
| 引擎 | `GET /v1/docs/health` | 版本、依赖（PG、parser sidecar、model-egress）各自状态 |
| Drive | `GET/POST /v1/docs/drives`、`GET/PATCH/DELETE /v1/docs/drives/{id}` | 登记 Drive（后端 + 根路径或桶）、配额、模型开关 |
| 同步 | `POST /v1/docs/drives/{id}:sync` | 触发一次列举对账 |
| 文件 | `GET /v1/docs/drives/{id}/files?path=&state=&cursor=` | 列表含处理状态 |
| 文件详情 | `GET /v1/docs/files/{fid}` | 状态历史、实体、证据、被哪些 Share 引用 |
| 重试 | `POST /v1/docs/files:retry`（批量） | 失败重试 |
| 实体 | `GET /v1/docs/entities?q=`、`GET /v1/docs/entities/{eid}` | 搜索与详情（含事实、证据、别名） |
| 解析 | `POST /v1/docs/resolve` | 文本 → 候选实体与置信度 |
| 待确认 | `GET /v1/docs/review?kind=&source=`、`POST /v1/docs/review:decide`（批量，支持按来源整体） | 审核队列 |
| Context Pack | `POST /v1/docs/context-packs:compile`、`GET /v1/docs/context-packs/{pid}` | 编译与预览 |
| Share | `POST /v1/docs/shares`（mount）、`GET /v1/docs/shares`、`POST /v1/docs/shares/{sid}:refresh`、`POST /v1/docs/shares/{sid}:revoke` | 生命周期 |
| 事件 | `GET /v1/docs/events?since=`（SSE） | 状态变化、队列变化、Share 漂移 |
| 用量 | `GET /v1/docs/usage` | LLM 调用次数与上限 |

### 6.2 稳定枚举

- 文件状态：§5.1 中的全部状态名。
- Share 状态：`active`、`drifted`、`refreshed`、`expired`、`revoked`。
- Share 角色：`viewer`（只读）、`suggester`（只读 + 可写 `suggest/`，默认）、`editor`（可直接写回，M2 起才开放）。
- 待确认种类：`low_confidence_fact`、`entity_merge`、`alias_conflict`、`orphaned_overlay`、`type_promotion`（M2）、`suggestion`（M2）、`reprocess_diff`（M2，D23 重跑差异）。
- 错误码前缀：`DOCS_SOURCE_*`、`DOCS_PARSE_*`、`DOCS_EXTRACT_*`、`DOCS_REVIEW_*`、`DOCS_PACK_*`、`DOCS_SHARE_*`、`DOCS_ENGINE_*`；`DOCS_DOCTOR_*` 只出现在 `oo doctor` 输出中，不属于 API。

### 6.3 Context Pack 目录

```
.context/
├── BRIEF.md          ← ≤预算；固定章节；每行带 [src:锚点]
├── SHARE.json        ← 范围、角色、drive 版本（内容哈希 + 对象 versionId）、有效期、授权人、已拦截数量
├── INDEX.md          ← 下一层清单（三级渐进披露）
├── people/ projects/ decisions.md terms.md
├── refs/             ← 原文片段（锚点前后若干行）；原件按需凭 SHARE.json 中的短期凭证拉取
├── suggest/          ← M2 起启用
└── manifest.json     ← 每个文件的哈希与 token 数
```

## 7. 术语

| 术语 | 含义 |
|---|---|
| Drive | 一个被登记的文件根（本地目录或桶 + 前缀） |
| 文件版本 | 某个 Drive 内一个路径在某个内容哈希下的实例 |
| 块 / 锚点 | IR 中的最小可引用单元及其在原文中的位置 |
| 事实 | 一条带双时间、置信度、证据的主谓宾陈述 |
| 本体版本 vN | 一次发布后的本体快照，Pack 与 Share 都钉在某个 vN 上 |
| Context Pack | 为一个范围和任务编译出的 `.context/` 目录 |
| Share | 一次挂载记录：谁、什么范围、什么角色、哪个版本、何时过期 |

## 8. 风险与对策（总表）

| 风险 | 对策 | 归属 |
|---|---|---|
| 解析质量不够，尤其中文 PDF 表格 | 解析器可插拔；评测集按格式分别出分；M2 加 PaddleOCR | 后端 |
| LLM 抽取漂移导致事实错误 | 证据必带锚点；低置信进队列；评测集回归门禁 | 后端 |
| 泄露范围外内容 | D26 + 红队测试进 CI 门禁 | 后端 |
| 审核疲劳 | 批量与按来源整体接受；连续两周确认率 < 30% 就重新设计 | 前端 + 产品 |
| Docker 门槛劝退个人用户 | 清晰的 preflight 与引导；M3 免 Docker | 前端 + 后端 |
| 本地磁盘被解析产物占满 | 内容寻址去重；产物配额与 LRU；引擎状态里显示占用 | 后端 |
| 与 Puppyone 现有页面冲突 | 独立 surface + flag；`check:boundaries` 全绿 | 前端 |

## 9. 交叉校对裁决记录（2026-11-16）

10、20 分册各自列了“待总纲裁决”。下表是裁决结果，均已同步进相关分册。§9.2 的 6 项由 owner 于 2026-11-16 确认按建议执行。B-Qn 指 10 §15，F-Qn 指 20 §8.2，Gn 指 20 §8.3.2 / 10 §8.8。

### 9.1 已裁决

| 编号 | 裁决 | 落在 |
|---|---|---|
| B-Q1 | Desktop 只调 `oo serve`，由它管理文档引擎容器；不走 `oo up` | D30、§3、30、60 |
| B-Q3 | `oo serve` = 宿主守护进程 + 容器内引擎，对外契约不变 | §3 |
| B-Q4 | DOCX 锚点用“标题路径 + 正文段落序号” | D14 |
| B-Q5 | 偏好用 `Decision{subkind=preference}` | D15 |
| B-Q6 | 新增审核种类 `reprocess_diff`（M2 启用） | §6.2、10 §8.2 |
| B-Q9 | md/txt 单文件上限 64MiB | D31 |
| B-Q10 | `DOCS_DOCTOR_*` 只用于 doctor 输出 | §6.2 |
| B-Q11 | refresh：旧 Share 转 `refreshed` 终态 + 新建 `active` Share（`refreshed_from`） | D28、§5.4 |
| B-Q12 | 10 新增端点一并冻结进 `docs-api/1.0` | §6.1 |
| F-Q2 | 指针文档放 `puppy-issues/document/puppyone-desktop/super-drive/README.md` | D7 |
| F-Q3 | “一键执行”调 `POST /v1/docs/shares`，不跑 shell | D29 |
| F-Q4 | 允许把 ShareWizard 部件从 `cloud/share/` 纯移动抽出（F-S6-01）；M1 只复用 `DesktopCloudScope` 的形状，L7 才真正调用 Cloud | D33 的补充：数据工作区仍一行不改 |
| F-Q5 / G3 | 新增 `GET/PUT /v1/docs/config/model`；key 由主进程写 0600 文件后以 `file:` 引用 | 10 §8.8 |
| F-Q6 / G2 | 新增 `GET /v1/docs/files/{fid}/content`，M1 起 s3 Drive 可 App 内预览；从 App 上传到 s3 推迟到 L7（经 Cloud 预签名） | 10 §8.8 |
| F-Q7 / G4 | 审核撤销 `POST /v1/docs/review:undo` 进 `docs-api/1.1`（L5） | 10 §8.8 |
| F-Q8 | 新增验收构建渠道 `acceptance` 与只在 acceptance/dev 生效的 `--puppyone-user-data-dir` | 60 §3.2、20 §1.10.2 |
| F-Q9 | T2 场景目录改为 `tests/e2e/super-drive/packaged/` | 60 §3.2 |
| F-Q10 | `discovery.mjs`、`run-desktop-e2e.mjs`、`checks.json`、发版身份相关文件列入主线程独占 | 50 §4 |
| F-Q11 | 示例资料从评测集挑 10–15 份（不含红队），作为 `extraResources` 只打进验收渠道 | 60 §3.4 |
| F-Q12 / G7 | `cas` 磁盘低统一为 `degraded` + `DOCS_ENGINE_DISK_LOW` | 10 §8.8 |
| F-Q13 / G8 | `i18n_key` 统一为 `super-drive.errors.<CODE>` | 10 §8.8 |
| F-Q14 | 允许以 devDependency 引入 `playwright-core`，只用于 T2 断言，不进打包产物 | D35 的补充 |
| F-Q15 / G1 | `FileSummary.modified_at` 与按修改时间排序 | 10 §8.8 |
| F-Q16 / G6 | `PATCH /v1/docs/shares/{sid}` 只改 `expires_at`（≤30 天） | 10 §8.8 |
| G5、G9、G10 | 启动阶段进度行、目录汇总计数、`reprocess_diff` 枚举 | 10 §8.8 |

### 9.2 owner 拍板（2026-11-16：全部按建议执行）

| 编号 | 问题 | 结论 |
|---|---|---|
| O1（B-Q2） | D12“文档实体 ↔ 数据库实体互相链接”何时落地。文档本体存在独立的 docs-db，M1 做不到 | 放到 L7：把已发布的文档实体以只读投影写入 Agent DB，在那里建链接；M1、M2 不承诺 |
| O2（B-Q8） | 发布到 npm 的 `oo` 何时默认打开 `docs` feature | **D41**：L4 签收后，在发布构建中打开并标为实验功能；Puppyone 侧 flag 仍默认关。发布本身仍由 owner 操作 |
| O3（B-Q7） | M2 的 MCP `suggest` 工具会让 hash 锁定的 `docs/08` 工具清单不完整 | **D42**：L5 开始时走一次 spec 重新定基线的流程（`rebaseline.py`），把 `suggest` 写进 docs/08 |
| O4（F-Q1） | `qubits` 落后 `main` 98 个提交，没有 ShareWizard | **已改（2026-11-16 owner 追加要求）**：不同步 `qubits`；`codex/super-drive` 直接从本地 `main` 拉出，开发期间不碰 `main`/`qubits`，最后一次性提 PR（D43） |
| O5（F-Q18） | 全新安装、没打开任何项目时进不了 Super Drive（surface 只存在于项目窗口），T2 和 owner 验收都会卡在第一步 | M1：在 onboarding / 项目入口页给打开了实验开关的用户加“打开 Super Drive”入口；把 Super Drive 改成不依赖项目的应用级页面放到 M3 再评估 |
| O6（F-Q17） | `oo` 只发布 darwin/linux，Windows 版 Desktop 用不了 Super Drive | Windows 构建里强制关闭 `superDrive` flag，等 `oo` 有 Windows 版本再开 |
