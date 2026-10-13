# 06 风险、开放问题与检索来源

> Status: Proposed
> Last reviewed: 2026-10-09
> 本文件区分三类内容：§1 风险（设计判断）、§2 开放问题（需外部确认）、§3 检索事实（带来源）。§3 以外的设计内容不得被当作已核实的外部事实。

## 1. DBX 风险登记

| ID | 风险 | 影响 | 缓解 |
|---|---|---|---|
| DBX-R1 | 把“协议兼容”当“能力兼容”：兼容库的 CDC、权限、类型、位点实际不同 | 数据错误或遗漏，且难以察觉 | 每个兼容库独立目标与证据；套件要求真实实例；矩阵不继承（[01 §6](01-admission-and-support-matrix.md)） |
| DBX-R2 | 第三方捕获组件要求在源库写对象（Oracle LogMiner 辅助表、Debezium 信号/心跳、Db2 ASN、Snowflake Stream、HANA 触发器） | 破坏零写承诺，DBA 与合规拒绝 | R0–R3 分类；R2/R3 不准入；R1w 默认禁用并走 ADR；源端审计证明（[03 §1](03-change-capture-and-consistency.md)） |
| DBX-R3 | 有限保留窗口过期造成静默漏数 | 数据缺失 | 窗口健康阈值、`POSITION_EXPIRED` 强制重快照、新旧 generation 切换（[03 §4](03-change-capture-and-consistency.md)） |
| DBX-R4 | 游标增量在长事务下漏读；删除不可见 | 数据缺失、幽灵行 | `cursor_safety` 分类、overlap ≥ 事务年龄、对账、`DELETES_LAGGED` 披露（[03 §5–6](03-change-capture-and-consistency.md)） |
| DBX-R5 | 云数仓读取产生意外费用或长期唤醒 | 成本事故 | 计量单位、dry run/硬上限、专用小型 warehouse、调度合并、取消传到云端（[02 §7](02-connector-architecture.md)） |
| DBX-R6 | 专有驱动许可不允许再分发，或客户环境缺驱动 | 无法交付 | 默认客户提供驱动卷；`DRIVER_MISSING` 明确拒绝；法务逐目标确认 |
| DBX-R7 | native/JVM 驱动扩大攻击面与供应链风险 | 漏洞、崩溃影响主进程 | P2 独立 worker、无 Agent DB 凭证、digest 锁定、SBOM、无 unsafe 进入核心 crate |
| DBX-R8 | 源语义（空串=NULL、CI 排序、尾空格、精度、缺失默认值）被 Agent 忽略，答案静默出错 | 错误结论 | `semantic_caveats` 进入 catalog→Builder→Pack→freshness；评测轨道 D 加入对应用例（见 §2 Q9） |
| DBX-R9 | 夹具不可得或云账号成本高，导致“声称支持”大于“证明支持” | 虚假支持声明 | 证据类型分开标注；无证据不进矩阵；成本预算与“本周期未验证”标识 |
| DBX-R10 | 目标数量膨胀，维护与回归成本超过团队能力 | 质量下降 | 波次与晋升闸门；降级自动化；社区贡献走同一套件；`experimental` 不进入默认发布承诺 |
| DBX-R11 | 修改 `oos/v1` 追加字段或 `_oo.table_quality` 枚举影响已发布契约与评测 | 兼容性问题 | 追加式、可选字段；F0 契约 owner 先合并；版本化迁移 |
| DBX-R12 | 本目录与基线文档不一致 | 实施者读到两套说法 | 已于 revision 3 回填（§4）；冲突时以本目录与 Spec 第 7A 章为准 |

## 2. 开放问题

