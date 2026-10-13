# 03 捕获方法、位点序与一致性等级

> Status: Proposed
> Last reviewed: 2026-10-09
> 依赖：[04 管线](../04-pipeline.md)、[05 Agent DB](../05-agent-db.md)；Spec §7.2、§8、DEC-06/07/08/15/17
> 对应 Spec：REQ-SRC-09、REQ-SRC-10；AT-SRC-07、AT-SRC-08、AT-SRC-09

## 1. 源端写入分类（零写不变量的可执行版本）

I-SRC-1 要求平台对源库只读。现实中，每种捕获方式在源端都需要一些对象或设置。为了把“只读”变成可审计的判据，按**谁创建、是否写**分类：

| 类别 | 定义 | 例子 | 是否准入 |
|---|---|---|---|
| **R0** | 只读查询，无需任何源端对象 | `SELECT`、`AS OF`/Time Travel 读取、读取 `INFORMATION_SCHEMA`、Iceberg/Delta 元数据读取 | 准入 |
| **R1** | **DBA 手工创建/配置**的对象或参数，平台仅读取；平台不写这些对象 | PG publication、复制槽（由 DBA 创建）、MySQL binlog 设置、SQL Server 在库/表上启用 CDC 或 Change Tracking、Oracle 补充日志与归档模式、Snowflake/BigQuery/Delta 上开启 change tracking/change history 表属性、Mongo 副本集与 `changeStream` 权限 | 准入，必须出现在 DBA 清单并带风险/回滚 |
| **R1w** | DBA 预创建的**平台专用技术辅助对象**，连接器需要**写入它**，不触碰业务表 | Oracle Debezium LogMiner 的 flush 表（上游示例要求 `CREATE TABLE`；是否仅写预创建表待 DBP1 验证）；Debezium 心跳写入表 | **默认禁用**。需最小权限 ADR（对应 DEC-17），单独的账号与对象，源审计显示写入范围仅限该对象；仍不得有业务表写 |
| **R2** | 平台在源库创建对象（表、流、触发器、槽、信号表、存储过程） | Debezium 信号表/增量快照写入、Snowflake `CREATE STREAM`、SAP HANA 触发器复制、Db2 ASN 安装由平台执行 | **不准入** |
| **R3** | 需要对业务表的写权限或会修改业务数据 | 触发器、`UPDATE` 标记已读、删除源数据 | **不准入** |

要点：

1. **R1 与 R1w 的区分由源审计证明**：`experimental` 起，套件对平台身份开启语句级审计（见 [05 §2](05-conformance-and-evidence.md)），非 R0 的语句出现即失败。
2. 云引擎中的“对象”也要分：Snowflake **Stream 是 schema 对象（R2）**，而 `CHANGES` 子句 + DBA 已启用的 `CHANGE_TRACKING`（R1）不需要平台创建对象，因此采用后者。BigQuery 的 `CHANGES`/`APPENDS` 需要表的 `enable_change_history`（R1）；Delta 需要表属性 `delta.enableChangeDataFeed`（R1）。
3. 读取副本时必须验证“副本是否会因平台读取而延迟/冲突”，并在 DBA 清单里提示 `max_standby_streaming_delay` 之类的参数（沿用 DEC-09）。
4. 账号被检出写能力 → 沿用 `allow_write_capable_credential` 的显式审计例外，但**例外不改变零写要求**。

## 2. 捕获方法目录

