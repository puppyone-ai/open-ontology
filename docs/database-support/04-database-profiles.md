# 04 逐库画像：体检、最小权限、捕获路径、类型与限制

> Status: Proposed
> Last reviewed: 2026-10-09
> 依赖：[02](02-connector-architecture.md)、[03](03-change-capture-and-consistency.md)；事实来源与“未核实”清单见 [06 §3](06-risks-open-questions-and-sources.md)
> 约定：本文所有 SQL/GRANT 是**DBA 清单草案**，由 DBA 执行，平台不执行；凡标“待验证”者，对应 Plan 4A 的 DBP 原型，验证前不得写入 `verified`。等级见 [support-matrix.yaml](support-matrix.yaml)，当前**没有任何目标是 verified**。

## 1. 总览

| 家族 / 目标 | 波次 | 驱动路径 | 首选捕获（拟） | 一致性上限 | 关键限制 |
|---|---|---|---|---|---|
| MySQL 8.x 自建 | 0 | P1 sqlx | C1 binlog | T | GTID/ROW/FULL；见 §2 |
| MariaDB 11.4+ | 1 | P1 sqlx | C1（独立 Debezium MariaDB 连接器） | T | 与 MySQL 的 GTID、JSON 不同，独立目标 |
| RDS/Cloud SQL/PolarDB MySQL、Aurora MySQL | 1 | P1 | C1（Aurora 仅 writer 可读 binlog） | T（待 DBP3） | 保留窗口、性能开销 |
| PostgreSQL 13+ 自建 | 0 | P1 sqlx/tokio-postgres | C1 逻辑解码 | T | publication/槽由 DBA |
| RDS/Aurora/Cloud SQL/AlloyDB PG | 1 | P1 | C1 | T | 各云参数不同；failover 槽（DBP3） |
| SQL Server 2019/2022（含 Azure SQL MI、RDS） | 1 | P1 tiberius | C1 内置 CDC；C2 Change Tracking | T / S1 | CDC 需 DBA 启用；datetime2(7) 精度 |
| Oracle 19c / 23ai | 2 | P2（ODPI-C 或官方纯 Rust，DBP1） | C3/C5 起步；C1 LogMiner 属 R1w 例外 | S1（`AS OF SCN`）/ B | 许可、驱动、空串=NULL、AWR 许可 |
| Db2 LUW 11.5 | 2 | P2 ODBC/JDBC | C3/C5 | B | ASN 需 IIDR 许可且要写控制对象，默认不用 |
| SAP HANA 2.0 | 2 | P2 ODBC/JDBC | C3/C5 | B | 无只读日志 CDC；触发器方案排除 |
| 达梦 DM8、神通 | 2（实验） | P2 | C3/C5 | B | 兼容模式在建库时固定；无标准驱动 |
| TiDB、OceanBase、PolarDB-X、GoldenDB、TDSQL | 2 | P1 MySQL 协议 | 先 C3/C5（TiDB 另评历史读）；合成 binlog 另评 | B（TiDB 历史读待验证后可 S1） | “MySQL 兼容”不含 CDC 兼容 |
| openGauss/GaussDB、Kingbase、HighGo | 2 | P1（若认证兼容）或 P2 | 先 C3/C5 | B | 认证扩展、逻辑解码分叉、三权分立 |
| Snowflake、BigQuery、Databricks、Redshift | 3 | P2 ADBC / P3 REST | C2；Redshift 用 C3/C4/C5 | S1 / S0 | 成本、保留窗口、约束不强制 |
| ClickHouse、StarRocks、Doris | 3 | P1 HTTP / MySQL 协议 | C4 / C3 / C5 | S0 / B | 最终一致、删除延迟 |
| Iceberg、Delta 表 | 3 | P3（对象存储直读）或经 SQL 引擎 | C2 | S0 | 保留窗口；跨快照增量读 Rust 支持不足 |
| MongoDB | 4 | P1 官方 crate + P4 | C1 change stream | 事务级 T 待验证，否则 S0/B | 副本集 / 分片 |
| DynamoDB、ES/OpenSearch | 4 | P3 | C1(Streams)/C3/C5 | B | Streams 仅 24 小时；ES 无变更流 |
| Cassandra、ScyllaDB | 4 | P1/P2 | Scylla CDC 日志表（R1）；Cassandra 排除 | B | Cassandra CDC 需节点本地 agent |
| Redis/Valkey、HANA 触发器、平台安装的 Db2 ASN | — | — | 排除 | — | 见 [01 §8](01-admission-and-support-matrix.md) |

