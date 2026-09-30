# 04 同步管线

> Status: Proposed
> Owner: Open Ontology Engineering
> Last reviewed: 2026-09-30
> 依赖：[03 数据源](03-sources-and-ingestion.md)、[05 Agent DB](05-agent-db.md)
> 被依赖：[06 Ontology](06-ontology-layer.md)、[13 评测](13-evaluation-and-benchmarks.md) 轨道 A

## 1. 职责与不变量

把源数据单向搬进 Agent DB，并持续保持同步（插入、更新、删除、结构变化）。

- **I-PL-1** 管线是唯一持有源库凭证、唯一能写 Agent DB 的组件；对 Agent、Gateway、沙盒网络不可达。
- **I-PL-2** 至少一次投递 + 幂等应用：任何重放都不改变最终结果。
- **I-PL-3** 收敛性：停止源库写入且管线追平后，Agent DB 与源库逐行一致（由对账工具判定）。
- **I-PL-4** 部分完成的表对 Agent 不可见（[05 I-DB-4](05-agent-db.md)）。
- **I-PL-5** 管线故障不能伤到源库：限速、优先从库、监控日志积压。
- **I-PL-6** 源事务原子可见（`consistency: transactional`，CDC 源的默认值）：同一个源事务对多张表的修改，在 Agent DB 中要么全部可见、要么全部不可见（§5.2）；无法保证时（`per_table` 模式、超大事务分段期间），通过 `_oo.source_watermark` 与质量标记如实告知（§5.7）。
- **I-PL-7** 不静默改值：任何类型转换失败、非法值替换都按行记录到拒绝日志，并汇总进 `_oo.table_quality`（§5.6）；行本身保留，不丢行。
- **I-PL-8** 新列默认不可见：源库新增的列只进入 Agent DB，不授予任何 `oo_r_*` 角色，直到完成分类并随 Ontology 发布（§5.4，[05 I-DB-5](05-agent-db.md)）。

## 2. 结构

```text
            可 CDC 的源库                      不可 CDC 的源库              文件
                 │                                  │                        │
   Debezium Server（每源库一个，只做流式）      批量读取 worker          解析 worker
   snapshot.mode = no_data                     （Procrastinate 任务）    （Procrastinate 任务）
                 │ 事务模式：每源库一个 stream        │                        │
                 │ 分表模式：每表一个 stream          │                        │
                 ▼                                  │                        │
          Valkey Streams ──▶ 流式 Loader            │                        │
                        （常驻；事务模式按源分片，     │                        │
                          分表模式按表分片）          │                        │
                 ▲                                  ▼                        ▼
   首次快照/重做快照：批量 worker 按主键分块读取 ─────▶ 变更批次（Arrow：op、_oo_pos、行数据）
                                                    │
                                              on_batch hooks（脱敏/清洗）
                                                    ▼
                                         Agent DB 主库（COPY → 合并）
                                                    ▼
                                    after_load hooks（场景 schema，防抖）
                                                    ▼
                           事件：table_ready / schema_changed / freshness_updated
```

同一张表同一时间只有一个**流式写入者**（事务模式下是该源库的 Loader 分片，分表模式下是该表的分片）；快照 worker 只做“补空”插入（§4），不覆盖流式写入者写过的行，保证顺序语义清晰。

## 3. 统一的变更批次

所有路径输出同一种 Arrow 批次，Loader 不区分来源：

| 列 | 类型 | 说明 |
|---|---|---|
| 业务列… | 由连接器 `type_map` 决定 | CDC 与批量路径必须一致 |
| `_oo_op` | `int8` | `1` 插入/更新（upsert）、`2` 删除、`3` 快照读取（fill） |
| `_oo_pos` | `decimal128(38,0)` | 源位点（见 §5.1） |
| `_oo_src_ts` | `timestamp[us, UTC]` | 源库提交时间（CDC）或读取时间（批量） |
| `_oo_unavailable` | `list<int16>` | 该行中“值不可用”的列序号（PG 未变化的 TOAST 列） |

