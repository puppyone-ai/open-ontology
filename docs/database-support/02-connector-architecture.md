# 02 连接器架构：复用边界、驱动策略与源语义传递

> Status: Proposed
> Last reviewed: 2026-10-09
> 依赖：[03 §4 连接器接口](../03-sources-and-ingestion.md)、[17 Rust 实施](../17-rust-implementation.md)、[06 Ontology](../06-ontology-layer.md)、[07 Gateway](../07-gateway.md)
> 对应 Spec：REQ-SRC-11、REQ-SRC-12、REQ-SRC-13、REQ-SRC-14；AT-SRC-10、AT-SRC-11、AT-SRC-12、AT-SRC-13

## 1. 复用边界：什么必须不变，什么每个源自己负责

```text
源 A ┐                                              ┌─▶ Self-build (S0–S10)
源 B ├─▶ 连接器 ─▶ Arrow IR ─▶ Loader ─▶ PG Agent DB ─┼─▶ Knowledge Pack
源 C ┘  (每源不同)   (统一)     (统一)    (统一 PG)     └─▶ Gateway ─▶ SDK/CLI/MCP ─▶ 沙盒
```

| 层 | 是否随数据库变化 | 说明 |
|---|---|---|
| Agent DB（PG）、RLS、列级授权、`_oo` 元数据 | 否 | 所有源落成同一套 PG 对象 |
| Loader：COPY、merge、receipt、墓碑、质量状态、发布屏障 | 否 | 只认 `Batch` 与 `Position` 契约，不认源 |
| Gateway：pglast 解析、函数白名单、RLS、审计 | 否 | Agent 只写 PG SQL；**源方言永不进入 Gateway** |
| Knowledge Pack、角色编译 | 否 | 增加“源语义”小节，结构不变 |
| Self-build S1–S10 | 基本不变 | S0 的输入由 `SourceCatalog` 归一；S1 画像在 Agent DB 上做 |
| **连接器（catalog、读取、类型、位点、DBA 清单、体检）** | **是** | 本文档与 [04](04-database-profiles.md) 的范围 |
| 捕获方法与一致性等级 | 是（声明式） | [03](03-change-capture-and-consistency.md) |

推论：新增一个数据库的工程量集中在**连接器 + 逐库证据**，不应该改动 Loader、Gateway、Pack 的安全不变量。若某个数据库逼迫改动这些层（例如需要源方言 SQL 进入 Gateway），说明该目标不应准入，或需要独立 ADR。

## 2. 能力模型（对 03 §4 的扩展）

现有 `Connector` trait 保持不变；扩展点是 `capabilities()` 与 `AccessReport`，两者必须是**结构化声明**，不能用连接器名字符串判断路径。

```rust
// 契约示意：业务类型由 oo-core 定义。
pub enum Capability {
    Catalog, Logic, QueryLog,
    BatchFull, BatchIncremental(CursorKind), ChangeFeed(FeedKind), Cdc(CdcKind),
    DeleteNative, DeleteReconcile,
    SnapshotToken(TokenOrder), ReplicaRead, Budget(BudgetUnit),
}
pub enum CursorKind { Timestamp, RowVersion, Sequence, PartitionModified }
pub enum FeedKind { ChangeTracking, ChangesClause, ChangeDataFeed, SnapshotDiff }
pub enum CdcKind { LogicalDecoding, Binlog, LogMiner, SqlServerCdcTables, ChangeStream, ExternalBuffer }
pub enum TokenOrder { Total, PerPartition, Opaque, None }   // 位点序，见 03 §3
pub enum BudgetUnit { Rows, Bytes, Credits, Dbu, ReadUnits, WallClock }
```

`AccessReport` 在 REQ-SRC-02 基础上追加：

