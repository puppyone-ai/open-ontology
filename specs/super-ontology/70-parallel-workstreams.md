# Super Ontology 并行板块与排期

> 前提：[00 决策](00-overview-and-decisions.md) D44；[30 闭环](30-closed-loops-and-sprints.md) 定义“交付什么、怎么验收”；本文定义“谁同时做什么、怎么不互相阻塞、多久做完”。
> 任务卡本身在 [10 后端](10-backend.md)、[20 前端](20-frontend.md)、[80 形式化验证](80-formal-verification.md)。本文只做分组、依赖和排期，不重新定义卡。

## 1. 结论

- **12 个板块并行**：后端 5 个、前端 3 个、质量 3 个（评测门禁、端到端、形式化）、集成 1 个（主线程）。
- **四个解耦手段**让板块不必等上游完成：契约先行、夹具边界、trait 接缝、目录所有权互斥。
- **闭环交叠**：下一个闭环的代码在上一个闭环签收前就开始写，只是签收前不合入集成主线（30 §1）。
- **排期从串行的约 15 周压到约 12 周**（估算）。关键路径在后端的“作业框架 → 状态机 → 本体 → 可见性与编译 → Share”这一条；前端和质量板块都不在关键路径上，前提是 S1 冻结契约、S2 交付夹具。

## 2. 板块定义

| 板块 | 名称 | 负责 | 拥有的目录（只有本板块能写） | 主要任务卡 |
|---|---|---|---|---|
| **WS-BE1** | 存储与同步 | objectstore provider、指纹与 BLAKE3、CAS、对账、事件去重、企业桶 | `crates/oo-docs/src/{ingest,cas}/`、`crates/oo-connectors` 中的 objectstore 部分 | B-S2-02、B-S2-03、B-S2-05、B-S4-01–04、B-S11-01–03 |
| **WS-BE2** | 解析 | parser sidecar、各格式到 IR、XLSX、嗅探与预检、OCR | `crates/oo-docs/src/parse/`、`tools/doc-parser/` | B-S2-06、B-S3-01–03、B-S3-05、B-S3-06、B-S3-10、B-S9-01 |
| **WS-BE3** | 本体 | 规则抽取、LLM 补抽、resolve、双时间、待确认队列、类型升格、重跑、投影、向量 | `crates/oo-docs/src/{extract,resolve,temporal,review}/`、`publish/constraints.rs`、`api/routes_correct.rs` 的处理逻辑（路由注册归 BE5） | B-S2-07、B-S5-02–06、B-S5-11、B-S5-12、B-S9-02、B-S10-02、B-S10-03、B-S11-04、B-S13-02 |
| **WS-BE4** | 编译与共享 | 可见性单一出口、BRIEF、Pack、Share、refs、mount、Suggest、漂移与刷新、远端挂载 | `crates/oo-docs/src/{compile,share,suggest}/`、`crates/oo-cli/src/docs/{context,mounts}.rs` | B-S2-08、B-S2-11、B-S6-01–06、B-S6-10–12、B-S7-02、B-S7-03、B-S7-05、B-S8-01、B-S10-01、B-S10-04、B-S12-02 |
| **WS-BE5** | 引擎与 API | 作业框架、状态机、事件 outbox、控制 API、`oo serve`、发布门禁、doctor、MCP、Cloud 代理、免 Docker | `crates/oo-docs/src/{api,store,obs,failpoints,publish}/`、`jobs/`、`state.rs`、`crates/oo-cli/src/docs/serve.rs`、`deploy/docs/`、`tools/mcp-gateway` 的 suggest 部分 | B-S2-04、B-S2-09、B-S2-10、B-S3-04、B-S3-07、B-S3-09、B-S4-05、B-S5-10、B-S5-07、B-S5-08、B-S7-04、B-S7-07、B-S12-01、B-S14-01 |
| **WS-FE1** | 壳与主进程 | flag、surface、主进程服务、IPC、引擎生命周期、验收渠道、无项目入口、上传通道、s3 Drive、Cloud 代理、免 Docker 的客户端侧 | `electron/main/super-ontology/`、`electron/main/ipc/super-ontology-ipc.mjs`、`shared/super-ontology/` | F-S1-01–04、F-S2-01–04、F-S2-10、F-S2-11、F-S3-01（主进程部分）、F-S3-07、F-S4-01、F-S8-03、F-S12-01、F-S14-01 |
| **WS-FE2** | 浏览与状态 | store、事件应用、列表与网格、队列面板、横幅、详情栏、状态矩阵、大目录、OCR 呈现、重跑、规模 | `src/features/super-drive/{browser,banners,details,queue}/`、`superDriveStore.ts`、`superDriveEvents.ts`、`superDriveStateModel.ts` | F-S2-05–07、F-S2-12、F-S3-01（渲染部分）、F-S3-02–05、F-S3-08、F-S4-02–04、F-S9-01、F-S10-02、F-S11-01、F-S13-01、F-S13-02 |
| **WS-FE3** | 审核与分享 | 待确认队列、实体、模型与用量、分享面板、共享列表、BRIEF 预览、修订模式、类型升格、漂移刷新、审计 | `src/features/super-drive/{review,entities,share,settings}/` | F-S5-01–03、F-S5-05、F-S5-06、F-S6-01–05、F-S6-07、F-S6-08、F-S7-01、F-S7-02、F-S8-01、F-S8-02、F-S9-02、F-S10-01、F-S10-03、F-S12-02 |
| **WS-QA** | 评测与门禁 | 评测集、评测工具、私有词与 trailer 检查、故障注入、性能、红队与确定性门禁、契约测试 | `testing/super-ontology/{golden,fixtures}/`、`crates/oo-eval/src/docs/`、`tools/super-ontology/`、`tests/{architecture,integration}/super-drive/contract*` | B-S1-04–06、B-S1-08、B-S3-08、B-S4-06、B-S5-09、B-S6-07、B-S8-02、B-S13-03、F-S1-05、F-S1-06、F-S3-06、F-S5-04 |
| **WS-E2E** | 端到端 | T1 真实点击场景、T2 打包黑盒、T3 Agent 探索、owner 验收包、闭环证据 | `tests/e2e/super-drive/`、`tests/support/super-drive/`、`testing/super-ontology/evidence/` | B-S2-12、B-S6-08、F-S2-08、F-S2-09、F-S4-05、F-S6-06、F-S8-04、F-S10-04、F-S12-03、F-S14-02 |
| **WS-FV** | 形式化验证 | Quint 模型、ITF 轨迹一致性测试、Kani 证明、形式化门禁 | `formal/`、各 crate 内 `#[cfg(kani)] mod proofs`、`crates/oo-docs/tests/conformance_*.rs`、`tests/unit/super-drive/conformance/` | V-S1-01 起（80 分册 §7） |
| **WS-INT** | 集成（主线程） | 共享文件、迁移、契约冻结与版本、合并、台账、闭环签收、owner 验收包交付 | 50 §4 列出的全部共享文件、`contracts/`、`execution/super-ontology/` | 全部 **[主]** 卡、B-S1-01–03、B-S1-07、B-S2-01、B-S5-01、B-S5-13、B-S6-09、B-S7-01、B-S7-06、B-S13-01、F-S1-07 |