## 2. MySQL 家族

### 2.1 体检与 DBA 清单（在 03 §5.1、§5.3 基础上的补充）

- **账号范围**：全局权限只授予必须全局的（`REPLICATION SLAVE`，MariaDB 10.5.2+ 另需 `BINLOG MONITOR`，`REPLICATION CLIENT` 为其同义词，待验证），`SELECT` 按 schema 授予；`RELOAD`（全局读锁）**仅在需要全局读锁时授予，本方案不依赖**（快照由 OO 分块读取，Debezium 使用无锁模式，待 DBP 验证不触发 `FLUSH TABLES WITH READ LOCK`）。
- **MariaDB**：版本线单独体检（10.6 已到或临近 EOL，10.11、11.4 LTS 较新）；GTID 为 `domain-server-sequence`；使用 Debezium 的**独立 MariaDB 连接器**（2.7 起拆分，不再支持用 MySQL 连接器连接 MariaDB）。
- **托管变体**：
  - RDS MySQL：参数组启用 binlog，`mysql.rds_set_configuration('binlog retention hours', N)`，上限 168 小时；
  - Aurora MySQL：binlog **只能在 writer 读取**；开启 binlog 的额外开销可达约 50%，增强 binlog 约 13%（AWS 文档），DBA 清单必须披露此风险；
  - Cloud SQL MySQL：启用 binlog 依赖 PITR，保留天数随 PITR 设置；
  - PolarDB MySQL：binlog 默认关闭，需手动开启并承担写开销。
- **字符集**：沿用 `latin1` 存 GBK 字节的 `charset_override` 提示；`utf8mb4_0900_*` 等排序规则的**分块排序必须强制二进制**（见 [05 §3](05-conformance-and-evidence.md) 的 `chunk_key_order`）。

### 2.2 类型补充（对 03 §6.2 的增量）

| 源类型 | 目标 | 注意 |
|---|---|---|
| MariaDB `JSON` | `text`，经 `json_valid` 校验后可选 `jsonb` | MariaDB 的 JSON 是 `LONGTEXT` 别名（非原生二进制 JSON）；不合法值写 reject |
| MariaDB `UUID`（10.7+） | `uuid` | — |
| MariaDB/MySQL 系统版本（temporal）表 | 仅当前版本行 | 历史行不同步；Pack 标注 |
| MySQL 8 `VECTOR`、MariaDB `VECTOR` | 排除，`lossy_type_fallback` | — |
| 零日期 | 沿用 `type_policy` | — |

### 2.3 一致性与限制

- C1：T（沿用现有设计）；Aurora/PolarDB 的故障转移与位点保留待 DBP3。
- Aurora 读副本做快照时的“副本回放 ≥ L0”证明方法未核实（DEC-06），在 DBP3 之前 Aurora 目标的 `replica_read` 能力维持 `designed`。

## 3. PostgreSQL 家族

| 变体 | 开启方式（检索事实） | 注意 |
|---|---|---|
| 自建 | `wal_level=logical`、publication、复制槽、`max_slot_wal_keep_size`（见 03 §5.3） | PG 16+ 可在备库建槽；17+ 有 failover slot |
| RDS | `rds.logical_replication=1`（静态参数，重启）、`rds_replication` 角色；解码插件可用 `pgoutput`；**不可安装 `decoderbufs`** | 17 起支持 failover slot 同步到读副本；更早版本槽不随 failover 保留（DBP3） |
| Aurora PG | 逻辑复制可用；有写穿缓存减少解码 I/O | 读副本上的槽、failover 后保留未核实 |
| Cloud SQL | `cloudsql.logical_decoding=on` + 复制属性角色 | 有用户报告设置该 flag 后 `wal_level` 未自动变更，DBA 清单需验证实际 `wal_level` |
| AlloyDB | `alloydb.logical_decoding=on`，`pgoutput`/`pglogical` | 在只读池上建槽的限制未核实 |

通用：TOAST 未变列占位（`REPLICA IDENTITY` 与 04 §5.3 的“保留原值”处理）、逻辑复制不含 DDL/序列、槽不消费会撑满 WAL——全部沿用现有设计。

## 4. SQL Server（2019/2022；Azure SQL MI；RDS for SQL Server）

### 4.1 驱动与体检