批次元数据：`source`、`table`、`schema_fingerprint`、`batch_id`、`path`（cdc / snapshot / incremental / file）。

## 4. 首次快照：“CDC 优先，快照补空”

### 4.1 为什么不用 Debezium 自带的快照

| 方式 | 问题 |
|---|---|
| Debezium `initial` 快照 | 阻塞式：全部表拷完才开始流式；不可断点续传；数千万行需数小时，期间 PG 复制槽持续积压 WAL |
| Debezium 增量快照（DDD-3 水位算法） | 需要源库上的 signaling 表并由 Debezium **写入**水位（MySQL 可用基于 GTID 的只读变体，但有已报告的边界竞态）；快照数据全部经过 Valkey，数千万行会把缓冲内存打满，触发连接器整体暂停 |

因此：**Debezium 只做流式（`snapshot.mode=no_data`），快照由 OO 的批量 worker 分块读取、直接 COPY 进 Agent DB**，不经过 Valkey、不写源库、按块断点续传、可并行。Debezium 增量快照保留为可选模式（`snapshot: debezium_incremental`）。

### 4.2 算法

前提：该表的 CDC 流已从位点 `L0` 开始被 Loader 消费，且 `L0` 早于任何快照块的读取开始。

```text
1. 新建目标表 T_new（不授权给任何 oo_r_*），以及墓碑表 TB（pk, pos）
2. Loader 对 T_new 应用 CDC 事件（照常：upsert / delete，“新位点覆盖旧位点”）；
   删除事件额外写入 TB
3. 快照 worker 按主键分块读取源表（从库、限速），每块作为 op=fill 的批次写入：
     INSERT INTO T_new SELECT … FROM chunk c
     WHERE NOT EXISTS (SELECT 1 FROM TB WHERE TB.pk = c.pk)
     ON CONFLICT (pk) DO NOTHING                 -- 已被 CDC 写过的行不覆盖
   快照块写入的行 _oo_pos = 0（低于任何 CDC 位点，之后的任何 CDC 事件都能覆盖它）
   对 CDC 写入时缺少不可用列（TOAST）的行，用快照块中的值补齐这些列
4. 所有块完成后，记录源库当前位点 L_end；等待 Loader 已应用位点 ≥ L_end
5. 建索引、ANALYZE、切换为 live（§6），删除 TB；
   授权只针对已完成分类、并已随 Ontology 发布的列（I-PL-8）——新表在首次发布前对 Agent 不可见
```

**正确性论证（每个主键的所有情况）：**

- 自 `L0` 起没有任何变更：没有 CDC 事件，快照块插入该行的值——这就是当前值。
- `L0` 之后被更新：若 CDC 先写入，快照块被跳过，保留 CDC 的值；若快照块先写入（值可能旧于或等于最新值），之后按顺序到达的 CDC 事件覆盖它。两种顺序最终都等于最后一个事件的值。
- `L0` 之后被删除：若删除事件先应用，墓碑阻止快照块“复活”该行；若快照块先写入，删除事件随后删除它。
- `L0` 之后插入又删除：同上。

由于 CDC 自 `L0` 起完整、按提交顺序应用，最终状态等于源库状态；快照块只负责填补“自 `L0` 起从未变化”的行。该算法不需要比较快照块与 CDC 事件的位点，因此对 PG/MySQL 的位点语义差异不敏感。快照期间可能出现短暂的旧值，但此时表对 Agent 不可见，第 4 步保证可见时已追平。

### 4.3 块的进度与续传

控制库 `sync.chunks(table_id, chunk_no, key_lo, key_hi, state, rows, started_at, finished_at, attempt)`。worker 崩溃后，未完成的块重新执行；由于插入是 `DO NOTHING` 语义，重复执行同一块无副作用。

## 5. 流式应用（CDC）

### 5.1 源位点 `_oo_pos`