“拥有的目录”是写权限的边界：别的板块需要改这些目录，只能提需求给拥有者，或者通过 WS-INT。这样同一时刻两个 writer 不会碰同一个文件（HANDOFF 的硬约束）。

## 3. 解耦手段

### 3.1 契约先行

- S1 结束时冻结 `contracts/docs-api/*`（`1.0-draft`）、`contracts/docs-ir/doc-ir.v1.schema.json`、`contracts/context-pack/context-pack.v1.schema.json`。
- 前端用 schema 生成类型和 mock（F-S1-05），从 S2 起就可以对着 mock 开发全部页面，不等后端。
- 契约改动只能由 WS-INT 提交，按 50 §6 的顺序走。

### 3.2 夹具边界（S2 由 WS-QA 交付，后续随评测集增补）

| 夹具 | 内容 | 让谁可以提前开工 |
|---|---|---|
| `fixtures/ir/` | 评测集子集手工制作的 `oo.doc-ir/v1` JSON（含 PDF、DOCX、XLSX、PPTX 的锚点） | WS-BE3 不等 WS-BE2 的真实解析器，S3 就能做抽取与 resolve |
| `fixtures/ontology/` | 已发布本体的快照（实体、事实、证据、别名、双时间，含红队） | WS-BE4 不等 WS-BE3，S3 就能做可见性、BRIEF、Pack |
| `fixtures/events/` | 控制 API 的事件序列（含乱序、重复、410 过期），由 Quint 模型生成的 ITF 轨迹转换而来（80 §4） | WS-FE2 的 store 不等真实引擎 |
| `fixtures/files/` | 毒丸、加密、超大、损坏、不支持的格式 | WS-BE2、WS-BE5、WS-E2E 的异常路径 |

夹具与真实实现之间各有一个契约测试：真实解析器对同一份文档产出的 IR 必须与 `fixtures/ir/` 在锚点集合上一致（允许文本规范化差异）；真实抽取对 `fixtures/ir/` 的输出与 `fixtures/ontology/` 比对、按评测口径打分。夹具不会悄悄和实现脱节。