- P1 tiberius（TLS：native-tls 或 rustls；AAD 令牌认证；Kerberos 通过 `integrated-auth-gssapi` 特性，待验证）。sqlx 已不含 MSSQL。
- 体检项：`SERVERPROPERTY`（版本/版本号/引擎版本）、数据库兼容级别、排序规则（库级与列级）、是否可读副本、`ALLOW_SNAPSHOT_ISOLATION`、Change Tracking/CDC 是否已启用、CDC 捕获与清理作业状态与保留期、权限（`HAS_PERMS_BY_NAME`、角色成员、`VIEW DEFINITION`、`VIEW DATABASE STATE`）。

### 4.2 DBA 清单草案（待 DBP2 验证）

```sql
-- 只读账号（只授予必要对象）
CREATE LOGIN oo_reader WITH PASSWORD = '<由 DBA 设置>';
USE erp;
CREATE USER oo_reader FOR LOGIN oo_reader;
GRANT SELECT ON SCHEMA::dbo TO oo_reader;          -- 不用 db_datareader，避免新 schema 自动可见
GRANT VIEW DEFINITION TO oo_reader;                -- 读取视图/过程正文（Self-build S0）
GRANT VIEW DATABASE STATE TO oo_reader;            -- 统计与 DMV（可选）

-- 方案 A：内置 CDC（C1，T）。需要 sysadmin（Azure SQL Database 为 db_owner）
EXEC sys.sp_cdc_enable_db;
EXEC sys.sp_cdc_enable_table @source_schema = 'dbo', @source_name = 'orders',
     @role_name = 'oo_cdc_reader', @supports_net_changes = 0;
ALTER ROLE oo_cdc_reader ADD MEMBER oo_reader;
EXEC sys.sp_cdc_change_job @job_type = 'cleanup', @retention = 10080;   -- 分钟；须覆盖最长停摆

-- 方案 B：Change Tracking（C2，S1）。只记录键，数据由 AS OF 快照读取
ALTER DATABASE erp SET CHANGE_TRACKING = ON (CHANGE_RETENTION = 7 DAYS, AUTO_CLEANUP = ON);
ALTER TABLE dbo.orders ENABLE CHANGE_TRACKING WITH (TRACK_COLUMNS_UPDATED = OFF);
ALTER DATABASE erp SET ALLOW_SNAPSHOT_ISOLATION ON;  -- 一致读取 CT 版本与数据
```

- 风险披露：CDC 创建 `cdc` schema、变更表与 Agent 作业，磁盘与日志重用延迟增加；Azure SQL Database 无 Agent，使用内置 scheduler（约每 20 秒捕获、每小时清理，不可定制）；启用 CDC 会影响在线 DDL，新增列需 DBA 新建捕获实例；RDS 使用 `msdb.dbo.rds_cdc_enable_db`。
- CDC 不能在 AlwaysOn 可读副本上“运行捕获”，捕获发生在主库；读取变更表可走副本连接（`ApplicationIntent=ReadOnly`），但副本回放进度需证明（DEC-06，待 DBP2）。
- Debezium 增量快照与信号表 **不使用**（R2）。

### 4.3 类型映射与陷阱

| SQL Server | 目标 | 注意 |
|---|---|---|
| `bit` | `boolean` | — |
| `tinyint/smallint/int/bigint` | `smallint/smallint/integer/bigint` | `tinyint` 为 0–255 |
| `decimal/numeric(p,s)` | `numeric(p,s)` | — |
| `money/smallmoney` | `numeric(19,4)` / `numeric(10,4)` | 避免 locale |
| `float/real` | `double precision/real` | — |
| `date` | `date` | — |
| `time(n)` | `time(min(n,6))` | n=7 → `precision_loss` |
| `datetime` | `timestamp(3)` | 精度为 1/300 秒量级，毫秒内取整 |
| `smalldatetime` | `timestamp(0)` | — |
| `datetime2(n)` | `timestamp(min(n,6))` | n=7 → `precision_loss`；用作游标/PK 则 block |
| `datetimeoffset(n)` | `timestamptz(min(n,6))` | `offset_lost` + 可能的 `precision_loss` |
| `uniqueidentifier` | `uuid` | — |
| `char/varchar/nchar/nvarchar/text/ntext(max)` | `text` | CI 排序规则 → `source_collation_ci`；尾空格语义 → `trailing_space_insensitive` |
| `binary/varbinary/image` | `bytea` | 大对象默认排除 |
| `rowversion/timestamp` | `bytea`（8 字节），标 `system_column` | 作为游标候选，需 `MIN_ACTIVE_ROWVERSION()` 作上界 |
| `xml` | `text` | — |
| `sql_variant`、`hierarchyid`、`geography/geometry` | `text`（WKT/字符串）或排除 | `lossy_type_fallback` |
| `json`（原生，新版本） | `jsonb` | 版本支持待验证 |