| 字段 | 含义 |
|---|---|
| `target_id`、`level_by_capability` | 与矩阵比对的结果（[01 §4](01-admission-and-support-matrix.md)） |
| `write_class_required` | 每个候选捕获方法所需的源端写入分类（R0–R3，[03 §1](03-change-capture-and-consistency.md)） |
| `driver` | `{kind, name, version, digest}`，驱动出处写入审计 |
| `consistency_grade_by_mode` | 每个可选模式能达到的 T/S1/S0/B |
| `retention_windows` | 各捕获方法的源端保留窗口（binlog/oplog/变更历史/快照） |
| `cost_model` | 计量单位与估算方式（见 §7） |
| `compat_mode` | 兼容模式（如 Oracle/MySQL 模式、sql_mode、字符集） |
| `license_flags` | 此目标上被许可限制的读取项（如需要 Diagnostic Pack 的 AWR） |

未知权限、未知兼容模式、未知计费单位都解释为 `block`，与 REQ-SRC-02 一致。

## 3. 驱动策略

### 3.1 五类驱动路径

| 类别 | 适用 | 隔离 |
|---|---|---|
| **P1 进程内纯 Rust 驱动** | PostgreSQL（sqlx、tokio-postgres）、MySQL/MariaDB（sqlx、mysql_async）、SQL Server（tiberius）、MongoDB（官方 crate）、ClickHouse（官方 HTTP crate） | 与现有 `oo-connectors` 同进程模型；受 `cargo-deny`、`unsafe`/native 审计约束 |
| **P2 外部驱动 worker** | 需要专有/原生库或 ODBC/JDBC 的目标：Oracle（ODPI-C 或官方纯 Rust 驱动，二选一经原型）、Db2、SAP HANA、达梦、神通、openGauss 的私有认证；ADBC 驱动（Go/C 共享库，如 Snowflake、BigQuery） | 独立容器/进程，沿用 03 §3.1 与 17 §4 的“版本化 JSON 控制协议 + Arrow IPC 数据流”；镜像 digest 锁定；**无 Agent DB 凭证**，只拿到该源的 `secret://` 解析后的凭证与到该源的网络路径 |
| **P3 REST/SDK 客户端** | BigQuery（Storage Read API）、Redshift Data API、Databricks Statement Execution API、DynamoDB SDK、Snowflake SQL API | 进程内或 worker；出口只允许目标云 API 域名；凭证短期化 |
| **P4 Debezium Server** | 日志 CDC：PG、MySQL、MariaDB（独立连接器）、SQL Server、Oracle（经 DBX 例外）、Db2（受限）、MongoDB | 每源一个容器，沿用 ADR-002；禁用所有写入型选项（见 §3.3） |
| **P5 外部 CDC 生产者** | TiCDC、OceanBase OBLogProxy/Binlog 服务、PolarDB-X 全局 binlog、Flink CDC | **不在 v0.1 内**；经 SP15-W03 的 buffer 适配器（Kafka）接入，且需证明事务路由与回放等价 |

### 3.2 专有驱动与再分发

- 默认策略：**项目不分发专有驱动**。P2 worker 镜像不含厂商驱动；客户通过只读卷挂载（或自建基础镜像），AccessReport 记录驱动 digest；缺失返回 `DRIVER_MISSING`。
- 若法务确认某驱动可再分发（例如 Oracle Instant Client 的 OTN 条款、Oracle JDBC 的 FUTC 条款需逐项确认），可做成**可选的、单独签名的 worker 镜像**，附 NOTICE，仍不进入默认镜像。
- Microsoft ODBC Driver、IBM Db2 驱动、SAP HANA 客户端的再分发条款本次检索未核实，按“客户提供”处理。
- 许可证写入矩阵的 `driver.license` 字段与 SBOM。

### 3.3 Debezium 使用约束（所有 P4 目标共同）

1. 只用于 CDC；初始快照由 OO 自己完成（CDC-first 快照，沿用 04 §4）。**禁用** Debezium 增量快照和信号表（Oracle、SQL Server、Db2 的增量快照要求可写信号表；仅 PG/MySQL/MariaDB 的只读增量快照可不写源，但仍不使用，以保持单一路径）。
2. 禁用心跳写入（`heartbeat.action.query` 等）与任何自动建表选项；空闲位点推进使用只读 position-complete 信号（见 04 SP04-W01 的约束）。
3. 镜像 digest 与 offset/history 版本锁定；连接器稳定性分级（stable / incubating）写入矩阵，`incubating` 连接器只能到 `experimental`。
4. 启用的连接器账号权限由本项目的**最小权限清单**决定，不直接采用上游文档的示例 GRANT（例如上游 Oracle 示例含 `CREATE TABLE`、`LOCK ANY TABLE`、`SELECT ANY TABLE`，与零写及最小权限冲突，见 [04 §5](04-database-profiles.md)）。

