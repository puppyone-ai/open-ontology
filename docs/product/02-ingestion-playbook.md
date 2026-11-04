# 02 数据拉取手册：分块、并行、一致性与故障恢复

> Status: Accepted（规则来自参考生产部署的实际运行；跨引擎推广部分为 Proposed）
> Owner: Open Ontology Engineering
> Last reviewed: 2026-11-04
> 依赖：[03 数据源](../03-sources-and-ingestion.md)、[04 管线](../04-pipeline.md)、[database-support/03](../database-support/03-change-capture-and-consistency.md)

参考生产部署是一套 MySQL 5.7 源（约 670 张表、1.6 万列、百万行级大表若干），每 8 小时一个周期，经 SSH 转发读取，持续发布到 Gateway。下面的规则都在那里出过问题并修过。引擎无关的部分适用于所有源；引擎相关的部分见 §7。

## 1. 先认清源库形状

实际表结构往往不是“整数自增主键”。参考部署中：整数主键只占少数，复合主键表数十张，无主键表十余张，**所有百万行以上的大表都是 `varchar` 主键**，排序规则大小写不敏感。所以：

- 预检必须统计每张表的键形态：整数单列 / 字符串单列 / 复合 / 仅唯一索引 / 无键，并据此选分块策略。
- 预检输出“分块计划”给用户看：每表策略、估计块数、无法分块的表及原因。

## 2. 分块策略

| 键形态 | 策略 | 要点 |
|---|---|---|
| 整数单列主键 | keyset 范围 `WHERE pk > ? ORDER BY pk LIMIT n` | 不用 OFFSET |
| 字符串单列主键 | keyset，边界比较在**源库**用列自身排序规则完成 | 客户端不比较字符串（避免排序规则不一致导致漏读/重读）；不加 `COLLATE xxx_bin`，否则走不了索引 |
| 复合主键 | 行值比较改写为 `a > ? OR (a = ? AND b > ?)` | 用执行计划确认走主键（Oracle 不用会写 `PLAN_TABLE` 的 `EXPLAIN PLAN`，改查本会话游标的 `V$SQL_PLAN`）；不信任各版本对行值比较的优化 |
| 无主键但有非空唯一索引 | 用该索引做 keyset | 预检时选定并记录 |
| 无任何唯一键 | 单次整表读取，或引擎原生物理分块（PG ctid 范围、Oracle ROWID 范围） | 写入计划并标注一致性等级；不能用 OFFSET 假装可续 |

**估算块数与切边界**：先算分布因子 `(MAX(pk)-MIN(pk)+1)/行数`；落在 [0.05, 100] 内视为均匀，按步长切；否则逐块 keyset 探边界；预计块数超过 1000 时按 1/1000 抽样切分（Flink CDC / SeaTunnel 的做法）。行数用 `EXPLAIN` 或 MIN/MAX 估，不用 `information_schema.TABLES.TABLE_ROWS`（MySQL 8.0 默认缓存 24 小时）。用非主键列做 chunk key 时，更新可能让同一行跨块出现两次，只能配合日志窗口修正或整表重读。

**物理分块的限制**：PG 14 起有 TID Range Scan，`ctid` 范围切块很快，但 `ctid` 会因 UPDATE / `VACUUM FULL` 改变，只能在单个快照内使用，不能作为跨周期断点。Oracle 的 ROWID 范围不在源端建 `DBMS_PARALLEL_EXECUTE` 任务（只读账号不允许写），改为读 `DBA_EXTENTS`/`ALL_EXTENTS` 后用 `DBMS_ROWID.ROWID_CREATE` 生成。

**Oracle 执行计划检查**：`EXPLAIN PLAN` 会写入 `PLAN_TABLE`，零写连接器改为检查实际只读游标的 `V$SQL_PLAN`（通过 `V$SESSION` 定位）；缺少视图权限时明确报告未验证，不视为索引检查通过。

**块大小自适应**（`crates/oo-connectors/src/mysql/chunk.rs` `AdaptiveRows`）：硬上限 100,000 行 / 32 MiB；按 EWMA 估算读取速率，下一块取“约 3 倍当前速率 × 目标耗时”并受字节上限约束；失败时折半。这些是**内部资源预算**，不是业务上限。