CDC 路径中的陷阱：排序规则与列排序规则不一致可能导致 CDC 变更表数据丢失（微软已知问题）；新增带默认值的非空列，存量行在 CDC 中显示 NULL；内存优化表的 LOB 不被捕获；启用 CDC 后在线索引重建等 DDL 受限。这些均写入对应目标的 `limits` 清单。

## 5. Oracle（19c；21c 创新版；23ai）

### 5.1 驱动与许可

- P2 worker。候选：基于 ODPI-C 的 `oracle` crate（需 Oracle Client 库在运行时存在）、Oracle 官方新纯 Rust 驱动（thin 模式，成熟度需 DBP1 评估）、第三方纯 Rust TNS 实现（早期，不作默认）。sqlx 不支持 Oracle。
- **默认不分发 Instant Client**；客户提供驱动卷。若法务确认可分发，按 [02 §3.2](02-connector-architecture.md)。
- 12c 已出支持，不列为目标（候选）；21c 为创新版本，生命周期较短，只在客户需要时评估。

### 5.2 最小权限（批量，待 DBP1 验证）

```sql
CREATE USER oo_reader IDENTIFIED BY "<由 DBA 设置>";
GRANT CREATE SESSION TO oo_reader;
GRANT SELECT ON erp.orders TO oo_reader;                 -- 逐表授权；不授 SELECT ANY TABLE
GRANT FLASHBACK ON erp.orders TO oo_reader;              -- 对象级，使 AS OF SCN 可用；不授 FLASHBACK ANY TABLE
GRANT EXECUTE ON sys.dbms_flashback TO oo_reader;        -- 读取当前 SCN
-- 元数据：ALL_* 视图在 CREATE SESSION 下只显示可访问对象；视图/过程正文需对象访问权
```

- 一致读：`SELECT … AS OF SCN n`，受 undo 保留限制（`ORA-01555`）；计划阶段按估算读取时长与 `undo_retention` 比较，不满足则拒绝 S1 模式、退回 B。
- 读取副本：物理 standby 只读打开需要 Active Data Guard 许可（需客户确认）；Debezium 对 standby 的支持被标为孵化特性。

### 5.3 CDC 的处理（默认关闭）

- Debezium Oracle 支持 LogMiner（默认）、XStream（需 Oracle GoldenGate 许可）、OpenLogReplicator 三种适配器；
- 上游示例的连接器账号包含 `CREATE TABLE`、`CREATE SEQUENCE`、`LOCK ANY TABLE`、`SELECT ANY TABLE`、`FLASHBACK ANY TABLE` 等，其中 LogMiner 适配器使用一张连接器辅助表（flush 表）。这类写入属于 **R1w**：必须由 DBA 预创建专用 schema/对象，账号去掉 `CREATE TABLE` 与 `ANY` 权限，**经 DBP1 验证实际写入范围仅为该辅助表**，并取得最小权限 ADR 后才能进入 `experimental`。
- XStream 需要商业许可且创建出站服务器：默认 `excluded`。
- 补充日志与 ARCHIVELOG（R1）、归档保留（RDS Oracle 需用 `rdsadmin` 过程设置）进入 DBA 清单；补充日志增加 redo 量，需披露。
- 因此 Oracle 的路线：**先 verified 批量，CDC 作为单独能力经 ADR 决定**；`sync: auto` 不会为 Oracle 选 CDC。

### 5.4 类型映射与陷阱

