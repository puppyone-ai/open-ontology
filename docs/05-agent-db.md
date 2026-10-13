# 05 Agent DB

> Status: Proposed
> Owner: Open Ontology Engineering
> Last reviewed: 2026-10-01
> 依赖：[01 架构](01-architecture.md)、[04 管线](04-pipeline.md)
> 被依赖：[06 Ontology](06-ontology-layer.md)、[07 Gateway](07-gateway.md)

## 1. 职责与不变量

Agent DB 是一个**独立的 PostgreSQL 实例（主 + 流复制只读从库）**，存放同步来的数据，只服务 Agent。

- **I-DB-1** 只有管线（`pipeline_rw`）能写主库；Gateway 只连只读从库。
- **I-DB-2** 所有面向 Agent 的读取都以某个 `oo_r_<role>` 角色执行，受表/列 `GRANT` 与 RLS 约束，这是权限的**权威执行点**（应用层检查只是前置过滤，见 [07](07-gateway.md)）。
- **I-DB-3** Agent DB 中的数据可以从源库完全重建；它不是任何业务数据的真相源。
- **I-DB-4** 部分写入的数据对 Agent 不可见：首次快照、重做快照完成前，表不授予任何 `oo_r_*` 角色。
- **I-DB-5** 对 `oo_r_*` 只做**列级授权**，禁止表级 `GRANT SELECT ON <table>`：PG 的表级 SELECT 权限自动覆盖之后新增的列，会让源库新加的敏感列未经分类就对 Agent 可见。只有在已发布 Ontology 中完成分类（`pii`、`semantic_type` 已确定）的列才会被授权。`oo apply` 与每日权限巡检都校验“`information_schema.table_privileges` 中不存在授予 `oo_r_*` 的表级 SELECT”。

## 2. 版本与实例

| 项 | 选择 | 说明 |
|---|---|---|
| 版本 | 最低 16，默认 17，18 在原型中验证后切换 | `MERGE` 自 15 起可用；17 支持 `MERGE ... RETURNING` 与 failover slot；18 的异步 I/O 对顺序扫描型分析查询有收益，需实测 |
| 拓扑 | 1 主 + ≥1 从（流复制，异步） | 小规模 all-in-one 可以只有主库，此时 Gateway 读主库，限流加严 |
| 扩展 | `pg_trgm`（值检索）、`pg_stat_statements`（慢查询分析）；可选 `pg_duckdb`（M2 评估） | 扩展必须同时装在主从镜像中 |
| 镜像 | 固定 minor 版本的官方镜像 + 上述扩展 | 版本写入会话记录与评测记录 |

## 3. 命名空间布局

```text
agentdb
├─ <source>                 # 每个数据源一个 schema，源表一比一副本，例如 erp_mysql.kh
│    └─ <table>             # 源列 + 系统列 _oo_pos, _oo_synced_at
├─ <source>__stage          # Loader 的暂存/影子表（UNLOGGED），从不授权给 Agent
├─ files_<upload>           # 文件上传源，一个上传集一个 schema
├─ <scene>                  # 场景 schema，由 after_load hook 构建，例如 logistics.customer
├─ oo_dict                  # 由已发布 Ontology 物化的码表与财务日历，Agent 可读（见 §6.3）
├─ oo_entity                # 实体解析的成员映射与规范实体表，Agent 可读（见 §6.4）
└─ _oo                      # 平台保留的派生 schema，Agent 不可读（见 §6）
```

`oo_dict` 与 `oo_entity` 和 `_oo` 一样由平台维护、可随时从 Ontology 与控制库重建，但它们是**给 Agent 用的**：统计口径和 JOIN 可以直接引用它们，而不是让模型从 Markdown 里抄码值。

### 3.1 标识符规范化

- 源库标识符统一转为小写；非 `[a-z0-9_]` 字符替换为 `_`；与 PG 保留字冲突或规范化后重名时追加 `_2` 等后缀。
- 原始名称与规范化名称的映射存入控制库，并写进 Ontology 的 `source_name` 字段，Agent 回答时可以引用业务人员熟悉的原名。
- MySQL 的 `lower_case_table_names` 差异、PG 的带引号大小写标识符都在连接器中处理，不泄漏到 Agent DB。

### 3.2 系统列

每张副本表只增加两个系统列：

