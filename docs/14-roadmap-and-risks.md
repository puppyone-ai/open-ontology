# 14 路线图、风险与待决问题

> Status: Proposed
> Owner: Open Ontology Engineering
> Last reviewed: 2026-10-09

里程碑按“依赖与验收”划分，不按日历。每个里程碑的退出条件都是可验证的评测结果（[13](13-evaluation-and-benchmarks.md)）。

## 产品交付边界

各里程碑交付的是Ontology系统：生成、审核、Git版本化、发布、角色Pack及数据访问接口。M1的CLI/HTTP/SDK先提供外接路径，M2继续完成远程MCP和授权；内置Pi、网页及IM是体验和验证入口，各自的安全及释放工作保留。核心产物消费不依赖内置Pi。

面向任意harness通过稳定产物和标准接口实现。具体客户端、版本和transport只声明实际验证的范围；外接验收与内置Pi验收分别记录，不把通用协议设计写成“所有harness已支持”。

## 1. 里程碑

### M0 原型验证（消除关键未知）

每一项都是一个有明确结论的技术验证（spike），结论写成 Evidence，并据此把 [02](02-technology-stack.md) 中对应的 ADR 改为 Accepted 或修订。

| # | 验证项 | 结论要回答的问题 |
|---|---|---|
| P1 | “CDC 优先、快照补空”算法（[04 §4](04-pipeline.md)） | 在 5,000 万行 + 持续写入 + 故障注入下对账是否为零；吞吐是否达标 |
| P2 | PG 事务提交 LSN 的获取 | 能否从 Debezium 事件中可靠拿到提交 LSN；拿不到时事务边界关联是否可行 |
| P3 | Debezium 位点/历史存到控制库（JDBC store） | 在 Debezium Server 3.5 中是否稳定可用 |
| P4 | Valkey `appendfsync always` 的吞吐 | 峰值变更速率下是否够用；不够时的替代（副本 `WAIT`） |
| P5 | SQL 解析器许可证与实现 | pg_query/libpg_query/内嵌 PG 的 LICENSE/NOTICE；parser 大版本配套、重写后再校验、FFI 构建与攻击差分测试 |
| P6 | 角色 + RLS + `SET LOCAL` 方案在从库上的行为与性能 | RLS 对典型 Agent 查询的开销；热备冲突频率 |
| P7 | EXPLAIN 代价阈值标定 | 在客户规格的从库上，代价与实际耗时的关系；阈值取值 |
| P8 | gVisor + Docker 内部网络 + 源 IP 身份 | 身份识别可靠性；沙盒启动时延；预热池效果 |
| P9 | Pi RPC 驱动与会话恢复 | 事件完整性；中止与恢复；模型 base URL 指向代理 |
| P10 | Ontology Builder S0–S5 在客户样例库上的初稿质量 | 描述准确率、关系 P/R 的初始值；每百表 LLM 成本 |
| P11 | Debezium Server + Redis/Valkey sink 事务路由 | SMT 是否覆盖数据与 BEGIN/END；过滤表后的计数如何对齐；重放后是否保持源提交顺序与跨表原子性；不可用则修订 ADR-011 |
| P12 | 超大事务、暂停表与质量快照 | 分段及故障恢复时水位不推进、不一致标记不丢失；Gateway REPEATABLE READ 下结果与质量状态是否一致 |
| P13 | 列级授权与派生对象 | 新增列默认隐藏；星号展开含 JOIN/CTE 且不改 count(*)；日历受限所有者权限、码表与实体 RLS 无越权 |
| P14 | Rust 驱动、Arrow 与原生构建 | portal/TLS/cancel/连接回收；PG numeric 与 MySQL unsigned 双路径；pg_query/DuckDB 的 Linux x86_64/aarch64 构建及 Arrow 版本兼容 |
| P15 | oo-jobs 与 Loader fencing | 领取/心跳/到期重领、旧 worker 隔离、目标提交后强杀、ACK/裁剪与控制库镜像恢复；无重复副作用 |
| P16 | Rust FFI、插件与释放 | 文件/hook 超时真正终止；异步取消/SIGTERM/panic 后 S1–S11 释放；Compose/离线安装与 rmcp 协议契约 |

**退出条件：** P1–P9、P11–P16 全部有结论；P10 给出首个客户的初始基线。

### M1 首个客户 MVP