| Oracle | 目标 | 注意 |
|---|---|---|
| `NUMBER(p,s)` | `numeric(p,s)` | p≤9、s=0 可映射 `integer`，p≤18 `bigint`（由 type_policy，默认保持 numeric 以避免二义） |
| `NUMBER`（无精度） | `numeric`（不约束） | **读取走文本/十进制路径，禁止经 f64**；CDC 路径下被某些组件表示为可变标度十进制，需与批量一致（双路径测试） |
| `FLOAT/BINARY_FLOAT/BINARY_DOUBLE` | `double precision/real/double precision` | NaN/Inf 保留 |
| `DATE` | `timestamp(0)` | **含时分秒**；`DATE` 并非纯日期 |
| `TIMESTAMP(n)` | `timestamp(min(n,6))` | n 7–9 → `precision_loss` |
| `TIMESTAMP WITH TIME ZONE` / `WITH LOCAL TIME ZONE` | `timestamptz` | 偏移丢失；LTZ 依赖会话/库时区，需显式时区设置 |
| `INTERVAL YEAR TO MONTH / DAY TO SECOND` | `interval` | — |
| `VARCHAR2/NVARCHAR2/CHAR/NCHAR/CLOB/NCLOB/LONG` | `text` | **空串即 NULL** → `empty_string_is_null`；CHAR 补空格保留；LONG/大对象默认排除；字节/字符长度语义记录 |
| `RAW/BLOB/LONG RAW/BFILE` | `bytea`/排除 | 默认排除 |
| `ROWID/UROWID` | `text` | — |
| `XMLTYPE` | `text` | — |
| `BOOLEAN`（23ai）、`JSON`（21c+） | `boolean`、`jsonb` | 版本相关，分目标验证 |
| `VECTOR`（23ai） | 排除 | `lossy_type_fallback` |
| 用户定义类型、`SDO_GEOMETRY`、`ANYDATA` | 排除或 `text` | 记录 |

标识符：默认大写，保留原名；多租户（CDB/PDB）仅连接具体 PDB 服务。排序：分块键比较使用 `NLSSORT(col,'NLS_SORT=BINARY')` 或等价二进制排序（见 [05 §3](05-conformance-and-evidence.md)）。

### 5.5 证据来源

`V$SQL` 可免额外许可读取，AWR 视图依赖 Diagnostic Pack，AccessReport.`license_flags` 标记并默认关闭 AWR 读取。

## 6. IBM Db2 LUW 与 SAP HANA

### 6.1 Db2 LUW 11.5

- P2 ODBC/JDBC worker，驱动由客户提供；z/OS、iSeries 不在范围（Db2 for i 在 Debezium 为孵化的独立连接器，LUW 为稳定）。
- 最小权限：`CONNECT` + 对象 `SELECT`；目录 `SYSCAT` 视图对象可见性按授权；读取视图/例程正文需相应权限（不足即 `logic_unreadable`）。
- 增量：`GENERATED ALWAYS … ROW CHANGE TIMESTAMP` 类列仅当表已有；否则 C3 依赖业务时间列；无可靠只读 CDC。Debezium Db2 连接器依赖 ASN SQL Replication：需在源库创建 ASN 对象与变更数据表、服务器端运行 capture 程序，且使用 SQL Replication 需要 IBM InfoSphere Data Replication 许可，连接器也需调用控制接口 → 属 R1w/R2，**默认 `excluded`**；DBA 自行搭建 ASN 并由平台仅轮询变更表的**纯读取实现**列为 `candidate`（非 Debezium）。
- 类型：`TIMESTAMP(p)` 最大 12 位 → `precision_loss`；`DECFLOAT` 的 NaN/Infinity 写 reject；`FOR BIT DATA` → `bytea`；`CLOB/DBCLOB/BLOB` 默认排除；`XML` → `text`（捕获路径对 XML 不完整，仅批量）；代码页转换严格解码。
- 一致性：C3/C5，B（或单语句 S0）；无通用 as-of 读取。

### 6.2 SAP HANA 2.0

- P2 ODBC/JDBC worker，客户端 `hdbclient` 受 SAP 开发者许可约束，按客户提供。
- 权限：`CATALOG READ`（元数据）+ 逐 schema/表 `SELECT`；分析权限与行级过滤可能让读取结果因账号而异 → 体检必须核对源侧限制，未知则 block。
- 无只读日志 CDC：SDI/SLT 触发器复制需在源库建触发器与影子表（R2），平台不采用；Debezium 无 HANA 连接器。**批量 C3/C5**；一致性 B。
- 类型：`SECONDDATE/TIMESTAMP(7 位)` → 微秒（`precision_loss`）；`ALPHANUM/SHORTTEXT/NVARCHAR` → `text`；`BLOB/CLOB/NCLOB/TEXT` 默认排除；`ST_GEOMETRY` → `text`；`SMALLDECIMAL/DECIMAL` → `numeric`。
- 生命周期：2.0 SPS05 已于 2025 年底结束主流维护，SPS07 为长期版本；目标版本区间限 SPS07 及以上，其余需客户提出。

## 7. 自有协议的国产库（达梦 DM8、神通 OSCAR）

