# 03 数据源、连接器与文件接入

> Status: Proposed
> Owner: Open Ontology Engineering
> Last reviewed: 2026-10-01
> 依赖：[01 架构](01-architecture.md)、[02 技术栈](02-technology-stack.md)
> 被依赖：[04 管线](04-pipeline.md)、[06 Ontology](06-ontology-layer.md)

## 1. 职责与不变量

登记企业已有的数据库与文件，让管线知道读什么、怎么读。

- **I-SRC-1** 平台对源库只读：不写业务数据，不在源库建表、视图、触发器、存储过程。唯一的例外是 DBA 按清单**手工**创建、平台只读取的 CDC 所需对象或设置（PG publication 与复制槽、MySQL binlog 设置、SQL Server CDC/Change Tracking 的启用、Oracle 补充日志等），平台代码不自动创建它们。连接器需要**写入**的辅助对象（Debezium signaling/心跳/flush 表等）不属于该例外：默认禁用，仅在最小权限 ADR 与源端审计证明写入范围后才可作为 R1w 例外启用（分类见 [database-support/03 §1](database-support/03-change-capture-and-consistency.md)）。
- **I-SRC-2** 凭证不以明文出现在配置中；只以 `secret://` 引用。
- **I-SRC-3** 能写业务表的凭证默认拒绝登记。
- **I-SRC-4** 所有读取受限速，优先从只读从库读。

“任何数据库”的承诺按以下方式兑现：**CDC 能力取决于已验证的 Debezium 连接器；批量接入需受支持的只读驱动/连接器与 `SELECT` 权限**，因此具有受支持 Rust 驱动或已审核外部连接器、且能执行 SELECT 的库可以批量接入；仅有 SQL 接口不等于首版已支持该数据库；文件和数据库导出文件（dump）作为第三条路径。

> **数据库范围与准入的权威位置：** 本文 §2、§8 的数据库类型与路线表是早期草案。哪些数据库、哪些版本、哪些能力已被接受，由[支持矩阵与准入路线](database-support/01-admission-and-support-matrix.md)和 Spec 第 7A 章决定；能力模型、捕获方法、一致性等级、驱动策略与逐库设计见[数据库支持扩展设计](database-support/README.md)。两者冲突时以后者为准。

## 2. 源的类型

> 下表只是源的分类示意，不代表已支持。“第一版/第二批/第三批”等字样不是承诺，也不替代支持矩阵中的等级。

| 类型 | 例子 | 同步方式 | 第一版 |
|---|---|---|---|
| 关系数据库（可 CDC） | MySQL / MariaDB、PostgreSQL（含 RDS、Aurora、Cloud SQL、AlloyDB、Azure） | CDC + 每日对账 | ✓ |
| 关系数据库（不可 CDC） | 同上但无法开日志；以及任何有 SQL 接口的库 | 批量增量（`updated_at` 水位）+ 每日对账 | ✓ |
| 数据仓库 | Snowflake、BigQuery、Databricks、Redshift、ClickHouse | 批量增量（对应 Rust 驱动 / 可选 ADBC） | 第二批 |
| 需要额外配置的 CDC 库 | SQL Server（CDC 表）、Oracle（LogMiner） | CDC | 第二、三批 |
| 文档库 | MongoDB | CDC（Debezium）→ `jsonb` 列 + 展开视图 | 规划中 |
| 文件 | CSV / TSV / Excel（xlsx、xls）/ Parquet / JSON Lines | 上传 → 解析 → 全量替换或按键合并 | ✓ |
| 数据库文件 / 导出 | SQLite 文件、`mysqldump` / `pg_dump` 的 SQL 文件 | 恢复到临时实例 → 当作批量源全量读取 | ✓（M1） |

## 3. 注册机制

### 3.1 插件与实例

- **插件**内置实现为 workspace crate，按 Cargo feature 显式注册为 `Arc<dyn Trait + Send + Sync>`；禁止自动扫描目录并加载本机动态库（Rust ABI 不稳定且没有隔离）。第三方实现使用管理员审核的独立进程/容器，通过版本化 JSON 控制协议 + Arrow IPC 数据流接入；清单锁定协议版本、镜像 digest、权限与资源限额。见 [17 §4](17-rust-implementation.md)。
- **实例**写在配置 Git 仓库的 YAML 中，用 `oo validate → oo plan → oo apply` 生效。控制台的“添加数据源”向导生成同样的 YAML 并提交（审核后生效）。