| 源 | 编码 | 注意 |
|---|---|---|
| MySQL | `epoch << 64 \| (binlog 文件序号 << 32 \| 文件内偏移)` | binlog 按提交顺序写入，位点即提交顺序。主从切换后文件序列不可比，`epoch` 加一（Loader 在检测到 server_uuid / 文件序列跳变时递增） |
| PostgreSQL | `epoch << 64 \| 事务提交 LSN` | **必须用事务的提交 LSN，而不是单条变更记录的 LSN**：长事务内较早的变更记录 LSN 可能小于其他已提交事务的提交 LSN。需要在原型中确认从 Debezium 事件（事务元数据 / `source.sequence`）中取得提交 LSN 的方式，取不到则由 Loader 通过事务边界事件关联 |
| 批量增量 | `updated_at` 微秒时间戳 | 同一时间戳内的多次修改无法区分，依赖“整行覆盖 + 每日对账” |

### 5.2 应用模式与跨表一致性

业务上一个源事务常常同时改多张表（例如一次开票同时写订单、应收、明细）。如果每张表各自独立提交，Agent 做 JOIN 时可能看到“订单已更新、应收还没到”的半个事务，算出源库里从未存在过的数字。因此 CDC 源按源配置一致性模式：

| 模式 | 缓冲布局 | 写入者 | 保证 | 适用 |
|---|---|---|---|---|
| `transactional`（默认） | 该源所有表 + 事务元数据路由到**同一个 stream**（`oo:<source>`），提交顺序须通过 M0 验证 | 每源库一个 Loader 分片 | 普通源事务原子可见；无分段/暂停等降级且 cross_table_consistent=true 时为全源一致切面 | 绝大多数业务库 |
| `per_table` | 每表一个 stream | 每表一个分片，可并行 | 每张表各自最终一致；跨表**无**一致性保证 | 变更量极大、单消费者吞吐不够，且业务上跨表 JOIN 少或可接受短暂不一致的源 |

**`transactional` 模式的实现：**

1. Debezium 开启 `provide.transaction.metadata=true`：每个数据事件带事务 ID 与事务内序号，另外产生事务 BEGIN/END 事件（END 携带该事务的事件总数）。
2. 拟用 topic 路由 SMT（Kafka Connect `RegexRouter` 或 Debezium `ByLogicalTableRouter`）把该源的全部数据 topic 与事务元数据 topic 路由到同一个 stream。SMT 在 Debezium Server + Redis sink 下是否可用、BEGIN/END 是否一并路由、过滤表后的事件计数以及重放后的顺序均须在 M0 P11 验证；不能仅因共用 stream 就断言源提交顺序已得到保证。验证未通过时不得启用或宣称 transactional 保证，须修订路由方案或显式降级。
3. Loader 按事务 ID 缓冲事件，收到 END 且事件数对齐后，该事务才“完整”；**普通事务只在源事务边界提交 PG 事务**。多个完整事务可以合并进同一个 PG 事务（组提交，≤ 2 秒或 ≤ 1 万事件）；超大事务按第 5 步显式标记分段期间的一致性降级。
4. 同一个 PG 事务内更新 `_oo.source_watermark`（§5.7）与涉及表的 `_oo.table_freshness`。
5. **超大事务**（事件数 > `max_txn_events`，默认 20 万，常见于月结、批量导入）：事件先溢出到本地磁盘缓冲，然后按主键分段写入；首段提交时一并设置 `_oo.source_watermark.state = 'applying_large_txn'`、`cross_table_consistent = false`，水位保留最后完整事务的位置，涉及的表在 `_oo.table_quality` 中标记 `partial_txn_in_progress`。分段期间不应用后续源事务，Gateway 对触及这些表的查询附加质量警告（[07 §5.4](07-gateway.md)）；全部分段成功后在最后一段的事务里推进水位、清除标记，无暂停表等其他降级时才恢复一致性标记。中途失败则从该源事务开头重放（幂等），完成前不能提前清除标记。

**批量路径的一致性：** 设置为 `transactional` 的批量源，每次增量在源库从库上用一个一致性快照事务（MySQL `START TRANSACTION WITH CONSISTENT SNAPSHOT` / PG `REPEATABLE READ`）读取本轮全部表的增量，并在 Agent DB 的一个事务里应用，得到跨表一致的切面；快照事务时长受 `max_snapshot_txn_seconds`（默认 300 秒）约束，超出时本轮降级为逐表读取，并在质量标记中说明。