- 驱动：**标准 MySQL/PG/Oracle 驱动不可用**；达梦必须使用 `DmJdbcDriver`/ODBC，神通使用 `oscarJDBC`（默认端口 2003）。均为 P2 worker，驱动由客户提供，许可再分发条款未核实。
- DM8 兼容模式在建库时固定（Oracle/MySQL 等），体检**必须读取并记录兼容模式与字符集/长度语义**，未知 → `COMPAT_MODE_UNKNOWN` block；官方承认其 MySQL 兼容度不高，Oracle 兼容为主；因此 DM 的 Oracle 模式不继承 Oracle 目标的任何结论。
- CDC：达梦 DMHS 为官方实时同步产品（日志解析），属商业组件；Debezium 无连接器；网上有 canal/Flink CDC 集成实践但未核实。**不规划 CDC**，仅批量（C3/C5）。神通 CDC 手段未核实。
- 等级：两者最高暂定 `experimental`，需真实实例与官方文档核对类型与权限后再评估 `verified`。

## 8. 兼容层国产库（MySQL/PG 协议兼容 ≠ CDC 兼容）

| 目标 | 驱动 | 批量路径 | CDC/变更路径（检索事实） | 本方案处理 |
|---|---|---|---|---|
| **TiDB** | MySQL 协议（P1） | C3/C5；历史读（`AS OF TIMESTAMP`/`tidb_snapshot`）可提供 S1，待验证 | **不支持 MySQL 复制协议**，唯一官方路径是 TiCDC（输出 Kafka/Canal-JSON/Avro 等） | v0.x 只做批量；TiCDC 经 P5/buffer 适配器（SP15-W03），独立目标 |
| **OceanBase（MySQL 模式）** | MySQL 协议（2881 直连/2883 ODP，用户名含租户） | C3/C5 | 经 OBLogProxy/Binlog 服务得到**合成** binlog（对齐 MySQL 5.7、仅 ROW、租户粒度、enum/set 扩展不支持）；Debezium 无官方连接器，Flink CDC 有专用连接器 | 批量先行；binlog 路径作为独立目标 + DBP7 |
| **OceanBase（Oracle 模式）** | 企业版专有 | — | — | `candidate`，不继承 Oracle |
| **PolarDB MySQL** | MySQL 协议 | C3/C5 | 原生 binlog，默认关闭，需开启 | 归入 §2 托管变体 |
| **PolarDB-X** | MySQL 协议 | C3/C5 | 全局 Binlog（TSO 归并的合成位点） | 独立目标；位点 `synthetic_binlog` |
| **PolarDB PG** | PG 协议 | C3/C5 | 逻辑复制槽（宣称兼容 PG） | 归入 §3 作为独立目标，需 DBP3 同类验证 |
| **GoldenDB** | MySQL 协议（计算节点） | C3/C5 | 自带 CDC 组件输出 MySQL binlog 格式流 | 同 OceanBase：批量先行 |
| **TDSQL** | 按形态：TDSQL-C MySQL/PG、分布式 MySQL、TDSQL-PG | C3/C5 | TDSQL-C 提供 binlog 订阅；分布式版要求 ROW binlog | 每个形态独立目标，不互相推导 |
| **openGauss / GaussDB** | PG 协议；**认证（SHA256）与标准驱动可能不兼容**，官方提供自有 JDBC | C3/C5；若认证不兼容走 P2 | 逻辑解码弱于 PG，社区有 wal2json 适配；GaussDB 另有 MySQL 兼容形态 | 批量先行；认证兼容在 DBP7 验证 |
| **KingbaseES** | PG 驱动（PG 兼容模式，第三方实践）+ 自有驱动 | C3/C5 | 逻辑复制存在，Debezium 适配版为社区实现，官方地位未核实 | PG 兼容模式单独成目标；Oracle/MySQL 模式 `candidate` |
| **瀚高 HighGo** | PG 驱动（一般可用） | C3/C5 | 未核实 | **三权分立**安全版：平台账号可能无法读目录，体检不确定即 block |

共通规则：

1. 这些目标沿用相邻家族的**代码**，不沿用相邻家族的**验收**；
2. “合成 binlog/位点”目标的 `PositionCodec` 独立实现，质量元数据记录 `synthetic_binlog`；
3. 无法在真实实例上取得夹具的国产库（商业许可），只能在客户环境验证，矩阵为 `scope: deployment-local`。

## 9. 云数仓、湖表与 OLAP

### 9.1 公共前提