### 3.4 Rust 驱动现状（检索事实，非决策）

事实与来源见 [06 §3.4](06-risks-open-questions-and-sources.md)。设计依据：

- sqlx 已移除 MSSQL 驱动，不支持 Oracle；SQL Server 使用 tiberius。
- tokio-postgres 提供 binary COPY；逻辑复制需另行封装，但本项目 PG 的 CDC 走 Debezium，不依赖。
- mysql_async 有 binlog 特性，但同样不是本项目 CDC 路径，仅作备选。
- Oracle 有 ODPI-C 绑定、官方新纯 Rust 驱动与第三方纯 Rust 实现，成熟度需 DBP1 原型验证后再选。
- Snowflake/BigQuery 的 Rust 官方支持不齐，ADBC 或 REST 作为路径；Databricks 无官方 Rust SQL 驱动，Delta 表可经 `delta-rs` 直接读对象存储（绕过计算单元计费，但需对象存储凭证与表格式治理，另评）。

## 4. 进程与凭证模型

```text
control plane ──plan/apply──▶ source worker（P1 在进程内；P2 独立容器）
                                  │ 只读凭证（SecretRef 解析）
                                  ▼
                              源库/云 API
                                  │ Arrow IPC / 控制协议（mTLS 或 Unix socket）
                                  ▼
                               Loader ──▶ Agent DB（唯一写者）
```

- worker **不持有** Agent DB、Gateway、模型凭证；Loader 是 Agent DB 的唯一写者（I-DB 不变量不变）。
- worker 的网络出口只到该源的主机/云 API 域名；云仓使用服务账号的短期令牌或工作负载身份，不使用长期个人令牌。
- worker 受 CPU/内存/行数/字节/时长预算约束，并支持 `CancellationToken`；取消必须传递到源端（会话 kill、云作业取消，见 §7）。
- worker 崩溃不影响 Agent DB 已发布数据；由 jobs 租约与 generation 机制恢复（SP04-W05）。

## 5. `SourceCatalog` 归一模型（Self-build S0 的输入）

连接器把各家 catalog 归一成同一结构，标记**可信度**与**方言**，使 Builder 不必知道各库系统视图：

| 字段 | 内容 | 来源差异示例 |
|---|---|---|
| `objects[]` | 表/视图/物化视图/外部表，所属 schema、`kind` | Oracle `ALL_OBJECTS`、SQL Server `sys.objects`、Db2 `SYSCAT`、HANA 系统视图、MySQL/PG/Snowflake `INFORMATION_SCHEMA` |
| `columns[]` | 原生类型、精度/标度、可空、默认值、生成列、注释、`precision_loss`、`semantic_caveats[]` | 注释来源：Oracle `COMMENT ON`、SQL Server 扩展属性、BigQuery description、Snowflake COMMENT、Db2 REMARKS |
| `keys[]` | PK/UNIQUE/FK + **`constraint_trust: enforced \| declared_only \| absent`** | Snowflake、BigQuery、Redshift、Synapse、Databricks 的约束仅声明、不强制 → `declared_only` |
| `physical_hints[]` | 分区键、聚簇键、排序键（ClickHouse `ORDER BY`、BigQuery cluster/partition、Delta partition、Doris/StarRocks key 模型） | 作为 join/grain 的**候选证据**，不是关系事实 |
| `logic[]` | 视图/存储过程正文，附 `dialect` 与权限状态 | 无权限读取正文 → 记录为 `logic_unreadable`，不静默跳过 |
| `query_log` | 可选：归一后的语句、耗时、次数；字面量先掩码 | Oracle V$SQL/AWR（AWR 需 Diagnostic Pack 许可）、SQL Server Query Store、MySQL performance_schema 摘要、PG `pg_stat_statements`、Snowflake `QUERY_HISTORY`、BigQuery `JOBS`、Redshift `SYS_QUERY_HISTORY`、ClickHouse `system.query_log` |
| `stats` | 行数与基数估计（仅当不增加源负载） | 源端统计只在“免费”时读取；画像默认在 Agent DB 上做 |