### 5.3 Loader 的一个批次

```text
XREADGROUP（该源或该表的 stream，消费组 oo-loader，单一活跃消费者）
  → 攒批：≤ 2 秒 或 ≤ 1 万条；transactional 模式只在源事务边界截断
  → 解码 Debezium JSON（按 op 字段识别删除；Redis sink 没有 tombstone，null 以占位串表示）
  → 转为统一批次（类型映射与批量路径一致；转换失败写拒绝日志，§5.6）
  → on_batch hooks
  → 按表分组，表内按主键折叠：同一主键只保留位点最大的事件
  → BEGIN
      对每张涉及的表 t：
        COPY 该表的事件 → 临时表 s（TEMP，不写 WAL，也无需复制到从库）
        DELETE FROM t USING s WHERE s.op = 2 AND t.pk = s.pk AND t._oo_pos < s.pos
        INSERT INTO t SELECT … FROM s WHERE s.op = 1
          ON CONFLICT (pk) DO UPDATE SET
            col_i = CASE WHEN i = ANY(EXCLUDED._unavail) THEN t.col_i ELSE EXCLUDED.col_i END, …
          WHERE t._oo_pos < EXCLUDED._oo_pos           -- 旧事件不覆盖新数据
      INSERT 本批拒绝记录 → sync.rejects（控制库，见 §5.6；Agent DB 内只更新汇总）
      UPDATE _oo.table_freshness / _oo.table_quality / _oo.source_watermark
    COMMIT
  → XACK；推进可裁剪位置，XTRIM MINID 裁剪已确认的条目
```

- 用 `INSERT ... ON CONFLICT` 而不是 `MERGE`：前者在并发插入下有更强的保证；单一写入者的前提下两者等价，选更稳妥的一个。
- **Valkey 中的条目只在 Agent DB 提交成功后才确认与裁剪**；绝不使用 `XADD MAXLEN` 自动裁剪（会丢数据）。
- 崩溃恢复：重启后先用 `XAUTOCLAIM` 收回挂起条目，重新应用——幂等；未完成的源事务缓冲直接丢弃，从 stream 中该事务的起点重放。
- 分片：一个 Loader 进程负责一组源（或一组表）；每个分片一个租约（控制库行锁 + 心跳），保证单一写入者；水平扩展即增加进程并重新分配租约。`transactional` 模式下单源吞吐上限就是单个分片的吞吐（目标见 §12），不够时该源改为 `per_table`，由管理员显式确认一致性降级。
- 拒绝记录写在控制库而不是 Agent DB：它们是运维数据，不需要被 Agent 查询；Agent DB 中只维护按表、按列的汇总（§5.6）。控制库写入与 Agent DB 提交不是同一事务：先写拒绝记录（幂等键 = 源位点 + 表 + 主键 + 列），再提交 Agent DB，重放时重复写入被去重。

### 5.4 PG 的 TOAST 未变化列

PG 默认 `REPLICA IDENTITY` 下，UPDATE 事件不携带未变化的 TOAST 列，Debezium 以占位值（`__debezium_unavailable_value`）表示。Loader 把这些列记入 `_oo_unavailable`，合并时保留原值（见 §5.3 的 `CASE`）。若该行此前不存在（快照尚未到达），先插入缺列的行并标记，由快照块补齐（§4.2 第 3 步）。DBA 也可以对这些表设置 `REPLICA IDENTITY FULL` 从源头解决，但会增加源库 WAL 量，由 DBA 决定。

### 5.5 表结构变化