| 编号 | 方法 | 典型目标 | 删除感知 | 保留窗口风险 | 位点序 | 写入分类 | 一致性等级上限 |
|---|---|---|---|---|---|---|---|
| **C1 日志 CDC** | PG 逻辑解码、MySQL/MariaDB binlog、SQL Server 内置 CDC 表、Oracle LogMiner/OpenLogReplicator、MongoDB change stream、Db2（ASN，受限） | 自建与多数托管变体 | 原生 | binlog/WAL/CDC 清理/oplog 窗口 | 多为 Total | R1（Oracle LogMiner 为 R1w 待审） | **T**（需事务边界） |
| **C2 引擎原生变更读取** | SQL Server Change Tracking；Snowflake `CHANGES`；BigQuery `CHANGES`/`APPENDS`；Delta CDF；Iceberg 快照差 | SQL Server（无法开 CDC 时）、云仓、湖表 | 原生（窗口内）；Iceberg 需处理 delete files | **短**（Snowflake 数据保留，BigQuery 默认 7 天 time travel，Delta 日志/文件保留，Iceberg 默认快照过期 5 天） | Total（版本/时间戳/快照序号） | R1 | **S1**（同一 token 跨表一致读）或 S0 |
| **C3 游标增量** | `updated_at`、SQL Server `rowversion`、序列、自增 ID | 所有支持 SQL 的库 | **无** | 无 | Opaque/游标 | R0 | **B** |
| **C4 分区刷新** | 重读变化的分区并原子替换（BigQuery 分区修改时间、ClickHouse `system.parts`、Doris/StarRocks 分区、Hive/Delta/Iceberg 分区） | OLAP、仓库、湖表 | **分区内自动收敛** | 取决于分区元数据可靠度 | 分区时间/版本 | R0 | S0/B |
| **C5 全量差分** | 周期性整表读取，在 Agent DB 的 shadow 里与现状比较（键 + 行哈希） | 小表、无其他手段的源（如 ES） | 收敛（每周期） | 无 | 无 | R0 | S0/B |

选择规则（`sync: auto` 仅在**已 verified** 的方法中选择，顺序如下）：

```text
C1(满足 T，且写入分类 ≤ R1) ▶ C2(token 可用) ▶ C4(分区元数据可信) ▶ C3(有可靠游标，且 cursor_safety 通过) ▶ C5(体量低于阈值)
```

- 显式配置的方法条件不足 → 报错，不降级（沿用 REQ-SRC-03）。
- 无稳定键、无声明唯一键的表只能 C5（或 C4 全分区重放）；体量超限则拒绝同步该表并说明。
- 同一源内可以按表选择不同方法，但**一致性等级以最弱者**对外呈现（见 §3）。

## 3. 位点序与一致性等级

### 3.1 位点序 `TokenOrder`

| 序 | 含义 | 例子 | 后果 |
|---|---|---|---|
| `Total` | 全局可比较、单调 | PG LSN、Oracle SCN、SQL Server LSN、MySQL GTID 集合（同一 source UUID 内）、MongoDB `clusterTime`、Snowflake/BigQuery 时间戳、Delta 版本（单表）、Iceberg sequence number（单表） | 可做重放去重、修复与一致性声明 |
| `PerPartition` | 仅分区内有序 | DynamoDB shard 内序号、Kafka 分区 offset | 不得声明跨键事务一致性；修复需另行对账 |
| `Opaque` | 只能续读，不可比较 | MongoDB resume token（作为续读用）、云服务 cursor | 不得用于去重/一致性；只作续读凭证 |
| `None` | 无位点 | 游标全量差分 | 仅 B |

现有规则不变：**不可比位点禁止修复与一致性宣称**。MySQL 与 MariaDB 的 GTID 格式不同（`UUID:seq` 与 `domain-server-seq`），故 `PositionCodec` 按家族分实现，且 `binlog 文件名+位置` 仅在同一实例内有效，故障转移后失效，不用作持久位点。

### 3.2 一致性等级（对外可见）

| 等级 | 保证 | 典型来源 |
|---|---|---|
| **T** 事务 | 源事务原子应用；`transactional` 模式下 Agent DB 不出现半事务；水位为跨表最小已应用位点 | PG/MySQL/MariaDB/SQL Server/Oracle 的日志 CDC 且事务元数据可用 |
| **S1** 快照 | 每个读周期在**同一 token** 上读取所有选中表（`AS OF` 同一时间戳/SCN/RR 事务）；周期间无事务保证 | Oracle `AS OF SCN`、Snowflake/BigQuery 同一时间戳、PG/MySQL 的一致性快照事务（≤ 300s 约束沿用 SP04-W06）、TiDB 历史读（待验证） |
| **S0** 单表快照 | 每张表自身一致，表间不保证 | Delta/Iceberg（各表独立版本）、ClickHouse、StarRocks/Doris |
| **B** 尽力 | 游标/差分；可能漏读长事务或最终一致 | 游标增量、ES、DynamoDB 扫描 |