声明约束的处理：`declared_only` 的 PK/FK 不能当作强关系证据，S3 必须做包含依赖检验（沿用 ≥0.98 门槛）；`enforced` 的 FK 才可作为确定事实。

## 6. 源语义如何穿过 Self-build、Pack、Gateway

### 6.1 `semantic_caveats` 清单（连接器在 type_map 时生成）

| caveat | 触发 | 对 Agent 的后果 |
|---|---|---|
| `empty_string_is_null` | Oracle（VARCHAR2/CHAR 空串即 NULL）；Oracle 兼容模式的国产库待验证 | 在 Agent DB 中为 NULL；过滤“空”需用 `IS NULL`，不能与 `''` 比较 |
| `source_collation_ci` / `source_accent_insensitive` | SQL Server `CI_AS`、MySQL `_ci`、Oracle `NLS_COMP` 设置等 | PG 默认区分大小写；Pack 提示用 `lower()` / `search_values`；`search_values` 按此规范化 |
| `trailing_space_insensitive` | SQL Server、MySQL PAD SPACE 排序规则 | `'a' = 'a '` 在源为真、在 PG 为假；Pack 警告 |
| `precision_loss` | `datetime2(7)`、`time(7)`、`datetimeoffset(7)`、Oracle `TIMESTAMP(7–9)`、Snowflake `TIME/TIMESTAMP(9)`、ClickHouse `DateTime64(7–9)`、Db2 `TIMESTAMP(12)` → PG 最大微秒 | 列级声明；若该列是 PK、游标或唯一键 → **block**（会破坏幂等与去重），除非保留 `<col>__raw` 文本列作键 |
| `offset_lost` | `datetimeoffset`、Snowflake `TIMESTAMP_TZ` → `timestamptz`（保留瞬间，不保留原偏移） | Pack 提示原偏移不可得 |
| `default_for_missing` | ClickHouse 非 Nullable 列：缺失值以 0/'' 等默认值出现，无法区分 NULL | Pack 警告：0/空串可能是“缺失” |
| `synthetic_binlog` | OceanBase、PolarDB-X、GoldenDB 的兼容 binlog | 影响 CDC 位点/事务语义，非 Agent 可见；写入质量元数据 |
| `unenforced_constraints` | 数仓约束仅声明 | 唯一性/外键可能被违反；Pack 提示去重 |
| `soft_delete_hint` | 画像发现 `is_deleted/del_flag` 等 | 与源真实删除不同；作 Pack 提示，不改变同步删除语义 |
| `append_only_source` / `eventual_consistency` | ClickHouse ReplacingMergeTree、部分 OLAP | 删除/更新延迟可见；声明 `DELETES_LAGGED` 的前置事实 |
| `lossy_type_fallback` | 无法映射的类型落到 `text`/`jsonb`，或排除列 | 列描述中写明来源类型 |

### 6.2 各阶段的接线