| ID | 问题 | 需要谁确认 | 默认选择（确认前） |
|---|---|---|---|
| Q1 | 首个客户及后续客户实际使用哪些数据库、版本与托管形态？ | 客户 / 业务 owner | 波次 0–1 |
| Q2 | Oracle 驱动路线：ODPI-C 绑定、官方纯 Rust 驱动，或其他；Instant Client 是否可再分发 | 数据 owner + 法务 | 客户提供驱动卷 |
| Q3 | Oracle LogMiner 辅助表的实际写入范围能否收敛为单一预创建表（DBP1）；客户是否接受 R1w 例外 | 安全 + DBA | CDC 关闭，仅批量 |
| Q4 | Debezium 在各目标上是否能完全禁用写入（心跳、信号）并保持空闲位点推进（DBP2/DBP3） | Pipeline owner | 仅用只读 position-complete 信号 |
| Q5 | Aurora 与托管变体故障转移后位点/槽的保留与读副本回放屏障证明（DBP3） | 数据 + DBA | 视为 `POSITION_EXPIRED` 路径 |
| Q6 | 云仓的真实账号、预算与受保护 CI 环境由谁提供 | 运维 owner / 客户 | 证据类型 `managed_cloud` 缺失则不 verified |
| Q7 | 国产库（达梦、神通、金仓、瀚高、GoldenDB、TDSQL）能否取得夹具许可与官方文档，兼容模式如何选择 | 业务 / 厂商 | 仅 `experimental`，客户环境验证 |
| Q8 | TiDB、OceanBase 等的 CDC（TiCDC、OBLogProxy）是否在 v0.x 需要，以及 Kafka buffer 适配器（SP15-W03）是否前移 | 产品 owner | 批量先行 |
| Q9 | 评测轨道 D 如何加入“源语义”用例（空串/大小写/尾空格/精度）并度量 Pack 的覆盖效果 | 评测 owner | 先把 caveat 注入 Pack，评测另立卡 |
| Q10 | 基线 revision 3 的回填已执行；是否还需对 08–11、13 做一致性复核 | 规范 owner | 已批准，复核另议 |
| Q11 | 文档/半结构化源的稳定字段阈值（出现率、类型一致率）与新字段可见性策略（DBP6 标定） | 数据 owner | 初始 0.9/0.99，`doc` 默认不授权 |
| Q12 | Snowflake/BigQuery 的变更读取窗口小于初始快照耗时时的策略（分段快照？拒绝？） | Pipeline owner | 拒绝该模式并给出体量建议 |

## 3. 检索事实与来源（2026-10-09，AnySearch）

> 这些是子检索员的原始事实摘录与来源，**未经本项目在真实实例上验证**。设计依赖它们的地方已标“待验证”。每条以官方文档优先；出现 “未核实” 的内容不作为设计依据。

### 3.1 关系型源（CDC / 权限 / 版本）