```yaml
# sources/erp_mysql.yaml
sources:
  erp_mysql:
    connector: mysql
    connection: { host: 10.0.1.12, port: 3306, database: erp }
    read_from: replica                  # 有只读从库时从从库读
    credential: secret://erp_mysql
    timezone: Asia/Shanghai             # DATETIME 无时区时按此解释
    tables:
      include: ["*"]
      exclude: ["tmp_*", "bak_*", "*_log_2019*"]
    sync: auto                          # auto | cdc | incremental | full
    incremental: { cursor: updated_at, every: 1h, overlap: 5m }
    limits: { max_connections: 4, max_rows_per_sec: 50000 }
    type_policy: { zero_date: null, tinyint1: smallint, strip_nul: true }
    overrides:
      sys_dict: { sync: full, every: 6h }
      ydh: { columns: { exclude: [fj_blob] } }
    hooks: [mask_phone]
```

`sync: auto` 的判定：连接器 `check_access` 确认日志配置与复制权限齐备则走 CDC，否则走批量增量；没有主键也没有声明唯一键的表只能 `full`。

### 3.2 登记流程

```text
填写连接信息 ──▶ 连通性测试 ──▶ 权限体检 check_access ──▶ 表清单与体量估算
  ──▶ 选择表/列（默认排除大对象列）──▶ 生成 DBA 操作清单（若走 CDC）
  ──▶ plan（将要启动的容器、预计首次同步时长、源库预计读取量）──▶ apply
```

## 4. 连接器接口

```rust
// 接口契约：业务类型由 oo-core 定义，省略实现；不是独立程序。
use std::{collections::BTreeSet, pin::Pin};
use arrow_array::RecordBatch;
use async_trait::async_trait;
use futures_core::Stream;
use tokio_util::sync::CancellationToken;

type BatchStream = Pin<Box<dyn Stream<Item = Result<RecordBatch, OoError>> + Send>>;

#[async_trait]
pub trait Connector: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> BTreeSet<Capability>;
    async fn check_access(&self) -> Result<AccessReport, OoError>;
    async fn list_tables(&self) -> Result<Vec<TableRef>, OoError>;
    async fn read_schema(&self, tables: &[TableRef]) -> Result<Vec<TableSchema>, OoError>;
    async fn read_logic(&self) -> Result<Vec<LogicObject>, OoError>;
    async fn estimate(&self, table: &TableRef) -> Result<TableStats, OoError>;
    async fn plan_chunks(&self, table: &TableRef, target_rows: usize) -> Result<Vec<Chunk>, OoError>;
    async fn read_chunk(&self, chunk: Chunk, cancel: CancellationToken) -> Result<BatchStream, OoError>;
    async fn read_incremental(&self, table: TableRef, cursor: Cursor, cancel: CancellationToken) -> Result<BatchStream, OoError>;
    async fn read_keys(&self, table: TableRef, range: KeyRange, cancel: CancellationToken) -> Result<BatchStream, OoError>;
    fn type_map(&self, col: &SourceColumn, policy: &TypePolicy) -> Result<ColumnMapping, OoError>;
    fn cdc_config(&self) -> Result<Option<DebeziumConfig>, OoError>;
    fn position_codec(&self) -> Box<dyn PositionCodec + Send + Sync>;
}
```

- `read_chunk` 返回有界异步 Arrow RecordBatch 流。首版用 SQLx 的 PG/MySQL 驱动流式取行，再用 Arrow builders 按行数与字节预算组批；不是驱动天然零拷贝列式读取。ADBC / ConnectorX 的 Rust 路径作为可选优化，驱动覆盖、许可证、类型转换与取消语义验证后启用。
- `type_map` 是**连接器的职责**：批量路径与 CDC 路径必须产出同样的 Arrow 类型，否则对账会误报。每个连接器都要有“同一行经两条路径读取、schema 与规范值一致”的一致性测试。
- `capabilities()` 与 `AccessReport` 必须是结构化声明（能力种类、游标/变更读取/CDC 种类、位点序、预算单位、各捕获方法所需的源端写入分类、保留窗口、一致性等级、驱动出处与兼容模式），不得用连接器名称字符串判断路径；字段与等级见 [database-support/02 §2](database-support/02-connector-architecture.md)。未知权限、兼容模式或计费单位按 `block` 处理。
- 字符串/二进制主键的分块必须强制二进制排序，否则改用哈希分块或 `read_keys`（[database-support/05 §3](database-support/05-conformance-and-evidence.md)）。
- 分块按主键范围（`plan_chunks` 用主键的 min/max 与统计直方图切分；主键不均匀时用 `LIMIT` 游标式分块），每块默认 5 万行。