对 Agent 的体现（沿用 DEC-15）：

- `get_freshness` 与查询结果返回每个涉及表的 `consistency_grade` 与 `capture_method`；
- 涉及不同源，或涉及等级低于 T 的多个表的 JOIN，必须返回 `CROSS_TABLE_SKEW` 及各表水位；
- 同一源内全部为 T 或 S1 且同 token 周期，才可声明“同一时点”；否则不得在答案中使用“截至同一时刻”的措辞（由 AGENTS.md 指令与 Gateway 警告共同约束）。

## 4. 保留窗口与位点过期

所有有限保留的方法都需要 `retention_window`（AccessReport 采集，或由配置的 DBA 保证值）：

| 来源 | 保留的事实依据（检索事实，见 [06](06-risks-open-questions-and-sources.md)） |
|---|---|
| MySQL/MariaDB | binlog 保留设置；RDS MySQL 需 `mysql.rds_set_configuration('binlog retention hours', N)`，上限 168 小时 |
| PG | 逻辑槽保留 WAL，槽停摆会撑满源库磁盘（需 `max_slot_wal_keep_size`） |
| SQL Server CDC | cleanup 作业默认保留期；Azure SQL Database 的清理频率不可定制 |
| Snowflake | stream/数据保留延长默认最多 14 天；Time Travel 1–90 天（取决于版本） |
| BigQuery | 变更历史受 time travel 窗口限制，默认 7 天，可缩至 2 天 |
| Delta | 受日志保留（默认 30 天）与 VACUUM（默认 7 天数据文件）限制 |
| Iceberg | 快照过期默认 5 天 |
| MongoDB | oplog 是固定大小的 capped collection；resume token 超出窗口即失效 |
| DynamoDB | Streams 固定 24 小时 |
| ScyllaDB | CDC 日志表行有 TTL，默认 24 小时 |

规则：

1. **健康阈值**：位点滞后超过 `retention_window × 0.5` → `STALE_RISK` 警告并告警；超过 `× 0.8` → 暂停发布新一代数据，保持旧数据可读，通知。
2. **过期处理**：位点不在窗口内 → 状态 `POSITION_EXPIRED`；**不得**从最近位置“继续”。流程：进入 resnapshot（新 generation、shadow 表），完成后原子换表（沿用 SP04-W03）；期间旧 generation 继续按 Agent 当前权限可读，并附 `POSITION_EXPIRED` 质量警告与数据新鲜度。
3. **长周期快照**：有限保留的 `AS OF token` 读取，其总读取时长必须小于 token 保留；计划阶段按估算体量检查，不满足 → 拒绝该模式（沿用 >300s 不静默延长一致快照的规则）。
4. **托管故障转移**：槽/位点是否跨 failover 保留是目标级事实（RDS PG 17 起支持 failover slot 同步；早期版本与 Aurora 的行为待 DBP3 验证）。未证明保留者，failover 后默认进入 `POSITION_EXPIRED` 路径。

## 5. 删除检测

| 模式 | 适用 | 行为 | 披露 |
|---|---|---|---|
| `native` | C1、C2（窗口内） | 事件/变更行含删除；Loader 写墓碑 | 无 |
| `partition_replace` | C4 | 分区内整体替换，缺失行即删除 | 分区外的删除可能在源修改时间未更新时被漏掉，披露 |
| `reconcile` | C3、C5 或无原生删除的源 | 周期性 `read_keys` 读取键集，与 Agent DB 键集比较；源缺失者写墓碑（带 generation 与 fence，沿用 DEC-07） | 表级质量状态 **`DELETES_LAGGED`**：删除可见延迟 ≤ 对账周期；Gateway 查询返回警告；Pack 标注 |
| `none` | 不可检测（如列式引擎无法可靠枚举） | 不删除 | 该表标 `DELETES_UNDETECTED`，Agent 对“当前存在多少”类问题必须声明不确定 |