- Debezium 3.7 测试矩阵：Oracle 19c/21c/23ai/26ai 与 OpenLogReplicator 1.3.x–1.9.x；SQL Server 2017/2019/2022；MariaDB 11.4/11.8/12.3；Db2 11.5；MariaDB 连接器自 2.7 独立，2.7.3 起撤销用 MySQL 连接器连 MariaDB。来源：https://debezium.io/releases/ 、https://debezium.io/releases/2.7/
- Debezium Oracle：适配器 LogMiner / XStream / OLR；XStream 需要 GoldenGate 许可（无需安装 GoldenGate）。来源：https://debezium.io/documentation/reference/stable/connectors/oracle.html
- Debezium Oracle 示例 GRANT 含 `CREATE TABLE`、`CREATE SEQUENCE`、`LOCK ANY TABLE`、`SELECT ANY TABLE`、`FLASHBACK ANY TABLE`、`LOGMINING` 等；LogMiner 需 ARCHIVELOG 与补充日志。来源：https://debezium.io/blog/2022/09/30/debezium-oracle-series-part-1/
- Debezium 对 Oracle/SQL Server/Db2 的增量快照需要可写信号表；MariaDB/MySQL/PostgreSQL 可配置只读增量快照。来源：https://debezium.io/documentation/reference/stable/connectors/sqlserver.html
- Oracle 生命周期：19c Premier 至 2029-12-31、Extended 至 2032-12-31；23ai Premier 至 2031-12-31；21c 为创新版本。来源：https://dbvisit.com/blog/oracle-support-extended-for-19c-and-23ai 、https://mikedietrichde.com/2024/11/27/premier-support-end-date-for-oracle-database-23ai-has-been-adjusted/ 、https://endoflife.date/oracle-database
- `ORA_ROWSCN` 在无 `ROWDEPENDENCIES` 时为块级，不可靠且无法检测删除。来源：https://docs.oracle.com/en/database/oracle/oracle-database/26/sqlrf/ORA_ROWSCN-Pseudocolumn.html
- Oracle：`NUMBER` 无精度在部分 CDC 路径被表示为可变标度十进制；空串即 NULL。来源：https://support.matillion.com/s/article/Streaming-Oracle-numeric-columns-without-precision-scale-replicate-as-Base64-variant 、https://stackoverflow.com/questions/77461130/
- SQL Server CDC：`sp_cdc_enable_db`/`sp_cdc_enable_table`，创建 cdc schema、变更表与 Agent 作业，需 sysadmin；2016 SP1 起 Standard 版可用；Azure SQL Database 使用内置 scheduler（约 20 秒捕获、每小时清理）；RDS 使用 `msdb.dbo.rds_cdc_enable_db`；CDC 不能在 AlwaysOn 副本上运行捕获。来源：https://learn.microsoft.com/en-us/sql/relational-databases/track-changes/enable-and-disable-change-data-capture-sql-server 、https://learn.microsoft.com/en-us/azure/azure-sql/database/change-data-capture-overview 、https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/Appendix.SQLServer.CommonDBATasks.CDC.html 、https://debezium.io/documentation/reference/stable/connectors/sqlserver.html
- SQL Server Change Tracking：仅记录主键与操作，有保留窗口，可检测删除。来源：https://learn.microsoft.com/en-us/sql/relational-databases/track-changes/about-change-tracking-sql-server
- SQL Server CDC 已知限制（排序规则、在线 DDL、新增带默认值列显示 NULL）。来源：https://learn.microsoft.com/en-us/sql/relational-databases/track-changes/known-issues-and-errors-change-data-capture
- Db2 连接器基于 ASN SQL Replication，需源库创建 ASN 对象并运行 capture，使用需 IBM InfoSphere Data Replication 许可；z/OS 6 字节 LSN 不支持；LOB 序列化存在已知缺陷。来源：https://debezium.io/documentation/reference/stable/connectors/db2.html 、https://github.com/debezium/dbz/issues/2304
- Db2 11.5 EOS 2027-04-30。来源：https://www.ibm.com/docs/en/db2/11.5.x?topic=database-db2-eos-dates
- SAP HANA 无 Debezium 连接器；CDC 路径为 SDI/SLT 触发器复制或厂商专有 log-based。来源：https://debezium.io/releases/ 、https://help.sap.com/docs/HANA_SMART_DATA_INTEGRATION/7952ef28a6914997abc01745fef1b607/912ce905be2343119cf5c8e33995dd4d.html
- MariaDB JSON 为 `LONGTEXT` 别名；GTID 格式为 `domain-server-sequence`。来源：https://mariadb.com/docs/server/reference/data-types/string-data-types/json 、https://debezium.io/documentation/reference/stable/connectors/mariadb.html
- MariaDB 生命周期：10.6 EOL 2026-07-06；11.4 LTS 至 2029-05-29。来源：https://mariadb.org/mariadb-server-10-6-reaches-end-of-life-on-july-6th/ 、https://endoflife.date/mariadb
- RDS PG：`rds.logical_replication=1`；可用 `pgoutput`，不可安装 `decoderbufs`；17 起 failover slot 同步到读副本。来源：https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/PostgreSQL.Concepts.General.FeatureSupport.LogicalReplication.html 、https://repost.aws/articles/AR0O7O8ZTgQ0K7yIeCDJJKWg/managing-logical-slot-synchronization-for-rds-for-postgresql-starting-version-17-after-promoting-the-read-replica
- Cloud SQL PG `cloudsql.logical_decoding`、AlloyDB `alloydb.logical_decoding`。来源：https://docs.cloud.google.com/sql/docs/postgres/replication/configure-logical-replication 、https://docs.estuary.dev/reference/Connectors/capture-connectors/alloydb/
- RDS MySQL binlog 保留：`mysql.rds_set_configuration('binlog retention hours', N)`，MySQL 上限 168 小时。来源：https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/mysql-stored-proc-configuring.html
- Aurora MySQL binlog 仅 writer 可读；开启开销最高约 50%，增强 binlog 约 13%。来源：https://docs.aws.amazon.com/AmazonRDS/latest/AuroraUserGuide/USER_LogAccess.MySQL.BinaryFormat.html 、https://docs.aws.amazon.com/AmazonRDS/latest/AuroraUserGuide/AuroraMySQL.Enhanced.binlog.html
- PolarDB MySQL binlog 默认关闭；Cloud SQL MySQL 启用 binlog 依赖 PITR。来源：https://help.aliyun.com/en/polardb/polardb-for-mysql/user-guide/enable-binary-logging 、https://docs.cloud.google.com/sql/docs/mysql/backup-recovery/configure-pitr