**增长中的块**：周期间某个范围长到超出单块预算时，不截断、不退回全量，而是拆分并生成新 revision 后完整读取（参考部署在 100,001 行上实测拆为 `[100000, 1]`，零差异）。

**断点**：每块落盘 `(table, chunk_id, lo, hi, rows, bytes, sha256)`。块内中断整块重读。大块的成员清单写成文件并以 sha256 引用，不塞进控制库单字段。

## 3. 并行与源库保护

- **调度**：有界滚动补位，不按组等待（旧做法每 3 个源一组、整组完成才放下一组，最慢的拖住全部槽位）。默认并发 2，可配 1–4。同一 `(source, target_database)` 内串行。
- **失败即停派**：一旦出现失败，停止派发新任务，等在途任务结束并写回执，再按断点重试。
- **源库压力保护**（默认开启，不为提速关闭）：
  - 每个读任务单连接；
  - 块目标耗时约 0.5 秒；占空比 ≤ 25%（读完休眠 ≥ 读耗时 × 3）；
  - 源端负载指标（MySQL `Threads_running`、PG 活跃会话、Oracle 活跃会话/等待）超阈值则暂停；
  - 服务端语句超时（MySQL `max_execution_time`、PG `statement_timeout`、Oracle 资源管理或客户端取消）加客户端超时，双保险；
  - 不 `LOCK TABLES`、不 `FLUSH`、不建对象；
  - 按 `max_connections` 余量动态限并发；流式读取时调大会话级 `net_read_timeout`/`net_write_timeout`，避免客户端消费慢被服务端断开；
  - 从副本读时，水位必须取自同一实例，复制延迟超阈值暂停。
- **长事务风险**：一致性快照持续越久，源端越受影响（MySQL undo 历史、PG 旧快照钉住 xmin 阻止 vacuum、Oracle `ORA-01555`）。全量导入按块使用短事务；需要跨表一致时用水位对齐（§5.1），而不是开一个持续数小时的事务。
- **不加全局锁**：MySQL Shell 式一致性快照需要 FTWRL（5.7）或 `LOCK INSTANCE FOR BACKUP`（8.0+，需 BACKUP_ADMIN）。生产账号通常只有 SELECT，FTWRL 还会在长查询期间挂住整个实例。默认走无锁分块，FTWRL 只作为用户显式选择的选项。

## 4. 暂存、装载与目标写放大

1. 每个范围先写私有暂存；完整范围就绪后原子替换；然后发布。暂存后、目标提交后各做过一次真实 SIGKILL 恢复，同一任务 lease 递增后恢复，没有重复应用。
2. 长事务内分段续租（COPY 每 512 行一段，租约 30 秒不变），避免大批量装载期间租约过期被他人接手。
3. 避免目标写放大：变化块不能复制整张父表。采用“不可变父存储 + 变化块组合”的代际模型，并计量每周期目标写入字节。

## 5. 增量与一致性

| 方式 | 适用 | 规则 |
|---|---|---|
| C1 日志 CDC | 源开启 binlog（ROW/FULL）/ WAL 逻辑复制 / Oracle 归档与补充日志 | 首选；位点 durable，ACK 不早于确认 |
| C5 范围摘要差分 | 拿不到日志时 | 分块 `COUNT` + 按列摘要（含 NULL 标志）；只和上一轮源摘要比较；不叫 CDC |

### 5.1 只读水位对齐（首次全量 + 日志）

DBLog / Debezium 增量快照的思路：按块读取，每块前后各记一个低/高水位，窗口内日志出现过的 key 以日志为准，覆盖快照缓冲中的同 key 行。标准实现要往源端 signal 表写水位，我们只读，所以水位取自引擎只读可见的位点：

| 引擎 | 水位 |
|---|---|
| MySQL | 已执行 GTID 集合；未开 GTID 时用 binlog file/pos（从多线程复制的副本读需 `replica_preserve_commit_order=ON`） |
| PostgreSQL | `pg_current_wal_lsn()`（副本上用 `pg_last_wal_replay_lsn()`） |
| Oracle | `CURRENT_SCN` |