对账成本：键读取对列式引擎代价低（只扫主键列），对宽行 OLTP 需限速并计入读取预算；频率由配置，默认每日，且不低于 `delete_visibility_sla`。软删除列（`is_deleted`、`DEL_FLAG`）只是业务语义，保留为普通列，**不替代物理删除检测**。

## 6. 游标安全（C3）

增量游标最常见的漏数来源是“语句时间赋值、事务稍后提交”。按引擎分类：

| 分类 | 含义 | 例子 | 要求 |
|---|---|---|---|
| `commit_ordered` | 游标值按提交顺序单调，不会出现晚提交的更小值 | SQL Server `rowversion` 配合 `MIN_ACTIVE_ROWVERSION()` 作上界；Oracle 带 `ROWDEPENDENCIES` 的 `ORA_ROWSCN`（仍可能因未提交范围需上界）；PG 无可用列 | 上界取“当前已提交安全点”，重叠可小 |
| `statement_time` | 值在语句执行时赋值，长事务提交后值更小 | `updated_at`、Oracle 触发器维护的时间戳、MySQL `ON UPDATE CURRENT_TIMESTAMP` | 重叠 `overlap ≥ 源最大事务年龄`（通过活跃事务视图测量，不可测则采用保守配置值），并必须配置周期性对账；套件加入长事务用例 |
| `app_maintained` | 由应用代码维护，可能漏更新 | 业务 `modify_time` | 视为不可信；`sync: auto` 不选；显式选择需管理员确认与更大对账覆盖 |
| `coarse` | 块级/页级粒度 | Oracle `ORA_ROWSCN` 无 ROWDEPENDENCIES 时为块级 | 仅作粗过滤，不能唯一标识变更行；禁止单独作游标 |

固定规则：游标值相等多次变更使用 `(run_generation, cursor, pk)` 检查点与确定性更新（DEC-08）；时间戳精度损失（微秒以下）**不得**用于游标或主键，否则 block。

## 7. 各类捕获的“初始加载配方”

沿用现有“CDC 优先、快照补空”的正确性论证，扩展到其他类别：

| 类别 | 配方 |
|---|---|
| **C1 日志 CDC** | 现行：先启动 CDC 并记录起点 L0；快照按主键分块读取并 fill，已被 CDC 写过的行不覆盖，墓碑防止复活；L_end 完整 applied 屏障后发布（04 §4，DEC-06/07） |
| **C2 变更读取** | **token 锚定快照 + 变更重放**：取 token `t0`；全量以 `AS OF t0` 读取（所有表同一 `t0`，得 S1）；之后读取 `(t0, t1]` 变更并应用，推进到 `t1`。无需“CDC 先行”的 fill 逻辑，因为读取的是同一 token 的静止视图；前提是 `t0` 在整个快照读取期间保持在保留窗口内（§4 规则 3） |
| **C3 游标** | 全量（B）→ 之后按 `(run_generation, cursor, pk)` 增量；旧约束全部保留（SP04-W06） |
| **C4 分区刷新** | 列出分区及其修改标识；每个变化分区在 shadow 中读入并以**单个 Agent DB 事务**替换该分区；分区元数据不可信时退回 C5 |
| **C5 全量差分** | 每周期读取到 shadow；比较键与行哈希，生成插入/更新/删除；超出阈值或读取中源在变化（无快照 token）则标记 B 并披露 |

## 8. 与现有 Loader 契约的对接

- 所有类别都归一为同一 `Batch`（含 `Position`、`txn_sequence`、`quality`）；Loader 的 receipt、墓碑、`sync.rejects`、`_oo.table_quality` 不变。
- 新增的质量状态值：`POSITION_EXPIRED`、`STALE_RISK`、`DELETES_LAGGED`、`DELETES_UNDETECTED`、`PRECISION_LOSS`（列级）、`SOURCE_BUDGET_EXHAUSTED`；这些是 `_oo.table_quality` 的新增枚举值，属于追加式迁移，不改变既有列语义。
- `consistency: transactional` 只对 `grade: T` 的源有效；对其他源，`oo validate` 拒绝该配置（而不是静默当作 `per_table`），避免用户误以为获得事务一致性。
- `PositionCodec` 对 `TokenOrder != Total` 的位点不提供“比较/去重”接口，只提供“续读序列化”。