## 5. 权限体检（`check_access`）

体检结果分为 `ok`、`warn`、`block` 三级，`block` 阻止登记。

### 5.1 MySQL

| 检查 | 方法 | 级别 |
|---|---|---|
| 账号是否有写权限 | `SHOW GRANTS FOR CURRENT_USER()` 解析出 `INSERT/UPDATE/DELETE/CREATE/DROP/ALTER/ALL` | block（可由管理员显式 `allow_write_capable_credential: true` 覆盖，记录审计） |
| 是否读的是从库 | `SELECT @@read_only, @@super_read_only`；`SHOW REPLICA STATUS` 的延迟 | `read_from: replica` 但连上的是主库 → warn |
| CDC 前提 | `@@binlog_format = ROW`、`@@binlog_row_image = FULL`、`@@gtid_mode = ON`、`@@binlog_expire_logs_seconds`（RDS：`mysql.rds_show_configuration` 的 binlog retention hours） | 不满足 → 回退批量，并生成 DBA 清单 |
| 从从库做 CDC | 从库需 `log_replica_updates = ON` | 同上 |
| 复制权限 | `REPLICATION SLAVE, REPLICATION CLIENT` | 同上 |
| 字符集 | 表/列的 `CHARACTER SET`；`latin1` 列可能存着 GBK 字节（陈旧中文系统常见） | warn，并提示配置 `charset_override` |

### 5.2 PostgreSQL

| 检查 | 方法 | 级别 |
|---|---|---|
| 写权限 | `has_table_privilege(t, 'INSERT,UPDATE,DELETE,TRUNCATE')`、`rolsuper`、`rolbypassrls`、对 schema 的 `CREATE` | block（同上可覆盖） |
| 是否从库 | `pg_is_in_recovery()` | — |
| CDC 前提 | `wal_level = logical`（RDS `rds.logical_replication = 1`，Cloud SQL `cloudsql.logical_decoding = on`）；`REPLICATION` 属性或托管库的等价角色；publication 已由 DBA 创建且包含所选表 | 不满足 → 回退批量 + DBA 清单 |
| 复制槽风险 | `max_slot_wal_keep_size`（默认 -1 即无限保留）、磁盘余量 | `-1` → warn：CDC 停摆时 WAL 会撑满源库磁盘，建议 DBA 设置上限 |
| 从库上做逻辑解码 | PG 16+ 支持在备库创建逻辑复制槽 | 源库 ≥16 且 `read_from: replica` 时优先使用 |
| TOAST 与 replica identity | 各表 `relreplident`；有大字段的表若为 `default`，UPDATE 事件中未变化的 TOAST 列不会携带值 | warn，Loader 按“保留原值”处理（见 [04 §5.3](04-pipeline.md)） |

### 5.3 DBA 操作清单（生成物）

平台根据体检结果生成一份可以直接交给 DBA 的清单，例如：

```sql
-- MySQL（在只读从库上）
-- my.cnf: binlog_format=ROW, binlog_row_image=FULL, gtid_mode=ON, enforce_gtid_consistency=ON,
--         log_replica_updates=ON, binlog_expire_logs_seconds=604800
CREATE USER 'oo_cdc'@'10.0.2.%' IDENTIFIED BY '<由 DBA 设置>';
GRANT SELECT, RELOAD, SHOW DATABASES, REPLICATION SLAVE, REPLICATION CLIENT ON *.* TO 'oo_cdc'@'10.0.2.%';
-- RDS: CALL mysql.rds_set_configuration('binlog retention hours', 168);

-- PostgreSQL
-- postgresql.conf: wal_level=logical, max_slot_wal_keep_size='50GB'
CREATE ROLE oo_cdc WITH LOGIN REPLICATION PASSWORD '<由 DBA 设置>';
GRANT USAGE ON SCHEMA public TO oo_cdc;
GRANT SELECT ON public.orders, public.customers TO oo_cdc;
CREATE PUBLICATION oo_pub FOR TABLE public.orders, public.customers;
```

清单里标注每一项的**风险与回滚方式**（例如 binlog 保留时长对磁盘的影响），因为 DBA 最关心的是“会不会伤到生产库”。

## 6. 类型映射

### 6.1 原则