| 列 | 类型 | 含义 |
|---|---|---|
| `_oo_pos` | `numeric(38,0)` | 该行最后一次变更的**源位点**，单源内按提交顺序单调可比；首次快照写入的行为 0。编码（PG 用事务提交 LSN、MySQL 用 binlog 文件序号与偏移、主从切换后的 epoch、批量路径用 `updated_at`）见 [04 §5.1](04-pipeline.md) |
| `_oo_synced_at` | `timestamptz` | 该行写入 Agent DB 的时间 |

系统列对 Agent 可见但在 Knowledge 中标注为“系统列，不用于业务分析”。

## 4. 角色与权限模型

```text
oo_owner          NOLOGIN   拥有全部 schema 与表；没有人以它登录
pipeline_rw       LOGIN     可 SET ROLE oo_owner；仅管线与 oo apply 使用；只能连主库
oo_builder_ro     LOGIN     只读画像；只在 Builder 中使用；受限速
oo_gateway        LOGIN     NOINHERIT；自身无任何对象权限；是所有 oo_r_* 的成员，仅用于 SET ROLE
oo_gateway_sys    NOLOGIN     只读 _oo schema（新鲜度）与 pg_catalog 元数据；oo_gateway 可 SET ROLE 到它，仅平台固定 SQL 使用
oo_r_<role>       NOLOGIN   由 roles.yaml 生成：schema USAGE、表/列 SELECT、RLS 策略的 TO 目标
```

### 4.1 从 `roles.yaml` 到数据库对象