### 3.3 trait 接缝

`DocumentSource`（存储）、`Parser`（解析）、`Extractor`（抽取）、`OntologyReader`（编译读本体）、`JobStore`（作业）都在 S1 由 WS-INT 定稿，并各带一个内存实现。每个板块对上游只依赖 trait 与内存实现，联调时再换真实实现。

### 3.4 目录所有权与分支

- 每张卡一个任务分支、一个 worktree（50 §2）。
- 板块内部可以并行多张卡，前提是卡之间不碰同一个文件；板块负责人（执行者）在领卡时声明文件清单，WS-INT 检查冲突。

## 4. 依赖与关键路径

```
S1 契约冻结 + trait 定稿（INT）
 ├─▶ BE5 作业框架/状态机/outbox（B-S2-04）──▶ BE5 完整状态机（B-S3-04）──┐
 ├─▶ BE1 fs 后端（B-S2-02）──▶ BE1 s3 与代际对账（B-S4-01/02）           │
 ├─▶ BE2 md/txt（B-S2-06）──▶ BE2 Docling/XLSX（B-S3-01/02/05）──┐      │
 ├─▶ QA fixtures/ir ──▶ BE3 规则/LLM/resolve/队列（B-S5-02…06）──┼──────┼──▶ 真实 IR → 本体联调（第 5 周）
 ├─▶ QA fixtures/ontology ──▶ BE4 可见性/BRIEF/Pack/Share（B-S6-01…06）┘      │
 │                                                                             ▼
 │                                                   L4 端到端 + 红队 + 确定性（B-S6-07/08，F-S6-06）──▶ owner 验收包 #1
 ├─▶ FE1 主进程/IPC/引擎（F-S2-01…04）──▶ FE2 store/列表（F-S2-05/07）──▶ FE2 状态矩阵（F-S3-02）
 │                                         └──▶ FE3 审核/分享（对 mock，第 3 周起）
 └─▶ FV 工具链 + FM1/FM2/FM4（V-S1、V-S2）──▶ 一致性测试随各板块的实现落地
```

**关键路径**（决定总工期）：`B-S1-03 → B-S2-04 → B-S3-04 → B-S5-04/06 → B-S6-01/02/04 → B-S6-07/08`。这一条上的卡优先分配执行者，评审优先，不能被别的板块的合并阻塞。

**次关键路径**：`F-S2-03 → F-S2-04 → F-S2-05 → F-S3-02 → F-S6-02 → F-S6-06`，和 `V-S2-01（FM1 作业租约）→ 一致性测试 → B-S3-04 签收`。

## 5. 排期（估算）

每格写的是该板块那一周的主要工作；“·”表示本周只做评审、修复或支援。