| 变化 | 检测 | 处理 |
|---|---|---|
| 新增列（可空或带默认值） | Debezium schema change 事件；或事件中出现未知字段 | 自动 `ALTER TABLE ADD COLUMN`（PG 中为即时操作，不带 NOT NULL 约束），继续同步数据；**不授予任何 `oo_r_*` 角色**；登记到控制库 `sync.unmapped_columns`（表、列、源类型、首次出现位点、画像摘要）；触发 Ontology 增量构建，该列进入“待分类”审核项（[06 §4.10](06-ontology-layer.md)）。分类完成（至少确定 `pii` 与 `semantic_type`）并随 Ontology 发布后，`oo apply` 才按角色授权 |
| 源库配置了列排除规则但新列命中排除 | 同上 | 不加列、不同步该列的值 |
| 删列 / 改类型 / 改主键 | 同上 | **暂停该表**：Loader 继续消费（事务模式下，同一 stream 中其他表照常应用），把该表的原始事件写入对象存储的隔离区（Parquet），避免 stream 堆积拖垮 Valkey；该表对 Agent 返回 `TABLE_PAUSED`；告警；管理员确认新映射后，选择“回放隔离区”或“重做快照”。暂停期间涉及该表的源事务只被部分应用，`_oo.source_watermark.state` 标为 `paused_tables`、cross_table_consistent=false，完整水位停止推进；查询其他表仍可执行但携带 CROSS_TABLE_SKEW，跨到暂停表的查询被拒绝，恢复并追平后才恢复一致性标记 |
| 新建表（匹配 include 规则） | 定期 `list_tables` 对比 | 生成 plan，`oo apply` 后启动快照 |
| 删表 | 同上 | 暂停并告警；不自动删除副本 |

### 5.6 类型转换失败与拒绝日志

陈旧库里的脏值是常态：零日期、非法日期、字符集不一致的字节、NUL 字节、`enum` 的空错误值、非法 JSON、夏令时切换时不存在的本地时间。原则是**显式转换、不静默改值、不丢行**：

- 连接器对这类列读取原始值（字符串/字节），在 Python 中按 `type_policy` 的显式规则转换；**不依赖**源库会话的宽松 `sql_mode` 或驱动的隐式转换（它们会把脏值悄悄变成 0 或当前时间）。
- 转换失败时，该值写为 `NULL`（或 `type_policy` 指定的替换值），行照常写入；同时产生一条拒绝记录。
- 文件接入的类型推断同样适用：推断为日期的列里解析失败的单元格按此处理（[03 §7.2](03-sources-and-ingestion.md)）。

拒绝记录存在控制库 `sync.rejects`：

| 字段 | 说明 |
|---|---|
| `source` / `table` / `pk` / `column` | 定位；同一 (表, 主键, 列) 只保留一条，重复出现时累加 `count`、更新 `last_seen_at` |
| `reason` | `zero_date`、`invalid_date`、`charset`、`nul_byte`、`invalid_enum`、`invalid_json`、`nonexistent_local_time`、`out_of_range`、`parse_error` |
| `raw_value` | 截断到 200 字符；该列 `pii != none` 或尚未分类时只存掩码与长度 |
| `pos` / `first_seen_at` / `last_seen_at` / `count` | — |
| `resolved_at` | 源库后来修正了该值（下一次事件转换成功）时写入 |

汇总进 Agent DB 的 `_oo.table_quality`（按表、按列的未解决拒绝数，§5.7），并驱动两件事：Ontology 构建时自动为受影响的列生成 `caveats`（例如“`fhrq` 有 312 行零日期，已置为空”）；Gateway 对使用了这些列的查询附加质量警告（[07 §5.4](07-gateway.md)）。

### 5.7 源水位与质量标记

以下两张表在 Agent DB 的 `_oo` schema 中（定义见 [05 §6](05-agent-db.md)），由管线在写数据的**同一个事务**里更新，因此从库上读到的水位与质量状态精确描述该从库上的数据：

- **`_oo.source_watermark`**：每个源一行——一致性模式、`applied_pos`、对应的源提交时间、状态（`live` / `lagging` / `applying_large_txn` / `paused_tables`）。只有 transactional 且 `cross_table_consistent = true` 时，当前源表才在 `applied_pos` 处构成一致切面；超大事务分段或表暂停期间标为 false，保留最后完整水位，不代表当前全部可见数据。`per_table` 模式下这一行只给出各表位点的最小值，并标注 `cross_table_consistent = false`。
- **`_oo.table_quality`**：每张表一行——未解决的拒绝数（按列）、最近一次对账结果、未释义码值（[06 §3.6](06-ontology-layer.md)）、未分类的隐藏列数、是否有超大事务分段进行中、实体解析不变量是否通过（[06 §3.10](06-ontology-layer.md)）。写入者分别是 Loader、对账任务、码值巡检任务（每小时在从库上统计各码表列中不在已发布码表内的值）与实体解析任务。