| 范围 | 内容 |
|---|---|
| 源 | MySQL、PostgreSQL（CDC + 批量）；CSV/Excel/Parquet 上传 |
| 管线 | Debezium 流式 + OO 分块快照；按源事务 Loader（per_table 显式降级）；超大事务恢复；拒绝记录/质量汇总；每日轻量对账；表结构变化处理（加列自动同步但分类发布前隐藏、其余暂停 + 隔离区） |
| Agent DB | 主从；列级授权 + RLS；`_oo` 水位/质量、`oo_dict` 码表/日历、`oo_entity` 实体结果 |
| Ontology | S0–S5 + S10 列分类；有效期/等级、码值 owner 审核；规则与管家驱动的实体解析；业务日历；认证指标控制总数/语义差异；名称生命周期；发布门禁 G1–G13；按角色编译 |
| Gateway | 全部门禁；审计；`query`/`export`/`resolve`/`search_values`/`describe`/`freshness`；质量 warnings 与歧义澄清 |
| 接入 | Rust `oo` CLI、Rust SDK、HTTP API（TS SDK 按 OpenAPI 契约提供） |
| 运行时 | docker-gvisor 沙盒、出口代理、Pi、预热池、状态释放与回收器 |
| 使用方 | 内置网页对话、飞书机器人 |
| 部署 | Docker Compose（standard） |
| 评测 | OO-Logistics 生成器 + 公开题库 v0；客户 dev/holdout 各 ≥ 150 题；`oo-eval` |

**退出条件：** 轨道 A/B/C 二值项 100%；客户 holdout pass^3 ≥ 初始目标（与客户确认）；编造率 ≈ 0；源库审计无平台写操作；首批业务用户试用。

### M2 生产化

- 全量与块修复对账、重做快照自动化；
- 远程 MCP Server（OAuth 2.1、RFC 9728/8707/8693）；外部客户端 token 管理；
- Builder S6–S8 自动候选建议（实体候选进入管家队列，不自动按相似名称合并；候选指标、候选示例）；会话反馈进入审核队列；
- 性能达标（轨道 F）；可观测性 profile；备份恢复演练；
- 评估 `pg_duckdb` 等列式加速在从库上的收益与 RLS 兼容性；
- Slack 适配器；更多 harness 适配器（Claude Agent SDK、Codex app-server）。

### M3 扩展

- Helm chart、kubernetes-sigs/agent-sandbox 驱动、E2B 驱动；
- 数据库扩展：按[数据库支持扩展设计](database-support/README.md)的波次与支持矩阵逐目标推进（SQL Server、Oracle、Db2、HANA、国产库、云数仓、OLAP、文档库等，等级与证据以矩阵为准）；Kafka 缓冲；
- `compile_metric` 工具（为认证指标提供语义查询 → SQL 的编译）；
- Ontology 导出：OSI（Apache Ossie）、dbt semantic models、Cube；导入：dbt manifest、DataHub/OpenMetadata；
- 可复现构建、镜像签名与 SBOM 常态化。

### M4 探索

- **写回通道**（独立设计评审）：面向“动作”的受控写回——必须经过显式审批、参数校验、幂等、审计，与管线和 Gateway 完全分离；
- 多租户部署形态；
- Oracle CDC；MongoDB。

## 2. 风险登记