| 周 | 闭环 | BE1 存储 | BE2 解析 | BE3 本体 | BE4 编译共享 | BE5 引擎 API | FE1 壳 | FE2 浏览 | FE3 审核分享 | QA | E2E | FV | INT |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | L1 | · | sidecar 选型验证 | · | · | · | flag/surface/IPC 骨架 | · | · | 评测集、评测工具、检查脚本 | T1/T2 设计 | 工具链、FM1 草稿 | 契约冻结、trait、供应链、迁移规划 |
| 2 | L2 | fs 后端、CAS、对账 | md/txt → IR | 规则 v0 | 最小 Pack、mount | 作业框架、outbox、API、`oo serve` | 引擎生命周期、验收渠道、无项目入口 | store、列表 | 对 mock 起页面骨架 | fixtures/ir、ontology、events | T1 骨架、T2 基础设施 | FM1、FM2(md)、FM4；Kani KP1/KP2 | 迁移 0137–0140、**L2 签收** |
| 3 | L3 | s3、代际对账 | Docling、XLSX | LLM 补抽、resolve（对 fixtures/ir） | 可见性单一出口、BRIEF（对 fixtures/ontology） | 完整状态机、毒丸、sidecar 监督 | 上传通道 | 状态矩阵、横幅、详情栏块 | 待确认队列、实体（对 mock） | 故障注入 | T1 L3 场景 | FM2 全量、FM3(fs)、一致性测试 | 合并、契约 1.0 定稿 |
| 4 | L3 | ranged、LRU、RustFS 格子 | pdf/docx/pptx 锚点、预检 | 双时间、待确认队列 | Pack 全目录、Share 生命周期 | 文件详情 API、doctor | s3 Drive | 大目录、生命周期态、网格 | 分享面板、共享列表（对 mock） | 1000 文件基准 | T2 L3 场景、T3 | FM3(s3)、KP3/KP4 | **L3 签收** |
| 5 | L4 | · | 修复 | **真实 IR 联调**、门禁 DOC-G1–G5 | refs、`oo context` 全参数 | 发布门禁、实体接口 | · | 详情栏实体 | 模型与用量、BRIEF 预览 | 评测基线、DOC-G8 | T1 L4 场景 | FM5、FM6、FM8、KP5/KP6 | 迁移 0141、0142 |
| 6 | L4 | · | · | 修复 | 红队、确定性、出处门禁 | · | · | · | ShareWizard 部件抽取、详情栏 Share | 红队门禁 | T2 P1–P16、T3、**owner 验收包 #1** | 形式化门禁全绿 | **L4 签收**；D41 |
| 7 | L5 | · | OCR sidecar | 类型升格统计 | suggest 约定、影响式规则 | MCP suggest | 回写通道（editor） | · | suggestion 种类、editor 角色 | 审核负荷基准设计 | T1 L5 | FM7 | D42 定基线、迁移 0143 |
| 8 | L5/L6 | · | OCR 锚点 | 重跑与 `outdated` | diff 接口、漂移检测 | · | · | OCR 呈现 | 修订模式、影响提示、类型升格卡 | 审核负荷基准 | T2 P17、**L5 签收** | FM6 扩展（漂移刷新） | **L5 签收** |
| 9 | L6 | · | · | 全量重建入口 | refresh 差异、`puppyone share` 一致性 | · | · | 可更新文件批处理 | 漂移刷新、`puppyone share` 交接 | OCR 召回评测 | T2 P18、T3、**owner 验收包 #2** | 重跑模型 | **L6 签收** |
| 10 | L7/L8 | oss/cos/r2、事件 | · | 只读投影（D12） | 远端挂载 | Cloud 代理 token 与审计 | Cloud 代理 | 企业桶登记 | 审计与远端 | 1 万文件基准设计 | T1 L7 | FM3 多云事件 | 迁移 0144 |
| 11 | L7/L8 | 分前缀并行列举 | · | 本地嵌入、确定性向量排序 | · | 免 Docker 原型 | 免 Docker 客户端侧 | 1 万文件规模 | 相关标记 | 1 万文件基准 | T2 L7、**L7 签收** | 规模放大的模型边界 | **L7 签收** |
| 12 | L8 | · | · | · | · | 免 Docker 收尾 | · | · | · | 性能回归 | T2 全量、T3、**owner 验收包 #3** | 全部门禁复跑 | **L8 签收**；若 owner 决定，提 Puppyone 唯一 PR（D43） |

**节省从哪里来**：L3 与 L4 的后端、前端各有一半工作靠夹具和 mock 提前到第 3–4 周；L5 与 L6 交叠一周；L7 与 L8 完全并行。

**不能压缩的部分**：第 5 周“真实 IR → 本体”联调、第 6 周红队与确定性门禁、每个闭环的 T2 和 owner 验收包。这些是质量闸门，不为赶工期跳过。

## 6. 产能与资源约束

- **执行者**：每个板块同一时刻 1 名 Codex 执行者（经 `dispatch-retry.sh` 派发），高峰约 8–10 个并行；原生子智能体同时不超过 2 个，只用于需要判断力的工作（形式化建模评审、根因定位、界面视觉、CI 收敛）。主线程只做 WS-INT。
- **机器资源**（HANDOFF §4、§7）：
  - 同一时刻最多 2 个板块跑容器集成测试；Docling sidecar 与 PaddleOCR sidecar 不同时跑；
  - 开工前 `df -g /` ≥ 20 GB，低于 15 GB 立即停；
  - 所有容器打 `oo.lane=super-ontology-<板块>` 标签，用完删除；
  - T2 和 Apalache 大边界验证都要独占机器，排在夜间或单独时段。
- **Codex 额度**：长时间 429 时，关键路径上的卡改由原生子智能体接手（读上一个执行者留下的未提交改动后继续），非关键路径的卡顺延。

## 7. 同步节奏

| 频率 | 活动 | 负责 |
|---|---|---|
| 每天 | 各板块把已过本地检查的任务分支合入集成分支；WS-INT 跑全量检查并更新台账 | WS-INT |
| 每周两次 | 跨板块联调检查点：契约测试、夹具 ↔ 实现一致性、形式化一致性测试 | WS-QA、WS-FV |
| 每周一次 | 集成演示：按 30 分册当前闭环的演示脚本走一遍，记录差距 | WS-E2E |
| 每个闭环 | 签收（30 §3、50 §7、60 §5、80 §8） | WS-INT |

阻塞升级规则：任何一张关键路径上的卡阻塞超过 1 个工作日，WS-INT 当天重新分配资源或调整依赖，并在台账写明原因。
