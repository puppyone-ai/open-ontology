# Open Ontology 设计文档

> Status: Proposed（设计文档；实际实现与验收范围见Spec、Plan和固定证据）
> Owner: Open Ontology Engineering
> Last reviewed: 2026-10-09

Open Ontology（OO）是生成、审核、发布和维护Ontology的开源系统。企业登记数据源后，系统把获准读取的数据汇聚到独立PostgreSQL，产出标准语义资产和Knowledge Pack，并通过Gateway及CLI、HTTP/SDK、MCP等接口供外部harness使用。这里的ontology是Agent理解和访问企业数据的语义与访问层。

内置Pi和网页对话提供快速体验，也用于内部端到端验证。用户取得产物并使用外部接口时，不依赖内置Pi会话。已实现哪些协议、验证过哪些客户端和版本，分别按证据声明；“面向任意harness”是接口设计方向，不代表所有现有客户端已经测过。内置沙盒仍遵守无可复用凭证、只读及最终用户权限边界。

## 阅读顺序

| # | 文档 | 读者 | 内容 |
|---|---|---|---|
| 00 | [愿景、定义与范围](00-vision-and-scope.md) | 所有人 | 问题、ontology 的定义、产品形态、原则、范围与非目标、成功标准、与参考草案的差异 |
| 01 | [总体架构](01-architecture.md) | 所有人 | 分层、三道单向边界、组件与状态归属、三条主数据流、网络分区、仓库结构 |
| 02 | [技术栈与 ADR](02-technology-stack.md) | 工程 | 选型总表、ADR 与 Rust 选型、许可证审计、版本基线 |
| 03 | [数据源、连接器与文件接入](03-sources-and-ingestion.md) | 工程、DBA | 注册机制、连接器接口、权限体检、DBA 清单、类型映射、文件上传与托管 |
| 04 | [同步管线](04-pipeline.md) | 工程 | 变更批次、“CDC 优先、快照补空”算法、Loader、表结构变化、换表、批量路径、对账、hook、编排、监控 |
| 05 | [Agent DB](05-agent-db.md) | 工程、DBA | 命名空间、角色与 RLS、claims 防伪、热备冲突、`_oo` schema、索引与分区、容量 |
| 06 | [Ontology 层](06-ontology-layer.md) | 工程、数据管理员 | Ontology Spec、自动构建流水线、审核、发布门禁、按角色编译、Knowledge Pack |
| 07 | [Gateway](07-gateway.md) | 工程、安全 | 身份与 token、SQL 判定规则、代价与读取控制、并发与过载、错误契约、审计 |
| 08 | [Agent 接口](08-agent-interface.md) | 工程、Agent 开发者 | HTTP API、SDK、`oo` CLI 输出契约、MCP Server、引用约定 |
| 09 | [沙盒与 Agent 运行时](09-sandbox-and-agent-runtime.md) | 工程、安全 | 会话模型、沙盒规格、出口代理、harness 适配、`AGENTS.md`、**状态释放**、预热池 |
| 10 | [安全](10-security.md) | 安全、合规 | 资产、攻击者、信任边界与 STRIDE、提示注入、输出处理、凭证、数据保护、合规、测试 |
| 11 | [部署与运维](11-deployment-and-operations.md) | 运维 | 一键启动、Compose 网络、控制台、升级、备份、SLO、Runbook、K8s |
| 12 | [配置参考](12-configuration-reference.md) | 管理员 | 配置仓库与各文件字段 |
| 13 | [评测与 Benchmark](13-evaluation-and-benchmarks.md) | 工程、客户项目 | OO-Bench 六条轨道、合成数据集、冻结快照、评分与统计、发布门禁、客户数据迭代闭环 |
| 14 | [路线图、风险与待决问题](14-roadmap-and-risks.md) | 所有人 | M0–M4、原型验证清单、风险登记、需与客户确认的问题 |
| 15 | [术语表](15-glossary.md) | 所有人 | — |
| 16 | [参考资料](16-references.md) | 工程 | 调研来源与待核实项 |
| 17 | [Rust 实现契约与迁移验收](17-rust-implementation.md) | 工程、安全 | crate 关系、任务与取消、FFI/插件隔离、类型/托管、构建与验收 |
| 附 | [数据库支持扩展设计](database-support/README.md) | 工程、DBA、安全、客户交付 | 统一准入路线与支持矩阵（等级、晋升/降级）、能力模型与源库写入分类、各类捕获方法与一致性等级、驱动策略、Self-build/Pack/Gateway 复用、逐库画像（MySQL/PG 家族、SQL Server、Oracle、Db2、HANA、国产库、云数仓、OLAP、文档/KV）、一致性套件与证据。数据库范围与准入以其及 Spec 第 7A 章为准，覆盖 03 §2、§8 的路线表 |

## 文档约定

- **状态：** `Proposed`（目标设计）→ `Accepted`（原型验证后确认）→ `Implemented`（有代码与测试）→ `Superseded`。
- **不变量**以 `I-<层>-<序号>` 编号（例如 `I-GW-3`），每条都应有对应的自动化测试（见 [13](13-evaluation-and-benchmarks.md)）。
- **ADR** 以 `ADR-<序号>` 编号，在 [02](02-technology-stack.md) 中维护；推翻一条 ADR 需要新增一条并标注被取代。
- **数字**（阈值、规模、SLO）均为初始值，经性能测试或与客户确认后更新，并记录来源。
- `references/` 目录中的早期草案是本设计的输入之一，不是权威；本目录与其不一致之处，以本目录为准，差异汇总见 [00 §8](00-vision-and-scope.md)。

核心实现栈统一为 Rust（Cargo/Tokio/Axum）；控制台与 TS SDK 保留 TypeScript，Debezium 与 harness 保留上游运行时。本文档仍是设计，依赖候选、接口示例和一键启动命令不代表已实现制品；工程落地约束见 [17](17-rust-implementation.md)。