| # | 风险 | 可能性 | 影响 | 缓解 | 触发信号 |
|---|---|---|---|---|---|
| R1 | 客户源库不允许开 binlog / 逻辑复制 | 中 | 中 | 批量增量 + 每日对账兜底；向客户说明新鲜度影响 | 体检结果 |
| R2 | 源表没有可靠的 `updated_at` | 高（陈旧库） | 中 | 画像检验；小表 `full`；大表请客户加列或改走 CDC | 体检/画像 |
| R3 | 快照算法在某种源位点语义下不成立 | 低 | 高 | M0 P1/P2 验证；只在已验证的源上启用块修复；保留 Debezium 增量快照作为备选 | 对账差异 |
| R4 | PG 行存在数千万行上的聚合查询太慢 | 中 | 中 | 场景表预聚合；索引建议；export + DuckDB；M2 评估列式扩展 | Gateway 超时率 |
| R5 | 自动 Ontology 草稿质量不足，审核负担过重 | 中 | 高 | 按“影响 × 不确定性”排序审核；专家问答卡片降低门槛；用消融数据说明哪些审核最值钱 | 审核积压、D 轨道指标 |
| R6 | 模型编造数字 | 中 | 高 | 可追溯校验（引用必须存在于审计）；诚实性评测为门禁；AGENTS.md 规则 | E10 占比 |
| R7 | 企业不允许数据发往外部模型 | 高（国内企业） | 高 | 私有化模型支持为一等公民；评测中包含私有化模型的基线 | 客户安全评审 |
| R8 | gVisor 在客户环境不可用（内核、云厂商限制） | 中 | 中 | Kata / Firecracker（E2B）驱动；最低限度不得降级为普通 Docker 用于生产 | 安装预检 |
| R9 | 依赖项目维护状态或许可证变化（先例：ARQ、Redis、MinIO、Vault） | 中 | 中 | §02 的许可证与维护状态审计纳入 CI；关键依赖都有替代方案 | 上游公告 |
| R10 | 群聊场景的权限语义复杂 | 中 | 中 | 默认私聊发送详情；管理员显式声明群成员权限一致 | 客户需求 |
| R11 | 评测题库的黄金答案本身有错 | 高 | 中 | 双人标注、黄金答案审计、E11 类别跟踪 | E11 占比 |
| R12 | 对客户数据过拟合（为 holdout 调 Ontology） | 中 | 中 | dev/holdout 分离、holdout 轮换、只看汇总 | holdout 与 live 表现背离 |
| R13 | Agent DB 容量/成本超出预期（存储翻倍） | 中 | 低 | 列排除（大对象）、表选择、保留期；容量规划 | 磁盘告警 |
| R14 | Harness 上游变化（Pi 所有权变更后的路线） | 中 | 中 | 版本锁定、适配器契约测试、多 harness 支持 | 上游发布 |
| R15 | MCP 规范快速演进 | 高 | 低 | 跟随官方 SDK；兼容上一个规范版本 | 规范发布 |
| R16 | 按源事务消费吞吐不足或事务路由假设不成立 | 中 | 高 | M0 P11/P12 验证；优化组提交/磁盘溢出；per_table 须显式降级并持续告知跨表偏差 | stream 积压、事务计数不齐、端到端延迟 |

## 3. 需要与首个客户确认的问题

1. 源库清单：类型、版本、是否托管、是否有只读从库、各库的表数与最大表行数、每日变更量。
2. DBA 能否开启 binlog（ROW/FULL/GTID）或逻辑复制；binlog/WAL 保留时长；变更窗口。
3. 模型：可以使用哪些模型服务（外部、私有化）；数据出境与驻留要求。
4. 身份：企业 IdP（飞书、企业微信、钉钉、AD/LDAP、OIDC）；用户组与区域等属性从哪里来。
5. 权限：角色清单与每个角色的数据范围（表、列、行），谁来维护。
6. 使用方：飞书群聊/私聊；是否需要网页端；是否要接企业系统。
7. 新鲜度期望：哪些场景需要分钟级，哪些日级即可。
8. 评测：可以提供哪些报表作为题目来源；谁来确认黄金答案；dev/holdout 的规模。
9. 部署环境：机器规格、是否 K8s、能否运行 gVisor、对象存储、备份策略。
10. 合规：等保级别、审计留存期、个人信息字段清单。
11. 成本：单次会话的模型成本上限、并发会话数预期。

## 4. 设计中的开放问题（内部）

| # | 问题 | 当前倾向 | 何时决定 |
|---|---|---|---|
| Q1 | 场景 schema 由客户写 SQL，还是由 Builder 生成草稿 | 两者都支持：Builder 生成草稿、人审核 | M1 |
| Q2 | 是否在 Knowledge 中包含“未审核”的自动描述 | 列描述可以（标注未审核），其余不可以 | M0 P10 后 |
| Q3 | 指标是否需要编译器 | 先不做（ADR-005），看 E03 占比 | M2 结束 |
| Q4 | Agent 的“记忆”（跨会话记住用户纠正） | 不做隐式记忆；纠正以 `oo gap` 形式进审核，发布后对所有人生效 | M2 |
| Q5 | 多个 OO 角色合并（一个用户多个组） | 不合并，按绑定顺序取第一个；需要时定义组合角色 | M1 |
| Q6 | 是否提供 PG 协议直连（给 BI 工具） | 不提供（门禁与审计难以保证）；需要时通过导出 | 长期 |