### 3.2 云数仓 / 湖表 / OLAP

- Snowflake：Streams 延长保留默认最多 14 天；`CHANGES` 子句 + `CHANGE_TRACKING`；Time Travel 0–90 天；约束仅 NOT NULL 强制；warehouse 最低计费 60 秒。来源：https://docs.snowflake.com/en/user-guide/streams-intro 、https://docs.snowflake.com/en/sql-reference/constructs/changes 、https://docs.snowflake.com/en/user-guide/data-time-travel 、https://docs.snowflake.com/en/sql-reference/constraints 、https://docs.snowflake.com/en/user-guide/warehouses-overview
- Snowflake ADBC 驱动（Go）与 Rust crate `adbc_snowflake`。来源：https://arrow.apache.org/adbc/20/driver/snowflake.html 、https://crates.io/crates/adbc_snowflake
- BigQuery：`CHANGES`/`APPENDS`，需 `enable_change_history`，绑定 time travel（默认 7 天）；约束 NOT ENFORCED；按扫描字节计费；`maximum_bytes_billed`；Storage Read API。来源：https://docs.cloud.google.com/bigquery/docs/change-history 、https://docs.cloud.google.com/bigquery/docs/primary-foreign-keys 、https://cloud.google.com/bigquery/pricing 、https://cloud.google.com/bigquery/docs/reference/storage
- Redshift：无原生行级 CDC，约束为信息性；`UNLOAD` 并行导出。来源：https://docs.aws.amazon.com/redshift/latest/dg/t_Defining_constraints.html 、https://docs.aws.amazon.com/redshift/latest/dg/r_UNLOAD.html
- Databricks Delta CDF 需 `delta.enableChangeDataFeed`，受日志/VACUUM 保留约束；Unity Catalog 最小读为 `USE CATALOG`+`USE SCHEMA`+`SELECT`；`delta-rs` 可直读 Delta。来源：https://docs.databricks.com/aws/en/tables/features/change-data-feed 、https://docs.databricks.com/aws/en/data-governance/unity-catalog/access-control/privileges-reference 、https://github.com/delta-io/delta-rs
- ClickHouse：无多语句事务；`ReplacingMergeTree` 后台去重；轻量删除异步；官方 Rust HTTP 客户端。来源：https://clickhouse.com/docs/reference/engines/table-engines/mergetree-family/replacingmergetree 、https://clickhouse.com/docs/concepts/features/operations/delete/lightweight-delete 、https://clickhouse.com/docs/integrations/language-clients/rust
- StarRocks / Doris：MySQL 协议；未发现对外变更流（Doris 5.x Table Stream 为文档标注未发布的实验特性）。来源：https://docs.starrocks.io/docs/table_design/table_types/primary_key_table/ 、https://doris.apache.org/docs/dev/data-operate/incremental/incremental-query/
- Iceberg：快照过期默认 5 天；`iceberg-rust` 不支持跨快照增量读（issue #2152）。来源：https://iceberg.apache.org/docs/latest/configuration/ 、https://github.com/apache/iceberg-rust/issues/2152
- 业界 ETL 对数仓源的增量策略为游标/分区级，删除检测为缺口。来源：https://docs.airbyte.com/integrations/sources/snowflake 、https://github.com/dlt-hub/dlt 、https://github.com/slingdata-io/sling-cli

### 3.3 文档、KV 与国产库