每块记录 `(low, high, 范围, 行哈希)`，之后用日志回放窗口内的变更修正该块。MySQL 部分有 Debezium `read.only=true` 的先例；PG、Oracle 部分是推导出的设计，须在 S2 原型中实证后才进矩阵。

**首次全量期间就要开始读日志**：Airbyte MySQL 的已知缺陷是初始同步期间不读 binlog，大表同步时间超过日志保留期后只能重来。我们在首次全量开始前记录 L0 并持续消费日志到暂存区。

Oracle C1 重启必须携带首次对齐基线前采集的事务低水位（不晚于当时最早活跃事务的起始 SCN），与 DBID/resetlogs 和已提交 SCN 一起持久化。重放从固定事务低水位开始，不能随提交位点推进它；缺少覆盖该低水位的 redo 时显式重基线。空 SCN 窗口不创建目标数据代际。当前 `oo-load` 实现不含持久化在途事务暂存，因此必须在这段历史被清理前重基线。

### 5.2 C5 摘要差分

C5 的四层：时间列只作正向提示（NULL 视为未知）→ 近窗口扫描加速 → 分块摘要比较 → 低频全读兜底。`update_time`、`MAX(id)`、业务日期都不能当可靠游标。无变化周期的标准是零载荷、零目标改写、零新代际；`COUNT` 相等不代表没变。

### 5.3 交接与保留期

**C5 → C1 交接屏障**：先订阅日志拿到 L0 → 以 L0 为界修复基线 → 日志优先、墓碑防复活 → 到 L_end 屏障才切换。日志谱系变化（GTID 集合不连续、日志被清理）进入显式新代际重基线，不静默补洞。交接必须覆盖多来源、多绑定的整个范围，不能只支持单表。

**保留期预检**：C1 要求在线日志保留覆盖“最长周期间隔 + 最长故障恢复时间”。托管库（RDS 类）的保留参数可能与引擎参数不一致，预检读取实际可见的最早日志位点，而不是信参数值。

## 6. 故障与恢复

| 现象 | 处理 |
|---|---|
| 容器或 Docker 引擎退出 | 执行容器 `restart: unless-stopped`；恢复后同周期断点重试，复用已成功项 |
| Docker socket 短暂拒绝 | 同周期重试，不重置位点、不延长截止 |
| 部分表客户端/服务端超时 | 断点重试；不放宽源库超时；连续失败的表单独报告 |
| SSH 握手超时 / 断线 | 按 socket 重连，块级续读；不改全局网络 |
| 失败日志挡住同任务重试 | 先原子归档失败日志，再沿原检查点重试 |
| 结构指纹误报漂移 | 不同算法版本的指纹不可直接比较；指纹带算法版本号 |
| 元数据查询慢 | 改为按库批量查询（参考部署 658 表 / 15,799 列 24.5 秒） |
| 周期预算 | 首次运行写入不可变预算文件；重试不能延长 deadline；进程停机时间也计入 |

调度器状态要区分：未到期、重试未到期、超出截止、子进程超过截止仍存活。**定时器存在不等于同步成功**，`oo status` 必须同时显示最近成功周期、最近失败原因、下次到期。

## 7. 引擎差异速查