1. **不丢信息优先于“好看”：** 无法无损映射时选更宽的类型，把语义放进 Ontology（例如 `tinyint(1)` 映射为 `smallint`，若画像显示取值只有 0/1，Ontology 标注为 `boolean_flag`）。
2. **两条路径一致：** CDC 与批量对同一列必须产出同一 Arrow/PG 类型。
3. **异常值有明确策略：** 零日期、非法编码、NUL 字节等都由 `type_policy` 控制，并在 Ontology 中自动生成对应的 `caveats`。对易发生转换错误的列先读取原始字符串/字节，再显式转换，禁止驱动隐式吞掉异常；置空、替换或删除非法字符均保留原行并逐值写入控制库 `sync.rejects`，敏感或未分类值仅记录掩码/长度。拒绝汇总随数据写入 Agent DB，查询与答案携带质量警告（[04 §5.6](04-pipeline.md)、[07 §5.4](07-gateway.md)）。

### 6.2 MySQL → PostgreSQL

| MySQL | PostgreSQL | 说明 |
|---|---|---|
| `tinyint(1)` | `smallint` | 默认不转 boolean（可能存 0–127）；`tinyint1: boolean` 可覆盖 |
| `tinyint/smallint/mediumint/int/bigint` | `smallint/integer/integer/integer/bigint` | — |
| `* unsigned` | 升一级；`bigint unsigned` → `numeric(20,0)` | 防溢出 |
| `decimal(p,s)` | `numeric(p,s)` | — |
| `float/double` | `real/double precision` | — |
| `bit(1)` / `bit(n)` | `boolean` / `bit varying(n)` | — |
| `char/varchar/text/*text` | `text` | 长度约束不复制（Agent 不需要） |
| `enum(...)` | `text` | 取值集合写入 Ontology 码表候选 |
| `set(...)` | `text[]` | — |
| `date` | `date` | 零日期 `'0000-00-00'` → `NULL`（默认），另可选保留原文到 `<col>__raw` |
| `datetime(n)` | `timestamp(n)` | 无时区，按源 `timezone` 解释；Ontology 标注时区 |
| `timestamp(n)` | `timestamptz(n)` | MySQL 以 UTC 存储 |
| `time` | `interval` | MySQL `time` 可超过 24 小时 |
| `year` | `smallint` | — |
| `json` | `jsonb` | — |
| `binary/varbinary/blob` | `bytea` | 大对象列默认排除（体积大、对 Agent 无用） |
| `geometry` 等空间类型 | `text`（WKT） | 装有 PostGIS 时可映射为 `geometry` |
| 字符串中的 `\0` | 删除（`strip_nul: true`） | PG `text` 不接受 NUL 字节 |

### 6.3 PostgreSQL → PostgreSQL

基本一一对应。例外：自定义 `DOMAIN` 映射为基础类型；扩展类型（PostGIS、hstore、citext、ltree）在 Agent DB 未安装对应扩展时映射为 `text`；`money` → `numeric(19,2)`（去掉 locale 依赖）；数组保持数组。

### 6.4 后续数据库的已知陷阱（接入时处理）

| 源 | 陷阱 | 处理 |
|---|---|---|
| Oracle | `DATE` 含时分秒；裸 `NUMBER` 无精度；空字符串等于 `NULL` | `DATE` → `timestamp(0)`；裸 `NUMBER` → `numeric`；在 Ontology 中说明空串语义 |
| SQL Server | `datetime` 精度 1/300 秒；`datetimeoffset`；`uniqueidentifier`；`money` | → `timestamp(3)`、`timestamptz`、`uuid`、`numeric(19,4)` |
| Snowflake / BigQuery | `VARIANT` / `STRUCT` / `ARRAY` | → `jsonb` / `jsonb` / 数组或 `jsonb` |

## 7. 文件接入与托管

### 7.1 上传

- 控制台与 API 支持断点续传（分片上传，单文件默认上限 5 GB，可配置）。
- 原件存入对象存储：`uploads/<upload_id>/<sha256>/original`（原文件名只作为展示元数据），保留期默认 180 天，可配置；同一内容（sha256 相同）不重复存储。
- 安全处理：使用隔离的有界解析 worker（[17 §4](17-rust-implementation.md)），只按内容嗅探格式，不信任扩展名；压缩包检查解压比（防压缩炸弹）和路径穿越；`.xlsm` 等含宏文件只读数据、不执行宏；可选接入 ClamAV 扫描。

### 7.2 解析

```text
原件 ──▶ 格式识别 ──▶ 编码识别（文本类）──▶ 结构识别 ──▶ 预览与确认 ──▶ 类型推断 ──▶ Arrow ──▶ Loader
```