- MongoDB change stream 要求副本集/分片集群、WiredTiger；resume token 受 oplog 窗口限制；6.0+ pre/post-image 需集合启用。来源：https://www.mongodb.com/docs/manual/changestreams/ 、https://www.mongodb.com/docs/manual/core/replica-set-oplog/
- DynamoDB Streams 保留 24 小时；PITR 增量导出到 S3。来源：https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Streams.html 、https://aws.amazon.com/blogs/database/introducing-incremental-export-from-amazon-dynamodb-to-amazon-s3/
- Elasticsearch 无原生 CDC，官方推荐 `search_after` + PIT。来源：https://www.elastic.co/docs/reference/elasticsearch/rest-apis/paginate-search-results
- Redis keyspace notifications 为 fire-and-forget；Valkey 同样存在该限制。来源：https://redis.io/docs/latest/develop/pubsub/keyspace-notifications/ 、https://github.com/valkey-io/valkey/issues/28
- Cassandra CDC 需每节点本地解析 commitlog；ScyllaDB CDC 为 CQL 日志表，默认 TTL 24 小时。来源：https://axonops.com/docs/data-platforms/cassandra/architecture/storage-engine/cdc/ 、https://docs.scylladb.com/manual/stable/features/cdc/
- TiDB 不支持 MySQL 复制协议，CDC 走 TiCDC。来源：https://docs.pingcap.com/zh/tidb/stable/mysql-compatibility/ 、https://docs.pingcap.com/tidb/stable/ticdc-overview/
- OceanBase Binlog 服务为合成 binlog（对齐 MySQL 5.7、仅 ROW、租户粒度）；Debezium 无官方 OceanBase 连接器。来源：https://www.oceanbase.com/docs/community-oblogproxy-doc-1000000000861458 、https://en.oceanbase.com/docs/common-oceanbase-cloud-10000000001430417
- PolarDB-X 全局 Binlog 为 TSO 归并的合成 binlog。来源：https://doc.polardbx.com/features/topics/global-binlog.html
- openGauss 逻辑解码弱于 PG，社区有 wal2json 适配；自有 JDBC 与 SHA256 认证差异。来源：https://opengauss.org/zh/blogs/July/openGauss%E6%95%B0%E6%8D%AE%E4%B8%8EPostgreSQL%E7%9A%84%E5%B7%AE%E5%BC%82%E5%AF%B9%E6%AF%94.html 、https://opengauss.org/zh/blogs/lihongda/Debezium-Adapt-openGauss.html
- 达梦必须使用达梦驱动；兼容模式建库时固定。来源：https://eco.dameng.com/community/question/c2aa4ffd00089d37f4334fec077f367a 、https://www.cnblogs.com/Chary/p/18026942
- 神通使用自有 JDBC（`oscarJDBC`，默认端口 2003）。来源：https://www.cnblogs.com/nhdlb/p/15660058.html
- 瀚高安全版为三权分立。来源：https://www.cnblogs.com/NyanKoSenSei/p/19422852

### 3.4 连接器框架与 Rust 驱动

- Airbyte：Connector Acceptance Tests（spec/connection/discovery/basic_read/incremental），high strictness 下未跑项需 `bypass_reason`；支持级别 Airbyte/Enterprise/Marketplace/Custom。来源：https://docs.airbyte.com/platform/connector-development/testing-connectors/connector-acceptance-tests-reference 、https://docs.airbyte.com/integrations/connector-support-levels
- Debezium 连接器分 stable / incubating；使用 Testcontainers 做集成测试。来源：https://debezium.io/documentation/reference/stable/connectors/index.html 、https://debezium.io/documentation/reference/stable/integrations/testcontainers.html
- Google Datastream 为每个源逐项公开已知限制、版本、权限与量化限制。来源：https://docs.cloud.google.com/datastream/docs/faq 、https://docs.cloud.google.com/datastream/docs/sources-oracle
- AWS DMS 逐源版本矩阵与 LOB 模式。来源：https://docs.aws.amazon.com/dms/latest/userguide/CHAP_Introduction.Sources.html 、https://docs.aws.amazon.com/dms/latest/userguide/CHAP_Tasks.LOBSupport.html
- sqlx 0.7 起移除 MSSQL，不支持 Oracle；tiberius（TDS，`integrated-auth-gssapi` 特性）；mysql_async 含 binlog 特性；tokio-postgres 提供 binary COPY；官方 `clickhouse`、`mongodb` crate；ADBC Rust 驱动管理器；`oracle`（ODPI-C）、`oracledb`（官方纯 Rust）、`oracle-rs`（第三方）。来源：https://github.com/launchbadge/sqlx/blob/main/FAQ.md 、https://github.com/tiberius-rs/tiberius 、https://crates.io/crates/mysql_async 、https://github.com/supabase/etl 、https://github.com/ClickHouse/clickhouse-rs 、https://docs.rs/mongodb/latest/mongodb/ 、https://arrow.apache.org/adbc/main/rust/index.html 、https://github.com/kubo/rust-oracle 、https://crates.io/crates/oracledb/ 、https://crates.io/crates/oracle-rs/0.1.2
- Oracle Instant Client 受 OTN 开发与分发许可约束；ODPI-C 开源；Debezium 与 Debezium Server 为 Apache-2.0。来源：https://www.oracle.com/downloads/licenses/instant-client-lic.html 、https://oracle.github.io/odpi/ 、https://github.com/debezium/debezium-server