### 5.8 缓冲的持久性与背压

- **背压链：** Loader 变慢 → stream 增长 → Valkey 内存达到 Debezium `memory.threshold.percentage`（默认 85%）→ Debezium 停止读取日志（不丢数据）→ 源库日志积压（监控告警）。
- **丢失窗口：** AOF `everysec` 在崩溃时可能丢失约 1 秒已确认写入的条目，而 Debezium 可能已提交了越过这些条目的位点，造成**静默丢失**。因此缓冲实例默认 `appendfsync always`（本场景写入量下吞吐可接受，性能测试中确认），并且 Debezium 的位点存储与缓冲实例分离（存控制库），外加每日对账兜底。
- **容量：** `maxmemory` 按“最大允许积压时长 × 峰值变更速率 × 平均事件大小 × 2”估算，写入 [11 运维](11-deployment-and-operations.md)。

## 6. 快照完成与换表

首次快照、重做快照、文件 `replace` 模式、批量 `full` 模式都以“影子表 + 事务内换表”完成：

```sql
SET lock_timeout = '5s';          -- 拿不到锁就重试，不长时间阻塞
BEGIN;
  ALTER TABLE erp_mysql.ydh RENAME TO ydh__old_20261001;              -- 若存在
  ALTER TABLE erp_mysql__stage.ydh__new SET SCHEMA erp_mysql;
  ALTER TABLE erp_mysql.ydh__new RENAME TO ydh;
  -- 重新应用 roles.yaml 编译出的 GRANT 与 RLS 策略（策略是按表对象存在的）
  -- 重建依赖它的场景视图（定义来自 Git；视图按 OID 绑定，换表后会继续指向旧表）
  -- 更新 _oo.table_freshness（state = live）
COMMIT;
-- 旧表延迟删除（默认 24 小时），便于回滚
```

两个容易忽视的点：

1. **依赖视图会跟着被改名的旧表走。** PG 视图按对象 OID 绑定，`RENAME` 后视图仍指向旧表。所以换表事务里必须重建依赖视图，或者场景视图全部由 `after_load` hook 在换表后重建。`oo validate` 检查每个场景视图都有 Git 中的定义。
2. **换表会在从库上回放 `ACCESS EXCLUSIVE` 锁**，与正在执行的 Agent 查询冲突，这些查询会在 `max_standby_streaming_delay` 后被取消。换表尽量安排在低峰，并由 Gateway 对这类取消返回可重试错误。

## 7. 批量路径

- **首次全量：** 与 §4 的分块读取相同，但没有 CDC 流，所以直接 COPY 进影子表，完成后换表。
- **增量：** 每表在控制库记水位 `W`（上次成功同步到的 `updated_at`）。读取 `updated_at > W - overlap` 的行（默认重叠 5 分钟，兜住晚提交的长事务），以 upsert 写入；**全部写入成功后才推进水位**，与任务完成标记在同一个控制库事务中。
- **前提：** 源表要有“每次修改都会更新”的 `updated_at` 列（画像可以检验：抽样对比）。没有的表只能 `full`，或请客户加列。
- **删除：** 批量路径看不到删除，依赖每日对账（§8）删除源库已不存在的行；源表用软删除时不需要。
- **限速：** 每个源库的并发连接数与每秒行数上限（令牌桶），优先读从库；执行窗口可配置（例如只在 20:00–08:00 做全量）。

## 8. 对账

对账工具是所有同步测试的判定依据，也是生产上每日对账的同一个工具。

### 8.1 算法（哈希分桶）