- **约束只声明不强制**（Snowflake、BigQuery、Redshift、Synapse、Databricks 的 PK/FK）：`constraint_trust=declared_only`；关系推断需包含依赖检验。
- 增量只在**有保留窗口**的 C2 上可靠；其他走 C3/C4/C5，删除靠对账。
- 读取成本按 [02 §7](02-connector-architecture.md)，每个目标声明计量单位与强制手段。
- 业界工具（Airbyte、Fivetran、dlt、Sling、Meltano）对这些源普遍使用游标/分区级增量，删除检测是公认缺口——这与 §03 的 `DELETES_LAGGED` 披露方向一致。

### 9.2 Snowflake

- 驱动：P2 ADBC（Go 驱动）或 P3 SQL REST API；无官方 Rust 驱动。
- 最小权限：对专用角色授予 warehouse `USAGE`、database/schema `USAGE`、逐表 `SELECT`（**不使用 `FUTURE` 授权**，新表默认不可见，契合“新对象需分类”）；查询历史证据另需 `ACCOUNT_USAGE` 访问（可选）。
- 捕获：C2 用 `CHANGES` 子句 + DBA 已启用的 `CHANGE_TRACKING`（启用为 DDL 且短暂锁表，R1）；**不创建 Stream（R2）**；一致读取用 Time Travel `AT/BEFORE`（1 天，Enterprise 以上可达 90 天）。stale 后变更丢失，需 `POSITION_EXPIRED` 重快照。
- 成本：专用 XS warehouse、`AUTO_SUSPEND=60`、Resource Monitor、`STATEMENT_TIMEOUT_IN_SECONDS`；每次唤醒最低计 60 秒，调度合并。
- 类型：`NUMBER(≤38)`→`numeric`；`VARIANT/OBJECT/ARRAY`→`jsonb`；`TIMESTAMP_NTZ/LTZ/TZ` 三种语义分别映射（`timestamp`、`timestamptz`、`timestamptz`+`offset_lost`）；`TIME/TIMESTAMP(9)`→微秒（`precision_loss`）；`GEOGRAPHY/GEOMETRY`→`text`。

### 9.3 BigQuery

- 驱动：P3 Storage Read API（Arrow）/REST；ADBC 驱动成熟度较低；官方 Rust SDK 已有，成熟度待评估。
- 权限：`bigquery.dataViewer`（数据集）、`bigquery.jobUser`（项目）、`bigquery.readSessionCreator`（Storage Read）。
- 捕获：C2 `CHANGES`/`APPENDS` 需表启用 `enable_change_history`（R1），窗口 = time travel（默认 7 天，可缩至 2 天）；长周期可由**客户事先创建的表快照**延长（R1）。
- 成本：**按扫描字节**，用 dry run 估算 + `maximum_bytes_billed` + 项目日配额；分区/聚簇表优先按分区读。
- 类型：`NUMERIC(38,9)`→`numeric(38,9)`；`BIGNUMERIC(76,38)`→`numeric`（无约束，十进制字符串路径）；`FLOAT64` NaN/Inf 保留；`DATETIME`→`timestamp`、`TIMESTAMP`→`timestamptz`；`STRUCT/ARRAY/JSON`→`jsonb`；`GEOGRAPHY/RANGE/INTERVAL`→`text`。

### 9.4 Databricks / Delta

- 驱动：P3 SQL Statement Execution REST；或 `delta-rs` 直读对象存储（绕过计算，需对象存储只读凭证与 UC 治理策略，作为独立目标评估）；JDBC/ODBC 作 P2。
- 权限：Unity Catalog `USE CATALOG`、`USE SCHEMA`、`SELECT`；专用小 SQL warehouse，自动停止，budget policy。
- 捕获：C2 Delta CDF（表属性 `delta.enableChangeDataFeed`，R1），受日志（默认 30 天）与 VACUUM（默认 7 天）约束；CDF 不是永久记录。一致读取 `VERSION AS OF`。
- 类型：`DECIMAL(≤38)`；`TIMESTAMP`（会话时区）与 `TIMESTAMP_NTZ` 区分；`STRUCT/MAP/ARRAY/VARIANT`→`jsonb`；删除向量表需支持该特性的读取方。

### 9.5 Redshift

- 驱动：PG 线协议（P1，需验证与 PG 的 catalog/行为差异）或 Data API（P3）。
- 无行级 CDC；`STL/SYS` 视图只是操作日志（保留数天）；增量只能 C3/C4/C5；大表读取优先 `UNLOAD` 到客户指定对象存储（写客户桶，不写源表，需 DBA 授权，R1）。
- 权限：`USAGE` + 逐表 `SELECT`；WLM 队列 + QMR 限制并发与时长。
- 类型：`SUPER`→`jsonb`（单值上限 1 MB）；`VARBYTE`→`bytea`；`HLLSKETCH` 排除；约束仅信息性。