### 3.5 本次检索**未核实**项（不得作为设计事实）

Db2 连接器与 HANA 的精确最小 GRANT；SQL Server 连接器官方最小权限逐字清单；Aurora PG 对 PG 17 failover slot 的支持；Oracle LOB 在各适配器下的逐项行为；HANA 位点可比较性与一致读取；Cassandra `cdc_total_space_in_mb` 默认值；Redis 作源的官方工具；DM8、KingbaseES、GoldenDB、TDSQL、瀚高、神通的官方支持周期与神通 CDC；TDSQL-PG 差异；KingbaseES Debezium 适配的官方地位；msodbcsql、IBM Db2 驱动、SAP HANA 客户端的再分发条款；sqlx Kerberos；odbc-api 与 duckdb crate 的 SPDX；OceanBase flashback 查询在 MySQL 模式的语法与可用性；TiDB 历史读的 GC 窗口细节；`MIN_ACTIVE_ROWVERSION()` 在目标版本上的边界行为；MariaDB `BINLOG MONITOR` 与 `REPLICATION CLIENT` 的版本边界。

## 4. 基线文档回填记录（revision 3）

本设计已回填到下列基线文档（2026-10-09，经项目负责人批准）；哈希基线与审阅记录见 `specs/sources-baseline.json` 与 `execution/decisions/source-baseline-revision-3.md`。

| 文档 | 回填内容 |
|---|---|
| README | 增加本目录入口 |
| 02 技术栈 | 新增 ADR-014（支持矩阵为准）；ADR-002 与本设计的约束一致 |
| 03 数据源 | I-SRC-1 的例外收窄：DBA 创建且平台只读的对象才是例外，连接器需写入的 signaling/心跳/flush 表默认禁用（R1w，须 ADR）；§2、§8 标注权威位置；§4 能力与 AccessReport 结构化、二进制分块排序 |
| 04 管线 | I-PL-6 与 §5.2：`transactional` 只对 T 级 CDC 源有效，其他源须显式声明 S1/S0/B；§5.7 补充新质量状态 |
| 05 Agent DB | `_oo.source_watermark.consistency_grade`；`_oo.table_quality` 增加 `capture_method`、`delete_detection`、`position_state`、`budget_state` |
| 06 Ontology | OOS 数据集可选字段 `source_semantics` |
| 07 Gateway | 质量警告新增 `POSITION_EXPIRED`、`STALE_RISK`、`DELETES_LAGGED`/`DELETES_UNDETECTED`、`SOURCE_BUDGET_EXHAUSTED`；freshness 返回等级与捕获方法 |
| 12 配置参考 | `target`、`driver`、`budget`、`delete_detection`；`sync` 增加 `change_feed`/`partition_refresh`；`snapshot` 不再默认提供 `debezium_incremental`；`consistency` 的适用范围 |
| 14 路线图 | M3 的数据库扩展改为引用本设计的波次与矩阵 |
| 16 参考资料 | 增加 §5A 指向本次检索来源 |

仍保持原文的内容：I-SRC-2 至 I-SRC-4、REQ-SRC-01 至 07 的语义、PG/MySQL 现有 CDC 与批量行为。