| 阶段 | 变化 |
|---|---|
| S0 采集 | 读 `SourceCatalog`；视图/过程正文带 `dialect`；用 `sqlglot` 仅做**翻译提示**（`translated_unverified`），不用于授权、不执行；`logic_unreadable` 与 `query_log` 许可缺失写入证据缺口，影响置信度 |
| S1 画像 | 默认在 Agent DB（`pg_stats` + 有界抽样）上进行，不对源增加负载；源端仅做对账所需的行数/校验和 |
| S2 语义类型 | 规则字典增加 `native_type_semantics`（Oracle `NUMBER(1)`/`CHAR(1)` Y/N 标志、SQL Server `bit`、`rowversion` 视为系统列、Mongo `_id`、Snowflake `VARIANT`…） |
| S3 关系 | 采用 `constraint_trust`；`physical_hints` 与查询日志 JOIN 作为佐证；约束声明但未强制的关系需包含依赖检验 |
| S4 码表 | 同前；源的 `COMMENT`/`MS_Description`/REMARKS 解析规则按家族扩展 |
| S5 起草 | 提示词输入 `semantic_caveats`；要求描述中不得写出源方言 SQL 作为示例 |
| S8 候选示例 | 只在 Agent DB 经 Gateway（PG 方言）验证；源方言视图改写出的示例必须重写并通过验证后才可入审核队列 |
| Pack | 每表页新增 “来源与语义”：`source_kind`、`consistency_grade`、`delete_detection`、caveats；顶层 README 固定声明“所有 SQL 为 PostgreSQL 方言，无论数据来自何种源库”，并可选附一页方言对照（如 `NVL`→`COALESCE`、`TOP n`→`LIMIT n`） |
| Gateway | 解析与授权逻辑不变；`get_freshness` 增加 `consistency_grade`、`delete_detection`、`capture_method`；`DELETES_LAGGED`、`POSITION_EXPIRED`、`PRECISION_LOSS` 作为表级质量状态沿用警告通道 |
| OOS 契约 | 生成层 `datasets[].source_semantics`（`family/product/version/capture/grade/caveats[]`）作为**可选追加字段**；这是 `oos/v1` 的契约变更，须由 F0 契约 owner 先合入版本化变更（Plan 2.6），不得各组件本地扩展 |

### 6.3 方言与标识符

- Agent DB 中的标识符沿用 catalog 的规范名映射与碰撞处理（SP03-W05）。Oracle/Db2/HANA 默认大写标识符，映射后保留原名并在 Pack 标明，避免 Agent 依赖引号大小写。
- 任何源方言片段只出现在 Evidence 与 Pack 的“来源”描述中，**永不**进入 Agent 可执行的 SQL 示例。

### 6.4 文档/半结构化源的关系化映射

适用于 MongoDB、DynamoDB、Elasticsearch/OpenSearch、Cassandra/ScyllaDB（CQL 有强类型，较简单）：

1. **每个集合/表一张主表**：`_id`（或主键）为 PK；`doc jsonb` 保存完整文档；稳定字段投影为类型化列。
2. **稳定字段推断**：采样（含最近变更窗口）统计字段出现率与类型一致率；只有达到阈值（初始：出现率 ≥ 0.9 且类型一致率 ≥ 0.99，待 DBP6 标定）的标量字段才投影；类型混用的字段只留在 `doc`。
3. **新增字段 = 新增列**，走“新列默认不可见 + 分类”流程（沿用 I-DB-5）；`doc` 的可见性等同其最敏感字段，默认只授权给能见全部字段的角色，或不授权，由角色 Pack 使用投影列。
4. **数组**：默认保持在 `jsonb`；对象数组拆子表是可选的 Builder 建议，经人审后启用，子表主键 `(parent_id, ordinal)`。
5. **类型保真**：BSON ObjectId→`text`（24 位 hex）、Decimal128→`numeric`（NaN/Infinity 作 reject）、Int64→`bigint`、Date→`timestamptz`（毫秒）、BinData→`bytea`（UUID 子类型→`uuid`）；`doc` 内的 Int64/Decimal128 用字符串或扩展 JSON 保真，避免经浮点。
6. **删除**：change stream 的 delete 事件仅带主键（除非启用 pre-image，6.0+），Loader 据主键删除；无原生变更流的源（ES/OpenSearch）用 `delete_reconcile`。
7. **无事务语义的源**（DynamoDB、ES）的一致性等级不超过 B；跨表 JOIN 触发 `CROSS_TABLE_SKEW` 警告。

## 7. 读取预算与成本控制