`oo apply` 把声明式角色（见 [12 配置参考](12-configuration-reference.md#roles)）编译成幂等的 DDL，在主库以 `oo_owner` 执行，经流复制同步到从库：

```sql
-- 角色
CREATE ROLE oo_r_dispatcher_eu NOLOGIN;
GRANT oo_r_dispatcher_eu TO oo_gateway WITH INHERIT FALSE, SET TRUE;
-- oo_gateway_sys 也按此方式授权给 oo_gateway，不授给任何 oo_r_*

-- 只做列级授权（I-DB-5）：tables: ["*"] 在编译时展开为“已分类列”的显式列表
GRANT USAGE ON SCHEMA logistics TO oo_r_dispatcher_eu;
GRANT SELECT (shipment_id, customer_id, region, status, picked_at, signed_at) ON logistics.shipment TO oo_r_dispatcher_eu;
GRANT SELECT (customer_id, name, region) ON logistics.customer TO oo_r_dispatcher_eu;  -- 不含 phone（角色排除）
-- 源库后来新增的列（例如 logistics.customer.id_card）不在任何列表中，直到分类并发布

-- 行级
ALTER TABLE logistics.shipment ENABLE ROW LEVEL SECURITY;
CREATE POLICY oo_p_dispatcher_eu_shipment ON logistics.shipment
  FOR SELECT TO oo_r_dispatcher_eu
  USING (region = ANY (_oo.oo_claim_array('region')));
```

`_oo.oo_claim_array(key)` 是 `_oo` schema 中由平台创建的 `STABLE` SQL 函数，读取 Gateway 在事务内设置的 `oo.claims`（JSON）：

```sql
CREATE FUNCTION _oo.oo_claim_array(k text) RETURNS text[]
LANGUAGE sql STABLE PARALLEL SAFE AS $$
  SELECT coalesce(array(SELECT jsonb_array_elements_text(
           current_setting('oo.claims', true)::jsonb -> k)), '{}')
$$;
```

`oo apply` 对需要 RLS 的角色仅授予 `_oo` USAGE 与指定 claims helper 的 EXECUTE，不授底表 SELECT；创建函数后显式撤销 PUBLIC EXECUTE，再按策略授权（与 §6.5 一致）。

未设置 `oo.claims` 时返回空数组，策略即“看不到任何行”——**失败即关闭（fail closed）**。

### 4.2 为什么 claims 不能被 Agent 伪造

`SET` 自定义 GUC 对任何角色都是允许的，所以必须证明 Agent 的 SQL 改不了 `oo.claims`。防线分三层，需要共同成立；单语句本身挡不住 `SELECT set_config(...)`，事务回滚也挡不住同语句内先改 claims 再读数据：

1. **单语句：** Gateway 用扩展查询协议（Parse/Bind/Execute）发送 Agent 的 SQL，PG 在协议层拒绝多语句字符串（`cannot insert multiple commands into a prepared statement`）。Rust Gateway 使用 `tokio_postgres::Transaction::prepare()`，再 `bind()` / `query_portal()` 执行预备语句；禁止把 Agent SQL 交给 `simple_query()` / `batch_execute()`。由集成测试断言空参数、多语句与可写 CTE 都被拒绝。
2. **每查询一个事务：** Gateway 的执行序列固定为
   `BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; SET LOCAL ROLE oo_r_x; SET LOCAL oo.claims = '<json>'; <agent_stmt>; ROLLBACK;`
   即使 `<agent_stmt>` 是一条 `SET`，它也只能影响这个即将回滚的事务，且顶层 SET 不能和读取拼成单条预备语句；表达式内修改 GUC 还须由第 3 层阻止。
3. **函数级撤权：** 对 `PUBLIC` 撤销 `set_config(text,text,boolean)` 的 `EXECUTE`（阻断内置的表达式级 GUC 修改入口；同时禁止角色执行可修改 GUC 的自定义函数/扩展）。`oo_r_*` 角色因此在 SQL 表达式里改不了 `oo.claims` 或 `role`。

外加应用层：基于 PG 语法分析器的检查只放行单条 `SELECT`/`WITH`，并拒绝 `set_config`、`SET`、`RESET` 等（见 [07 §4](07-gateway.md)）。

### 4.3 RLS 的已知边界

| 风险 | 处理 |
|---|---|
| 非 `LEAKPROOF` 函数在策略谓词之前求值导致侧信道 | PG 默认先求 RLS 条件，只有 `LEAKPROOF` 函数可下推到策略前；平台不创建任何 `LEAKPROOF` 自定义函数 |
| 场景视图绕过 RLS（视图以 owner 权限执行） | 场景视图一律 `WITH (security_invoker = true)`（PG15+），`oo validate` 拒绝未设置的视图 |
| 物化视图 / 场景表不继承源表 RLS | 场景表在 `roles.yaml` 中必须单独声明行级过滤；未声明且源表有过滤时 `oo validate` 报错 |
| 通过报错信息推断隐藏行（例如除零） | RLS 条件先于用户谓词求值，隐藏行不会进入用户表达式；Gateway 对错误信息做脱敏（见 07） |
| 统计信息泄漏（`pg_stats` 中的高频值） | `oo_r_*` 无权读 `pg_statistic`；`pg_stats` 视图只显示有 SELECT 权限的列，且 Gateway 拒绝系统目录直接访问 |
| `EXPLAIN` 的行数估计泄露隐藏行规模 | Gateway 不向 Agent 返回原始 EXPLAIN 输出，只返回“代价过高”类结论 |

### 4.4 会话级资源参数

```sql
ALTER ROLE oo_gateway SET default_transaction_read_only = on;
ALTER ROLE oo_gateway SET statement_timeout = '30s';      -- Gateway 按请求再用 SET LOCAL 收紧
ALTER ROLE oo_gateway SET idle_in_transaction_session_timeout = '10s';
ALTER ROLE oo_gateway SET work_mem = '64MB';
ALTER ROLE oo_gateway SET temp_file_limit = '2GB';        -- 需超级用户设置
ALTER ROLE oo_gateway SET search_path = '';                -- 强制全限定名，见 07
ALTER ROLE oo_gateway CONNECTION LIMIT 200;
```

## 5. 从库与热备冲突

Agent 查询常是秒级到数十秒级的分析型查询，而主库持续有 CDC 写入和 VACUUM，这会触发热备冲突（`canceling statement due to conflict with recovery`）。

| 参数 | 取值 | 权衡 |
|---|---|---|
| `max_standby_streaming_delay` | ≥ Gateway 最大 `statement_timeout` + 余量（默认 90s） | 回放可延迟到这个时长，从库新鲜度会相应落后 |
| `hot_standby_feedback` | `on` | 避免清理冲突；代价是主库膨胀，需配合长查询上限 |
| Gateway `statement_timeout` 上限 | 普通 30s，导出 120s | 是上面两个参数的约束来源 |

**新鲜度必须包含从库回放延迟：** Gateway 返回的“数据截至时间”取 `_oo.table_freshness.source_committed_at`（从库上读取，天然与从库已回放的数据一致，见 §6.1），并附带 `now() - pg_last_xact_replay_timestamp()` 作为从库延迟指标。

## 6. `_oo` 平台保留 schema

参考草案要求“平台不在 Agent DB 建任何表”。本设计放宽为：**允许平台维护的派生 schema**——Agent 不可读的 `_oo`，以及 Agent 可读的 `oo_dict`、`oo_entity`。它们都可以随时重建，不是业务数据的真相源。放在库里而不是库外，是因为新鲜度、水位、质量状态必须和数据处于同一事务、同一副本，否则会产生一致性问题；码表、日历、实体映射则需要能被 SQL 直接 JOIN。

### 6.1 `_oo.table_freshness`

```sql
CREATE TABLE _oo.table_freshness (
  schema_name  text,
  table_name   text,
  source       text,
  last_pos     numeric(38,0),
  source_committed_at timestamptz,   -- 最后一条已应用变更在源库的提交时间
  applied_at   timestamptz,          -- Loader 提交时间
  mode         text,                 -- cdc | incremental | full | file
  state        text,                 -- snapshotting | catching_up | awaiting_publish | live | paused | error
  PRIMARY KEY (schema_name, table_name)
);
```

Loader 在**同一事务**中写数据和更新这一行，因此任何从库上读到的新鲜度都精确描述该从库上的数据。控制库保留一份镜像用于监控和告警。

### 6.2 `_oo.source_watermark` 与 `_oo.table_quality`

```sql
CREATE TABLE _oo.source_watermark (
  source        text PRIMARY KEY,
  consistency   text,                 -- transactional | per_table（仅 T 级源有效）
  consistency_grade text,             -- T | S1 | S0 | B（见 database-support/03 §3）
  applied_pos   numeric(38,0),        -- transactional：最后完整水位；仅 consistent=true 代表当前切面
  source_committed_at timestamptz,
  applied_at    timestamptz,
  state         text,                 -- live | lagging | applying_large_txn | paused_tables
  cross_table_consistent boolean
);

CREATE TABLE _oo.table_quality (
  schema_name   text,
  table_name    text,
  rejects_open  integer,              -- 未解决的类型转换拒绝行数（按表）
  rejects_by_column jsonb,            -- {"fhrq": {"zero_date": 312}, …}
  reconcile_status text,              -- ok | mismatch | stale | never
  reconcile_at  timestamptz,
  reconcile_diff_rows bigint,
  unexplained_codes jsonb,            -- {"zt": ["6","7"]}：数据中出现、已发布码表未解释的值
  hidden_columns integer,             -- 已同步但尚未分类、对 Agent 不可见的列数
  partial_txn_in_progress boolean,    -- 超大源事务正在分段写入
  entity_check  text,                 -- ok | failed | n/a：实体解析不变量（§6.4）
  capture_method text,                -- c1_cdc | c2_change_feed | c3_cursor | c4_partition | c5_diff
  delete_detection text,              -- native | partition_replace | reconcile | none
  position_state text,                -- ok | stale_risk | expired（POSITION_EXPIRED）
  budget_state  text,                 -- ok | exhausted（SOURCE_BUDGET_EXHAUSTED）
  updated_at    timestamptz,
  PRIMARY KEY (schema_name, table_name)
);
```

写入规则见 [04 §5.7](04-pipeline.md)。Gateway 在每次查询时按引用的表读取这两张表（从库上，与数据同一可见性），把需要告知的状态作为质量警告返回（[07 §5.4](07-gateway.md)）。

### 6.3 `oo_dict`：物化的码表与财务日历

已发布 Ontology 中的码表与日历由 `oo apply` 物化进 `oo_dict`，Agent 与认证指标都可以直接 JOIN，而不是把码值抄进 SQL：

```sql
CREATE TABLE oo_dict.enum_values (
  enum_name   text,            -- 例如 erp_mysql.ysk.zt
  code        text,
  label       text,
  class       text,            -- 码表自定义的业务类别，例如 open / closed / void；未释义为 'unexplained'，待判定为 'undetermined'
  status      text,            -- approved | undetermined | unexplained | superseded | deprecated
  valid_from  date NOT NULL DEFAULT '-infinity'::date, -- 无下界；主键列不可为 NULL
  valid_to    date,            -- NULL 表示当前仍有效
  PRIMARY KEY (enum_name, code, valid_from),
  CHECK (valid_to IS NULL OR valid_to > valid_from)
);
-- 每个码表一个便捷视图（security_invoker），例如 oo_dict.e_erp_mysql__ysk__zt
```

- 码值巡检任务把数据中出现、但码表中没有的值以 `status = 'unexplained'`、`class = 'unexplained'` 写入，保证“每个出现过的码在字典里都有一行”，JOIN 不会静默丢行；
- 视图的授权跟随被解释的列：看得到 `erp_mysql.ysk.zt` 的角色才看得到它的码表视图。

发布门禁与码值巡检写入时均校验：同一 `(enum_name, code)` 的有效期为左闭右开区间且不得重叠，避免 JOIN 重复计数。共享码表表须按角色限制可见 enum_name；便捷视图使用 security_invoker，不能依赖视图过滤代替底表权限。

```sql
-- 财务日历：命名锚点，Agent 与指标只引用名字，不自己推算日期
CREATE VIEW oo_dict.calendar WITH (security_invoker = false, security_barrier = true) AS
SELECT d.today,
       date_trunc('month', d.today)::date                        AS month_start,
       (date_trunc('month', d.today) - interval '1 month')::date AS prev_month_start,
       (date_trunc('month', d.today) - interval '1 day')::date AS prev_month_end,
       date_trunc('quarter', d.today)::date AS quarter_start,
       (date_trunc('quarter', d.today) - interval '3 months')::date AS prev_quarter_start,
       (date_trunc('quarter', d.today) - interval '1 day')::date AS prev_quarter_end,
       f.fiscal_year_start,
       (f.fiscal_year_start - interval '1 day')::date AS prev_fiscal_year_end,
       c.last_closed_period_end                                   -- 财务结账时在控制台更新
FROM _oo.calendar_config c,
     LATERAL (SELECT coalesce(nullif(current_setting('oo.as_of', true), '')::date,
                              (now() AT TIME ZONE c.business_tz)::date) AS today) d,
     LATERAL (SELECT make_date(
       extract(year FROM d.today)::integer
         - CASE WHEN extract(month FROM d.today) < c.fiscal_year_start_month THEN 1 ELSE 0 END,
       c.fiscal_year_start_month, 1) AS fiscal_year_start) f;
```

`oo.as_of` 只在评测模式下由 Gateway 设置（把“今天”固定到题目的 `as_of`，[13 §5](13-evaluation-and-benchmarks.md)）；生产中不设置，`today` 按业务时区滚动。

日历是业务视图使用 `security_invoker` 规则的唯一受限例外：视图由专用 NOLOGIN、非超级用户且无 BYPASSRLS 的角色拥有，仅授予 `_oo` USAGE 与 `_oo.calendar_config` 所需列的 SELECT，不持有业务表权限，`oo_r_*` 不得成为其成员。Agent 只获日历输出列的 SELECT，无权读取配置底表；避免 invoker 视图因底表未授权而报错。配置须为单行，财年起始月限定 1–12；此处仅展示月/季度/财年锚点，周锚点按 calendar.yaml 的 week_start 构建。

### 6.4 `oo_entity`：实体解析结果

实体解析（[06 §3.10](06-ontology-layer.md)）的结果按实体物化：

| 表 | 内容 |
|---|---|
| `oo_entity.<entity>` | 每个 `entity_key` 一行：规范显示名、强键（例如统一社会信用代码）、成员数、状态 |
| `oo_entity.<entity>_members` | 源行 → `entity_key`：`source`、`source_table`、`source_pk`、命中规则或管家决定、生效区间 |
| `oo_entity.<entity>_aliases` | 别名 → `entity_key`：原文、规范化形式、等级（`preferred` / `normal` / `deprecated`）、生效区间、出处 |

- `entity_key` 一经发出不回收、不复用；键值来自控制库中的键登记表（需要备份，见 [11 §5](11-deployment-and-operations.md)），物化表本身可重建。
- 有行级过滤的角色必须为这些表声明对应的过滤（与场景表同一校验规则）。
- 每晚校验两条不变量：每个源行恰好属于一个 `entity_key`；按 `entity_key` 汇总的度量等于对应源行之和。失败时相关表的 `_oo.table_quality.entity_check = 'failed'`，答案带质量警告，直到处理完成。

### 6.5 其他内容

| 对象 | 用途 |
|---|---|
| `_oo.oo_claim_array()` 等函数 | RLS 策略使用 |
| `_oo.heartbeat` | 管线每 N 秒写一次，用于测量端到端复制延迟 |
| `_oo.schema_version` | 当前 apply 的配置版本、Ontology 发布版本 |
| `_oo.calendar_config` | 业务时区、财年起始月、`last_closed_period_end` |

`_oo` 数据对象只授予 `oo_gateway_sys`（读）和 `oo_owner`；Agent 角色仅获 §4.1 的 schema USAGE/指定 helper EXECUTE，无底表读取权限，另对日历视图所有者授予 §6.3 的最小权限；Agent 角色不可直接读取。数据画像、样本值等**不放**在这里，它们属于 Ontology，经权限过滤后编译进 Knowledge Pack（见 [06 §7](06-ontology-layer.md)）。

## 7. 索引、大表与分区

### 7.1 默认索引策略

| 索引 | 何时建 | 说明 |
|---|---|---|
| 主键 / 源唯一键 B-tree | 总是 | UPSERT 必需 |
| 外键列（声明的或 Ontology 中已确认的关系） | 默认 | 支撑 JOIN |
| 时间列 BRIN | 表 > 1,000 万行且时间列与物理顺序相关（相关度由画像给出） | 占用极小；对范围过滤有效 |
| 时间列 B-tree | 相关度低时 | — |
| `pg_trgm` GIN | Ontology 中 `searchable: true` 的名称/编码类列 | 供 `search_values` 使用 |
| 按慢查询建议 | 由 Builder 根据从库 `pg_stat_statements` 生成建议，人工确认后写入配置 | 不自动建索引 |

索引在快照写完之后、表对 Agent 可见之前创建（`CREATE INDEX` 在影子表上执行，不影响在线表）。

### 7.2 分区：默认不分区

对副本表分区看起来很自然，但有一个硬约束：**PG 分区表上的唯一约束必须包含分区键**。源表主键通常是单列 `id`，不含时间列，于是：

- 要么放弃 `id` 的全局唯一约束（`MERGE` 正确性全部依赖管线）；
- 要么把主键改成 `(id, created_at)`，而 CDC 变更事件按 `id` 查找时要扫描所有分区的索引，写入放大显著。

因此默认**不分区**，大表用 BRIN + 定期 `VACUUM`；满足以下全部条件才对某张表启用分区（按表配置）：

1. 行数 > 1 亿，或有明确的历史数据保留/归档需求；
2. 分区键在源表中不可变（例如 `created_at`）；
3. 源的变更事件中带分区键（CDC 事件中带完整行时满足）。

启用后 Loader 使用 `(pk, partition_key)` 定位行。

### 7.3 高频变更表的维护

```sql
ALTER TABLE erp_mysql.ydh SET (
  autovacuum_vacuum_scale_factor = 0.01,
  autovacuum_analyze_scale_factor = 0.02,
  fillfactor = 90             -- 为 HOT 更新留空间
);
```

批量全量替换后立即 `ANALYZE`，否则 Gateway 的 EXPLAIN 代价估计会失真。

## 8. 分析性能

单表数千万行的聚合在 PG 行存上是秒级到数十秒级。第一版的手段按优先级：

1. **场景表预聚合：** 高频口径（如“按月收款额”）由 `after_load` hook 构建成场景表，Ontology 的指标指向它。
2. **并行查询：** 从库设置 `max_parallel_workers_per_gather = 4` 起步，按核数调整。
3. **正确索引：** 见 §7。
4. **导出后分析：** 大结果走 `export`，在沙盒里用 DuckDB 计算，把计算从数据库挪走。
5. **（M2 评估）列式加速：** 在从库上评估 `pg_duckdb` 等扩展；前提是主从镜像一致、与 RLS 的兼容性经过验证。未验证 RLS 兼容前不启用。

## 9. 容量规划（初始估算）

| 项 | 估算 |
|---|---|
| 存储 | 源数据量 × 1.5（索引）× 1.3（膨胀余量）；另加 WAL 与备份空间 |
| 主库 | CPU 以 Loader 并发为主；内存 `shared_buffers` = 25% RAM |
| 从库 | CPU 核数 ≥ Gateway 全局并发上限 × 平均并行度；内存按热数据集 |
| 连接数 | Gateway 全局池（例如每实例 20，总计 ≤ 100）；不给 Agent 直连 |

具体值在性能测试（5,000 万行表）后更新到 [11 运维](11-deployment-and-operations.md)。

## 10. 备份与重建

- **Agent DB：** 可从源库重建，但数千万行的全量重建需要数小时，且会给源库带来读取压力。生产部署默认用 pgBackRest（或托管库自带快照）做每日全量 + WAL 归档，RPO ≤ 15 分钟、RTO ≤ 1 小时；恢复后管线从 `_oo.table_freshness.last_pos` 与 Debezium 位点中较早者继续，重放是幂等的。
- **控制库：** 必须备份（运行状态、审计索引、评测结果）。
- **演练：** 每季度一次“从备份恢复 + 对账”演练，结果记为 Evidence。