| 引擎 | 分块 | 增量 | 需要注意 |
|---|---|---|---|
| MySQL 5.7 | keyset；排序规则在源端比较 | binlog ROW + FULL；GTID 可选 | binlog 默认关闭；无 `LOCK INSTANCE FOR BACKUP`；默认认证 `mysql_native_password`；`max_execution_time`（5.7.8+）仅对只读 SELECT 生效 |
| MySQL 8.0 | 同上 | 同上 | 默认认证 `caching_sha2_password`（需 TLS 或 RSA 公钥交换）；binlog 默认开启；`information_schema` 统计默认缓存 24 小时 |
| MySQL 8.4 LTS | 同上 | 同上 | `SHOW MASTER STATUS` 删除，改 `SHOW BINARY LOG STATUS`；`expire_logs_days` 删除；tagged GTID `UUID:TAG:N`；`mysql_native_password` 默认禁用 |
| MySQL 9.x | 同上 | 同上 | `mysql_native_password` 移除，只能 sha2 |
| PostgreSQL 12–18 | keyset；无键表在单快照内按 ctid 范围（14+） | 逻辑复制 slot + `pgoutput`；或 C5 | slot 未消费堆积 WAL；13+ `publish_via_partition_root`；proto v2 大事务流式（14+）、v3 两阶段（15+）、v4 并行流式（16+），按服务端版本协商；16 支持备库逻辑解码；17 failover slot；18 `idle_replication_slot_timeout` 会作废闲置 slot、VIRTUAL 生成列不可复制 |
| Oracle 11g | keyset；无键表按 ROWID 范围（读 extents 生成，不在源端建任务） | LogMiner（可用 CONTINUOUS_MINE）/ 归档日志；SCN | 非 CDB；无 identity 列；空串即 NULL |
| Oracle 12c–19c | 同上 | LogMiner，CDB 下以 `c##` common user 连接 CDB$ROOT；只用 `online_catalog` 字典策略：读取账号零 DML/DDL，但 LogMiner 会话由 Oracle 写其自有对象（23ai 为 `SYS.LOGMNR_GT_*`，11g/21c 为 `SYSTEM.LOGMNR_GT_*`、`SYSTEM.LOGMNR_SESSION$`；T7 实测），归类“R1，含 Oracle 内部递归临时写”，C1 需显式选用；默认 C5 须单独零写通过 T7（Plan D11/D11a）；需要 `DBMS_LOGMNR_D.BUILD` 的 `redo_log_catalog` 属 R1w，默认禁用 | 19c 起无 `CONTINUOUS_MINE`，须自己维护日志文件列表；字典策略 `online_catalog` / `redo_log_catalog` 二选一 |
| Oracle 21c / 23ai | 同上 | 同上 | 不再支持非 CDB，一律 PDB；23ai 新系统表空间（如 vector）需从 LogMiner 排除；`ORA-01555` 限制单块快照时长 |

所有引擎实现 `capabilities(version)` 探测，用版本分派表代替散落的 if 判断；`oo doctor` 据此输出缺失的权限与参数，并给出可直接复制给 DBA 的 GRANT 与参数语句。

具体版本矩阵与测试镜像见 [05](05-version-matrix-and-testing.md)。

## 8. 外部参考（2026-11-03 检索）

- DBLog 水位算法：https://arxiv.org/abs/2010.12597
- Debezium 增量快照：https://debezium.io/blog/2021/10/07/incremental-snapshots/ ；MySQL `read.only`：https://debezium.io/documentation/reference/stable/connectors/mysql.html
- Flink CDC 分块参数：https://nightlies.apache.org/flink/flink-cdc-docs-master/docs/connectors/flink-sources/mysql-cdc/
- SeaTunnel 分布因子与抽样切分：https://seatunnel.apache.org/docs/2.3.9/connector-v2/source/Oracle-CDC/
- Airbyte MySQL 初始同步与 binlog 保留问题：https://airbyte.com/blog/behind-the-performance-improvements-of-our-mysql-source
- MySQL Shell dumpInstance 一致性与分块：https://dev.mysql.com/doc/mysql-shell/8.4/en/mysql-shell-utilities-dump-instance-schema.html
- MySQL 8.4 变更：https://dev.mysql.com/doc/refman/8.4/en/mysql-nutshell.html
- PG TID Range Scan：https://www.pgmustard.com/docs/explain/tid-range-scan ；逻辑复制协议版本：https://blog.peerdb.io/exploring-versions-of-the-postgres-logical-replication-protocol
- Oracle 19c 废弃项：https://docs.oracle.com/en/database/oracle/oracle-database/19/upgrd/oracle-database-changes-deprecations-desupports.html ；Debezium Oracle：https://debezium.io/documentation/reference/stable/connectors/oracle.html
- Oracle ROWID 分块：https://oracle-base.com/articles/11g/dbms_parallel_execute_11gR2