| 源类型 | 计量 | 估算（先于读取） | 强制手段 | 取消 |
|---|---|---|---|---|
| 自建 OLTP | 行数/字节/连接/语句时长 | 目录统计，不当作精确值（DEC-16） | 令牌桶、`statement_timeout` 类设置、读副本 | 终止源会话并确认退出 |
| BigQuery | 扫描字节（按需计费） | **dry run** 返回字节估算 | `maximum_bytes_billed`、项目/用户日配额、专用 reservation | 取消 job；核对无残留 job |
| Snowflake | warehouse 积分（按秒计，唤醒最低计 60 秒） | 无精确估算；用历史基线 + 行/字节上限 | 专用小 warehouse、`AUTO_SUSPEND`、Resource Monitor、`STATEMENT_TIMEOUT_IN_SECONDS` | 取消语句；不保持空闲会话以免延长计费 |
| Databricks | DBU（SQL Warehouse） | 同上 | 专用小 warehouse、自动停止、budget policy | 取消 statement |
| Redshift | 节点时/RPU 秒；并发扩展计费 | 同上 | WLM 队列 + QMR + `statement_timeout`；读大表优先 `UNLOAD` 到客户指定桶（写客户 S3，不写源表，R1，需 DBA 授权） | 取消查询 |
| ClickHouse | 行/字节 | quota 与 `system.parts` | `readonly` profile + quota + `max_execution_time` | kill query |
| DynamoDB | 读容量单位 | `DescribeTable` 项数/大小估算 | 限速并行 Scan、按需模式需额外上限 | 停止分页 |
| 对象存储表（Delta/Iceberg 直读） | GET 请求与字节 | manifest 统计 | 并发/字节上限 | 停止读取 |

通用规则：

- 计量单位**写入 AccessReport.cost_model**；未知单位 → block。
- `estimate()` 返回 `{unit, value, confidence}`；`confidence=unknown` 时只允许在管理员给定的硬上限内执行。
- 预算耗尽 → 暂停该源的读取并标记 `SOURCE_BUDGET_EXHAUSTED`，不静默跳过也不无限重试。
- 同步调度合并：为避免多次唤醒云仓，同一源的表按窗口合并执行；会话读取结束立即释放连接。

## 8. 配置示例

```yaml
# sources/oracle_erp.yaml  —— 实验目标，仅示意
sources:
  oracle_erp:
    connector: oracle
    target: oracle-19c-selfmanaged       # 必须在矩阵中存在，且所请求能力达到等级
    driver: { worker: oo-worker-oracle, volume: /opt/vendor/oracle-instantclient }   # 客户提供
    connection: { host: db1.corp, port: 1521, service: ERP }
    credential: secret://oracle_erp
    read_from: replica
    sync: incremental                    # 显式；不使用 auto 选 CDC
    incremental: { cursor: LAST_UPDATE_DATE, every: 1h, overlap: 30m }
    delete_detection: { mode: reconcile, every: 24h }
    timezone: Asia/Shanghai
    type_policy: { number_unconstrained: numeric, time_precision_overflow: truncate }
    limits: { max_connections: 2, max_rows_per_sec: 20000 }

# sources/dw_snowflake.yaml
sources:
  dw_snowflake:
    connector: snowflake
    target: snowflake-saas
    connection: { account: ORG-ACCT, warehouse: OO_READ_XS, role: OO_READER }
    credential: secret://snowflake_oo
    sync: change_feed                    # CHANGES 子句，需 DBA 已启用 CHANGE_TRACKING
    budget: { unit: credits, per_day: 2.0, statement_timeout: 600s, auto_suspend: 60s }
    tables: { include: ["ANALYTICS.CORE.*"] }
```

两个示例的 `target` 必须解析到矩阵；没有证据的目标在生产 profile 下由 `oo validate` 拒绝。

## 9. 错误与可观测

连接器错误使用既有脱敏 `Result`；新增分类 `CONNECTOR_UNSUPPORTED`、`TARGET_VERSION_UNVERIFIED`、`CAPABILITY_UNVERIFIED`、`COMPAT_MODE_UNKNOWN`、`DRIVER_MISSING`、`POSITION_EXPIRED`、`SOURCE_BUDGET_EXHAUSTED`、`SOURCE_CAPABILITY_BLOCKED`。指标按 `target_id` 打标签：读取字节/行、取消延迟、重试、位点滞后、保留窗口剩余、预算消耗、驱动版本。