### 9.6 ClickHouse、StarRocks、Doris

- ClickHouse：P1 官方 HTTP crate；**无 binlog/CDC、无多语句事务**；`ReplacingMergeTree` 去重发生在后台合并，一致读需 `FINAL`；轻量 `DELETE` 异步生效；表级捕获用 C4（`system.parts` 修改时间）或 C3/C5，对账靠主键键集（列式扫描主键列代价低）。账号：`readonly` profile + quota + `max_execution_time`。类型：`UInt64`→`numeric(20,0)`，`Int128/UInt128/Int256/UInt256`→`numeric`，`Decimal256`，`LowCardinality(T)` 视为 `T`，`Nullable` 保留；非 Nullable 列的“默认值代替缺失” → `default_for_missing`。
- StarRocks / Doris：MySQL 协议（P1）；Primary/Unique Key 表可变、Duplicate 表只追加；**2.x/3.x（Doris）无对外变更流**，Doris 5.x 的 Row Binlog/Table Stream（实验、文档标注未发布）不作依据；采用 C4 分区刷新或 C3/C5。`BITMAP/HLL/QUANTILE_STATE` 等聚合类型排除。资源限制靠 resource/workload group。

### 9.7 Iceberg、Delta（对象存储直读）与 Trino

- Iceberg：每次提交一个快照，读取任一 `snapshot-id` 是强一致 S0（单表）；`append` 增量易得，更新/删除需处理 delete files 或做 manifest 差；**Rust 官方库目前不支持跨快照增量读**（已知 issue），且快照默认 5 天过期；因此 Iceberg 的 C2 需自实现快照差，列 `designed`。
- Trino：联邦查询引擎，不是存储；作为源只在“底层库无法直连”时使用，一致性与删除语义取决于底层 connector，**不得跨 catalog 声明 T/S1**；等级上限 `experimental`，大结果集用 spooling protocol。

## 10. 文档与键值源

| 目标 | 捕获 | 权限与前置 | 要点 |
|---|---|---|---|
| **MongoDB** | C1 change stream（需副本集或分片集群，WiredTiger）；Debezium MongoDB 稳定连接器 | 内置 `read` 角色足以开启 change stream；pre/post-image（6.0+）需集合启用 `changeStreamPreAndPostImages`（DBA，R1） | oplog 是 capped collection，resume token 过期即失效 → `POSITION_EXPIRED`；删除事件默认只含 `_id`；映射见 [02 §6.4](02-connector-architecture.md)；标准单机实例（standalone）不支持 change stream → 只能 C5 |
| **DynamoDB** | Streams（24 小时）+ 并行 Scan；PITR 增量导出到 S3 | `DescribeStream`、`GetShardIterator`、`GetRecords`、`ListStreams` + 表读取；导出需客户指定桶的写权限（R1，写客户桶） | 记录序仅分区键内有序（`PerPartition`）；滞后须 < 12 小时告警；读容量预算（RCU） |
| **Elasticsearch/OpenSearch** | C5 或按时间戳字段 C3；PIT + `search_after` 取得稳定视图 | 索引只读 | 无变更流、无事务；删除靠对账；mapping 即 schema；一致性 B |
| **ScyllaDB** | CDC 日志表（CQL 可查，行 TTL 默认 24 小时），表级启用 CDC 属性（DBA，R1） | 日志表 `SELECT` | 读取日志表的 CL 需匹配基表写 CL 才可得强一致 |
| **Cassandra** | 官方 CDC 需**每节点本地 agent 解析 commitlog**（主机级） | — | 默认 `excluded`；用 C3/C5（CQL 强类型） |
| **Kafka 主题** | 消费 offset；compaction；tombstone=删除 | 主题只读 ACL | 仅经 SP15-W03 的 buffer 适配器，v0.x 不承诺 |
| **Redis/Valkey** | keyspace 通知为 pub/sub，**断线期间事件丢失、无持久** | — | `excluded`；提供导出文件路径 |

## 11. 各家族的“DBA 清单”共同模板

每个目标的 DBA 清单必须包含：前置条件与影响（磁盘、日志、锁、性能开销）、命令（可直接执行）、**回滚命令**、验证命令（平台体检会运行的只读检查）、失效时的恢复方案、需要的许可/授权声明，以及“若不执行此清单，将退回到哪种能力”。清单由 `oo` 体检按检测到的目标和可用能力生成，平台自身不执行。
