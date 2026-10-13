# 数据库支持扩展：统一准入、支持矩阵与逐库适配设计

> Status: Proposed
> Owner: Open Ontology Engineering（数据 / DBA / 安全 reviewer 联合审阅）
> Last reviewed: 2026-10-09
> 性质：对 [03 数据源](../03-sources-and-ingestion.md) §2、§8 数据库路线的**扩展设计**；数据库范围与准入以本目录和 Spec 第 7A 章为准。基线文档 03/04/05/06/07/12/02/14/README/16 已于来源基线 revision 3 按本设计回填（见 [06 §4](06-risks-open-questions-and-sources.md)）。
> 依赖：[03](../03-sources-and-ingestion.md)、[04](../04-pipeline.md)、[05](../05-agent-db.md)、[06 Ontology](../06-ontology-layer.md)、[07 Gateway](../07-gateway.md)、[17](../17-rust-implementation.md)

## 1. 要解决的问题

现行文档在代码与验收上只明确落到 PostgreSQL（13+）与 MySQL 8.x，其余数据库（SQL Server、Oracle、数仓、国产库、文档库）只在 03 §8 里有一张路线表，没有统一的准入标准、没有“哪个版本的哪项能力已验收”的矩阵，也没有逐库的增量、CDC、权限、类型与成本设计。目标是：

1. 让 Open Ontology 能接入市面上绝大多数企业数据库；
2. 让 Self-build、Knowledge Pack、Gateway 这条主链**一份实现服务所有源**；
3. 任何一个数据库“支持”与否都由证据决定，而不是由驱动名或协议兼容声称。

## 2. 核心设计结论

| 编号 | 结论 | 理由（详见） |
|---|---|---|
| D1 | **汇聚点不变**：所有源都归一为 Arrow → PostgreSQL Agent DB。Agent、Gateway、沙盒只面对 PG 方言，永不接触源方言 | 这是下游复用的前提；[02 §1](02-connector-architecture.md) |
| D2 | **准入单位是“目标”**（家族 × 产品 × 版本区间 × 部署变体 × 驱动），按**能力**分别定级，不是“库名 → 支持” | 同名数据库的托管变体、版本、兼容模式差异巨大；[01 §2–3](01-admission-and-support-matrix.md) |
| D3 | 五级准入：`candidate → designed → experimental → verified → certified`；矩阵是唯一真相源，未列出即不支持 | [01](01-admission-and-support-matrix.md) |
| D4 | **源端写入分类 R0–R3**：平台零写不变；DBA 手工创建的只读消费对象（R1）可接受；平台创建对象或写业务表（R2/R3）一律不准入；需要写“技术辅助表”的捕获方式（R1w）默认禁用，须最小权限 ADR | 保持 I-SRC-1；[03 §1](03-change-capture-and-consistency.md) |
| D5 | **捕获方法是目录而非二选一**：日志 CDC、引擎原生变更读取（SQL Server CT、Snowflake CHANGES、BigQuery CHANGES、Delta CDF、Iceberg 快照差）、游标增量、分区刷新、全量差分；各自声明删除检测、保留窗口、位点序与一致性等级 | 数仓、OLAP、文档库没有统一的 binlog；[03 §2](03-change-capture-and-consistency.md) |
| D6 | **一致性等级 T / S1 / S0 / B** 对 Gateway 可见，沿用现有 `CROSS_TABLE_SKEW`、新鲜度与质量警告机制 | 不宣称做不到的事务一致；[03 §3](03-change-capture-and-consistency.md) |
| D7 | **驱动分五类并按进程隔离**：进程内纯 Rust 驱动、外部驱动 worker（ODBC/JDBC/ADBC/厂商原生库）、REST/SDK、Debezium Server、外部 CDC 生产者（仅经 buffer 适配器）。专有驱动默认由客户提供，不由项目分发 | 许可证与供应链；[02 §3](02-connector-architecture.md) |
| D8 | **源语义显式化**：Oracle 空串即 NULL、大小写不敏感排序规则、纳秒精度被截断、约束只声明不强制、列式引擎用默认值代替 NULL，都写入 catalog → Builder → Pack，而不是让 Agent 猜 | [02 §5–6](02-connector-architecture.md) |
| D9 | **读取预算按计量单位**：云数仓按扫描字节/积分/DBU 限额，估算先于读取，取消必须取消云端作业，禁止为保持唤醒而长连接 | [02 §7](02-connector-architecture.md)、[04 §9](04-database-profiles.md) |
| D10 | **“兼容”不可推导**：MySQL/PG 协议兼容不等于 CDC、权限、类型或位点兼容；每个兼容库独立成目标，独立证据 | 研究事实：TiDB 无 MySQL 复制协议、OceanBase/GoldenDB/PolarDB-X 是合成 binlog、openGauss 认证与逻辑解码分叉 |
| D11 | **验收不放宽**：新增数据库复用现有 AT-SRC/AT-PL 套件，再追加 AT-SRC-06–14；没有真实实例的版本不进矩阵 | [05](05-conformance-and-evidence.md) |
| D12 | **分母隔离**：DBX 轨道不改变 87 个工作项与既有 Phase 退出条件；其进度另列分母 | Plan 第 4A 章 |

## 3. 文档导航

| 文档 | 内容 |
|---|---|
| [01 统一准入与支持矩阵](01-admission-and-support-matrix.md) | 目标/能力/等级、晋升与降级、矩阵 schema 与错误码、波次 |
| [02 连接器架构](02-connector-architecture.md) | 复用边界、能力模型、驱动策略与隔离、SourceCatalog 归一、Self-build/Pack/Gateway 接线、读取预算 |
| [03 捕获方法与一致性](03-change-capture-and-consistency.md) | 写入分类、捕获目录、位点序、一致性等级、保留期、删除检测、游标安全、初始加载配方 |
| [04 逐库画像](04-database-profiles.md) | 各家族的体检、最小权限与 DBA 清单、捕获路径、类型映射与陷阱、成本与限制 |
| [05 一致性套件与证据](05-conformance-and-evidence.md) | 套件、夹具与许可、版本矩阵、真实云证据、夜间回归与自动降级 |
| [06 风险、开放问题与来源](06-risks-open-questions-and-sources.md) | DBX 风险登记、需客户/法务确认的问题、本次检索事实与来源、未核实清单 |
| [support-matrix.yaml](support-matrix.yaml) | 机器可读矩阵初稿（当前**无任何目标处于 verified**） |

规范与计划：[Spec 第 7A 章](../../specs/open-ontology-v0.1-spec.md)、[Plan 第 4A 章](../../specs/open-ontology-v0.1-plan.md)。

## 4. 事实与决策的边界

[06 §3](06-risks-open-questions-and-sources.md) 记录了 2026-10-09 的联网检索事实及来源 URL；本目录其余部分是据此做出的**设计决策**，标为 Proposed。凡依赖外部产品行为、但尚未在真实实例上验证的地方，文中写“待验证”并对应 Plan 4A 的 DBP 原型；检索未能确认的内容写“未核实”，不作为事实使用。