1. 按主键范围把表切成桶（默认每桶 10 万行）。
2. 两侧分别读取桶内的行（源库侧通过连接器 `read_chunk`，与同步使用同样的类型映射，统一成 Arrow），逐行计算规范化哈希，桶内聚合为 `(count, xor/sum of hashes)`。
3. 桶摘要不同的，再二分细化到小范围，最后逐行比对，输出差异行（缺失、多余、值不同）。
4. 跳过“最近一个同步周期内变化过的行”（Agent DB 侧 `_oo_pos` 或 `_oo_synced_at` 较新的行），避免把正常延迟当成差异。

哈希在 Arrow 层计算而不是在两侧数据库里用 SQL 计算，是为了绕开不同数据库对同一值的文本表示差异（浮点、时间精度、字符集）。代价是需要把行读出来，所以生产上的每日对账默认只做：全部桶的行数比对 + 抽样桶（默认 2%）的全行哈希；每周一次全量（低峰、限速）。

对账结果写入 `_oo.table_quality`（`reconcile_status`、`reconcile_at`、差异行数）。状态为 `mismatch` 的表在修复完成之前，Gateway 对触及它的查询附加质量警告，Agent 必须在回答中说明（[09 §7](09-sandbox-and-agent-runtime.md)）；超过 `reconcile_max_age`（默认 3 天）未对账的表标为 `stale`。已由拒绝日志解释的差异（值被置空的列）不计为差异。

### 8.2 差异修复

| 差异规模 | 修复方式 |
|---|---|
| 少量行 | **块修复：** 记录源位点 `P1` → 重新读取该块 → 等待 Loader 已应用位点 ≥ 读取后的位点 `P2` → 对块内每行：若 Agent DB 中该行 `_oo_pos ≤ P1`（`P1` 之后未被 CDC 改写），用读到的值覆盖；源库中不存在且 `_oo_pos ≤ P1` 的行删除；`_oo_pos > P1` 的行已被 CDC 更新过，不动 |
| 大量或结构性差异 | 重做快照：按 §4 写新表，期间 CDC 同时写旧表与新表，完成后换表 |

块修复的正确性依赖 `_oo_pos` 是提交顺序位点（§5.1）；若某个源的位点语义未被验证，只使用重做快照。

## 9. Hook

生命周期：`before_run → before_table → on_batch → after_load → after_run`，失败触发 `on_error`。

| hook | 输入 | 能做什么 | 不能做什么 |
|---|---|---|---|
| `on_batch` | Arrow 批次 | 纯函数变换：脱敏、清洗、派生列、过滤行 | 访问数据库或网络（在 worker 进程内以受限方式调用，超时 5s） |
| `after_load` | 已更新的表集合 | 在 Agent DB 中以 `oo_scene_builder` 角色执行 SQL：建场景表/视图 | 写源表副本；访问 `_oo`；创建函数（除非在白名单中） |
| `on_error` | 错误上下文 | 通知 | — |

- 场景构建的三种方式：视图（`security_invoker`，实时）；物化表（影子表 + 换表，定时或防抖触发）；增量（hook 自己维护，高级用法）。
- `after_load` 默认防抖：同一场景 schema 最多每 10 分钟重建一次。
- hook 代码与 SQL 位于配置仓库，经过代码审核才能生效；`oo validate` 用 pglast 检查 `after_load` SQL 只包含允许的 DDL/DML 且目标都在场景 schema 中。

内置 hook：`mask`（按列规则掩码：手机号、证件号、邮箱）、`hash`（加盐哈希，保留可 JOIN 性）、`drop_columns`、`normalize_text`（全角半角、首尾空白）。实体合并与别名不再用 hook 模板实现，而是由实体解析任务按 Ontology 中的规则与管家决定物化（[06 §3.10](06-ontology-layer.md)）。

## 10. 编排与状态

- **离散任务**（Procrastinate，存于控制库）：快照块读取、批量增量、每日对账、块修复、重做快照、文件解析、after_load、码值巡检、实体解析、Ontology 构建触发。同一目标的任务用 queueing lock 合并；上一次未完成时跳过或排队；高频事件先防抖。
- **常驻服务**：流式 Loader（按源或按表租约）、Debezium Server（每源库一个容器，由 `oo apply` 通过 Docker API / K8s 管理）。
- **表的状态机：**

