# 03 数据源、连接器与文件接入

> Status: Proposed
> Owner: Open Ontology Engineering
> Last reviewed: 2026-09-30
> 依赖：[01 架构](01-architecture.md)、[02 技术栈](02-technology-stack.md)
> 被依赖：[04 管线](04-pipeline.md)、[06 Ontology](06-ontology-layer.md)

## 1. 职责与不变量

登记企业已有的数据库与文件，让管线知道读什么、怎么读。

- **I-SRC-1** 平台对源库只读：不写业务数据，不在源库建表、视图、触发器、存储过程。唯一的例外是 DBA 按清单**手工**创建的 CDC 所需对象（PG publication / Debezium signaling 表），平台代码不自动创建它们。
- **I-SRC-2** 凭证不以明文出现在配置中；只以 `secret://` 引用。
- **I-SRC-3** 能写业务表的凭证默认拒绝登记。
- **I-SRC-4** 所有读取受限速，优先从只读从库读。

“任何数据库”的承诺按以下方式兑现：**CDC 能力取决于 Debezium 连接器，批量能力只要求能执行 `SELECT`**，所以任何能用 SQL 读取的库都能以批量方式接入；文件和数据库导出文件（dump）作为第三条路径。

## 2. 源的类型

| 类型 | 例子 | 同步方式 | 第一版 |
|---|---|---|---|
| 关系数据库（可 CDC） | MySQL / MariaDB、PostgreSQL（含 RDS、Aurora、Cloud SQL、AlloyDB、Azure） | CDC + 每日对账 | ✓ |
| 关系数据库（不可 CDC） | 同上但无法开日志；以及任何有 SQL 接口的库 | 批量增量（`updated_at` 水位）+ 每日对账 | ✓ |
| 数据仓库 | Snowflake、BigQuery、Databricks、Redshift、ClickHouse | 批量增量（ADBC 读取） | 第二批 |
| 需要额外配置的 CDC 库 | SQL Server（CDC 表）、Oracle（LogMiner） | CDC | 第二、三批 |
| 文档库 | MongoDB | CDC（Debezium）→ `jsonb` 列 + 展开视图 | 规划中 |
| 文件 | CSV / TSV / Excel（xlsx、xls）/ Parquet / JSON Lines | 上传 → 解析 → 全量替换或按键合并 | ✓ |
| 数据库文件 / 导出 | SQLite 文件、`mysqldump` / `pg_dump` 的 SQL 文件 | 恢复到临时实例 → 当作批量源全量读取 | ✓（M1） |

## 3. 注册机制

### 3.1 插件与实例

- **插件**通过 Python entry point（`oo.connectors`、`oo.triggers`、`oo.hooks`、`oo.secrets`、`oo.notifiers`、`oo.harnesses`、`oo.sandboxes`）或 `plugins/` 目录自动发现。
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

```python
class Connector(Protocol):
    name: ClassVar[str]
    capabilities: ClassVar[set[Capability]]   # CDC, INCREMENTAL, FULL, READ_LOGIC, REPLICA_READ

    def check_access(self) -> AccessReport: ...
    def list_tables(self) -> list[TableRef]: ...
    def read_schema(self, tables: list[TableRef]) -> list[TableSchema]: ...   # 列、类型、主键、唯一键、外键、注释
    def read_logic(self) -> list[LogicObject]: ...                            # 视图定义、存储过程正文（给 Ontology Builder）
    def estimate(self, table: TableRef) -> TableStats: ...                    # 行数、体积（来自源库统计信息，不 COUNT(*)）
    def plan_chunks(self, table: TableRef, target_rows: int) -> list[Chunk]: ...
    def read_chunk(self, chunk: Chunk) -> Iterator[pa.RecordBatch]: ...
    def read_incremental(self, table: TableRef, cursor: Cursor) -> Iterator[pa.RecordBatch]: ...
    def read_keys(self, table: TableRef, key_range: KeyRange) -> Iterator[pa.RecordBatch]: ...  # 对账用
    def type_map(self, col: SourceColumn, policy: TypePolicy) -> ColumnMapping: ...
    def cdc_config(self) -> DebeziumConfig | None: ...
    def position_codec(self) -> PositionCodec: ...                            # 源位点 ⇄ _oo_pos 的编码
```

- `read_chunk` 返回 Arrow RecordBatch，内部用 ADBC 或 ConnectorX 做列式读取，避免 Python 行对象。
- `type_map` 是**连接器的职责**：批量路径与 CDC 路径必须产出同样的 Arrow 类型，否则对账会误报。每个连接器都要有“同一行经两条路径读取、结果逐字节一致”的一致性测试。
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
- 原件存入对象存储：`uploads/<upload_id>/<sha256>/<原文件名>`，保留期默认 180 天，可配置；同一内容（sha256 相同）不重复存储。
- 安全处理：只按内容嗅探格式，不信任扩展名；压缩包检查解压比（防压缩炸弹）和路径穿越；`.xlsm` 等含宏文件只读数据、不执行宏；可选接入 ClamAV 扫描。

### 7.2 解析

```text
原件 ──▶ 格式识别 ──▶ 编码识别（文本类）──▶ 结构识别 ──▶ 预览与确认 ──▶ 类型推断 ──▶ Arrow ──▶ Loader
```

| 格式 | 解析 | 要点 |
|---|---|---|
| CSV / TSV | DuckDB `read_csv`（嗅探分隔符、表头、类型） | 编码先用 charset-normalizer 识别；中文文件识别为 GBK 时按 **GB18030** 解码（GBK 的超集，减少乱码）；统一转 UTF-8 |
| xlsx | DuckDB `excel` 扩展（简单表格）；复杂表头走 openpyxl | 每个 sheet 一张表；识别合并单元格与多行表头，给出候选表头行，由用户在预览中确认；空行/合计行识别 |
| xls（旧格式） | xlrd 读取后转 Arrow | DuckDB 不支持 `.xls` |
| Parquet | DuckDB `read_parquet` | 类型直接采用 |
| JSON Lines | DuckDB `read_json` | 嵌套对象默认保留为 `jsonb`，可选展开 |
| SQLite | DuckDB `sqlite` 扩展 | 每张表一张表 |
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

| 顺序 | 数据库 | 同步方式 | DBA 要做的事 |
|---|---|---|---|
| 第一版 | PostgreSQL（含托管） | CDC（WAL） | `wal_level=logical`，复制权限，创建 publication，建议设置 `max_slot_wal_keep_size` |
| 第一版 | MySQL / MariaDB（含托管） | CDC（binlog） | ROW + FULL + GTID，复制权限，托管库调长 binlog 保留 |
| 第一版 | 文件、SQLite、SQL dump | 上传 | 无 |
| 第二批 | SQL Server（含 Azure SQL） | CDC（CDC 表） | 库与表开启 CDC，SQL Server Agent 运行 |
| 第二批 | Snowflake、BigQuery、Databricks、Redshift、ClickHouse | 批量增量 | 只需 `SELECT` |
| 第三批 | Oracle | CDC（LogMiner） | 归档日志 + 补充日志，权限较多 |
| 规划中 | MongoDB | CDC | 副本集 / change streams |

加一种 Debezium 支持的数据库，只需新写一个连接器和一份 DBA 清单模板；缓冲、Loader、Agent DB、Gateway 都不变。每个新连接器必须通过连接器一致性测试套件（类型映射双路径一致、分块覆盖无重叠、对账零差异）才能进入发布版本。