| 格式 | 解析 | 要点 |
|---|---|---|
| CSV / TSV | DuckDB `read_csv`（嗅探分隔符、表头、类型） | 编码先用 Rust `chardetng` 提出候选，再按解析配置用 `encoding_rs` 严格解码；不允许静默替换非法字节；中文文件选择 GBK/GB18030 时需验证 `encoding_rs` 锁定版本对四字节序列的支持，并在预览中确认；统一转 UTF-8 |
| xlsx | Rust `calamine`（统一 Excel 读取路径） | 每个 sheet 一张表；合并单元格与多行表头先预览确认；区分 Excel 1900/1904 日期系统；只读公式缓存，缺缓存则报告，不执行宏、公式或外部链接 |
| xls（旧格式） | Rust `calamine` 后转 Arrow | 有界工作进程解析，限制工作簿展开大小、单元格数量；不承诺任意 5 GB Excel 均可导入 |
| Parquet | Rust `parquet` / Arrow reader | 类型直接采用；限制单个 row group 解压内存与嵌套深度 |
| JSON Lines | DuckDB `read_json` | 嵌套对象默认保留为 `jsonb`，可选展开 |
| SQLite | SQLx SQLite 驱动，以只读模式打开上传副本 | 每张表一张表；不加载扩展、不附加外部数据库 |
| SQL dump | 在一次性容器中启动对应版本的 MySQL/PG 恢复 dump，然后以 `full` 模式作为批量源读取，完成后销毁容器 | 不在 Agent DB 里直接执行 dump 中的 SQL（dump 可以包含任意语句） |

文件类型推断后的转换也遵循 `type_policy`：解析失败的单元格不得静默置空或丢行，按源文件、行标识和列定位拒绝记录；结构无法解析时停止并报告错误。

预览页展示前 100 行、推断出的列名与类型，用户可修改列名、类型、表头行、要忽略的行，确认结果保存为该上传集的解析配置（下次上传同一模板的文件时复用）。

### 7.3 更新语义

```yaml
files:
  finance_ar_2026:
    schema: files_finance_2026
    mode: replace            # replace（整表替换）| append（追加）| upsert（按 key 合并）
    key: [invoice_no]        # upsert 时必填
    parse_profile: ar_monthly_v2
```

每次上传是一个版本；`replace` 通过影子表 + 事务内换表完成（见 [04 §6](04-pipeline.md)）。文件来源的表在 Ontology 中 `tier: file`，并记录原件 sha256 与上传人，便于回答“这个数来自哪份文件”。

### 7.4 结果文件托管

同一个对象存储还承载：导出结果（Gateway `export` 产生的中间文件由客户端在沙盒内写入，不经服务端落盘）、会话产出物（`/workspace/outputs/` 回收后的图表、CSV）、会话记录、Knowledge Pack。按前缀设置不同保留期，见 [11 运维](11-deployment-and-operations.md)。

## 8. 数据库支持路线

> 本节已被[统一准入与支持矩阵](database-support/01-admission-and-support-matrix.md)取代：波次、同步方式和 DBA 动作以[逐库画像](database-support/04-database-profiles.md)与[支持矩阵](database-support/support-matrix.yaml)为准。下表保留作历史对照；其中“接入一种 Debezium 支持的数据库只需实现连接器”的说法过于乐观，不再成立（写入分类、保留窗口、位点序、一致性等级、驱动与许可都需逐目标验收）。

| 顺序 | 数据库 | 同步方式 | DBA 要做的事 |
|---|---|---|---|
| 第一版 | PostgreSQL（含托管） | CDC（WAL） | `wal_level=logical`，复制权限，创建 publication，建议设置 `max_slot_wal_keep_size` |
| 第一版 | MySQL / MariaDB（含托管） | CDC（binlog） | ROW + FULL + GTID，复制权限，托管库调长 binlog 保留 |
| 第一版 | 文件、SQLite、SQL dump | 上传 | 无 |
| 第二批 | SQL Server（含 Azure SQL） | CDC（CDC 表） | 库与表开启 CDC，SQL Server Agent 运行 |
| 第二批 | Snowflake、BigQuery、Databricks、Redshift、ClickHouse | 批量增量 | 只需 `SELECT` |
| 第三批 | Oracle | CDC（LogMiner） | 归档日志 + 补充日志，权限较多 |
| 规划中 | MongoDB | CDC | 副本集 / change streams |

接入一种 Debezium 支持的数据库，需要实现连接器、类型/位点/事务映射和 DBA 清单，并验证事件兼容性；可复用缓冲、Loader、Agent DB 与 Gateway 的公共契约。每个新连接器必须通过连接器一致性测试套件（类型映射双路径一致、分块覆盖无重叠、对账零差异）才能进入发布版本。