```text
registered → snapshotting → catching_up → awaiting_publish → live ⇄ paused → resnapshotting → live
                   └──────────────┴────────────┴────────────────┴──────▶ error（需人工处理）
```

`awaiting_publish`：数据已同步追平，但列尚未全部分类或 Ontology 尚未发布，对 Agent 不可见。控制台在这里提示“还差 N 列待分类”。

- **控制库主要表：** `sources`、`tables`（状态、模式、配置指纹）、`sync.runs`、`sync.chunks`、`sync.watermarks`、`sync.freshness`（`_oo.table_freshness` 的镜像）、`sync.schema_changes`、`sync.unmapped_columns`、`sync.rejects`、`sync.reconcile_results`、`sync.quarantine`、`procrastinate_*`。

## 11. 保护源库与监控

| 指标 | 告警阈值（初始） | 为什么 |
|---|---|---|
| CDC 端到端延迟（`_oo.heartbeat`） | > 5 分钟 | 新鲜度 |
| PG 复制槽保留的 WAL（`pg_replication_slots` + `safe_wal_size`） | > 磁盘余量 30% | 撑满源库磁盘是最严重的事故 |
| MySQL：Debezium 位点距 binlog 最早保留文件的距离 | < 24 小时的保留余量 | binlog 被清理即需重做快照 |
| Valkey 内存 | > 70% | 在 Debezium 暂停（85%）之前预警 |
| 每个 stream 的长度与最老未确认条目的年龄 | > 10 分钟 | Loader 卡住 |
| 源库读取速率 / 连接数 | 超过配置上限 | 限速失效 |
| 源库从库复制延迟 | > 5 分钟 | 读从库时数据本身就旧 |
| 对账差异行 | > 0（持续两天） | 一致性 |
| 隔离区事件数 | > 0 | 有表处于暂停 |
| 新增未解决拒绝记录 | 单表单日 > 1,000 或 > 0.1% 行 | 源数据质量恶化或类型映射有误 |
| 待分类的新列 | 存在超过 3 天 | 新列长期不可见，业务可能在等 |
| 超大事务分段持续时间 | > 10 分钟 | 期间相关表带质量警告 |
| `per_table` 模式源的跨表位点差 | > 60 秒 | 跨表不一致窗口过大 |

**故障处理：**

| 故障 | 影响 | 恢复 |
|---|---|---|
| Loader 崩溃 | 延迟上升 | 重启后 `XAUTOCLAIM` 重放，幂等 |
| Debezium 崩溃 | 延迟上升 | 从控制库中的位点继续 |
| Valkey 重启 | 至多丢失未 fsync 的条目（`always` 下为 0） | 每日对账兜底 |
| Agent DB 主库不可用 | Loader 重试、stream 积压，最终 Debezium 暂停 | 主库恢复后自动追平 |
| MySQL binlog 被清理 / PG 复制槽失效 | 流中断，无法续读 | 告警，对受影响的表自动重做快照（需管理员确认，因为会带来源库读取压力） |
| 源库主从切换 | MySQL（GTID）：Debezium 继续，`_oo_pos` 进入新 epoch；PG：复制槽不随切换迁移（除非 PG17 failover slot 且托管库支持） | PG 需重建复制槽并重做快照 |

## 12. 性能目标（初始，待压测确认）

| 项 | 目标 |
|---|---|
| 首次快照吞吐（单表，受源库限速约束） | ≥ 5 万行/秒 |
| 5,000 万行表首次快照 | ≤ 30 分钟（不含索引构建）/ ≤ 60 分钟（含） |
| CDC 应用吞吐（单分片；`transactional` 模式下即单个源库） | ≥ 5,000 事件/秒 |
| CDC 端到端延迟 p95 | ≤ 60 秒 |
| 每日对账对源库的额外读取 | ≤ 源表体积的 5%（默认抽样配置） |
