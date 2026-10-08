# 发现

会话 `eef831aa-dba4-4af2-a78b-541bc9e0a390`。源码提交 `1c4797e2f846da7744565c0f95103b68c1d01846`。

## 只读核心里不够用的地方

驱动调用已发布的 `jobs.enqueue`、`jobs.claim`、`jobs.heartbeat`、`jobs.complete_success`、`jobs.complete_failure`、`jobs.retry_failure`、`jobs.cancel`。没有改这些函数，也没有改 `jobs.tasks` 的行。

这些缺口留在只读核心里，适配放在 `tools/job-worker`：

- 七个函数内部没有 `NOTIFY`。驱动在入队事务里调用 `pg_notify('oo_jw_wake', project_id)`。通知丢失时任务仍是 `queued`，随后靠轮询领走。`notify_left_queued=1` 记录了这一点。
- 没有 schedule 函数。`jobs.schedules` 没有载荷列。0003 之后运行时角色仍保有 schedule 列与 `jobs.schedule_occurrences`、`jobs.leases` 的原授权。驱动把载荷放进 `jw.schedule_payloads`，同一次 tick 靠 occurrence 主键只插入一行。并发两次触发得到 1 次 occurrence、1 个任务、1 行效果。
- `jobs.enqueue` 在读已提交下抓到 23505 后再查行。胜者尚未提交时会抛 `OO016`。schedule 触发把这当作输掉的竞争，已有 occurrence 提交后不再另开一轮。本次通过轮没有把 `OO016` 打成测试失败。
- 没有检查点列，也没有副作用表。`jw.checkpoints` 与 `jw.effects` 承担这两件事。`stop_after` 只在 generation 1 生效，否则新 generation 会被同一份不可变载荷卡住。
- 租约长度固定为服务器 `clock_timestamp()` 加 30 秒，调用方不能传 TTL。心跳同样只延长这 30 秒。实测 `lease_span_seconds=29`，第一次心跳间隔 10 秒，该次心跳后的剩余租约 29 秒。
- `retry_failure` 没有抖动。本驱动也不加抖动。
- 终态上的再次完成返回 `terminal`。`already_applied` 不是这条 API 的成功状态。
- `target_fence_certified` 不能被设为 true。通过轮 `target_fence_true=0`。目标围栏仍属后续项。
- a079 的 `40P01` 锁循环仍然可能。驱动对 schedule 触发最多重试 3 次，不拿 advisory lock `6401202601`。
- 已审 demo 的 `pg-wrap.sh` 把 `max_connections` 设为 20。100 个会话装不下。本次启动保留其余服务器参数，只把 `max_connections` 改为 128。`shared_buffers` 维持 64MB。
- 迁移 0003 收回了运行时对 `jobs.tasks` 与 `jobs.task_events` 的写入。检查点和效果因此不能写成任务列。这是授权边界，驱动用 schema `jw` 避开它。

`adapt.sql` 的 SHA-256 是 `8ba28e94b8b861f640a1aa5dc8021ba6bbd44cb64a7c8a13a1fefae421331bcd`。四份迁移校验和与 `CONTRACT.md` 中的冻结值一致，`checksums_match=true`。

## 驱动上修过的缺陷

记录在 `failed-attempts.txt`，原始日志仍在对应阶段目录。

1. `linux/run`：Rust 2024 把 `gen` 保留为关键字；`json!` 超过默认递归深度。
2. `linux/run-attempt-1`：`transaction()` 需要 `&mut Client`；schedule 在事务存活时借用 client；`NOTIFY` 的参数类型不对。
3. `linux/run-attempt-2`：Clippy `manual_range_contains`。
4. `linux/run-attempt-3`：`$1::uuid` 要求协议类型 uuid，`&str` 是 text，客户端报 `error serializing parameter 0`。改为 `$N::text::uuid` 与 `$N::text::timestamptz`。没有加入 uuid crate。
5. `linux/run-attempt-4`：心跳等待 SQL 把 `min(observed_at)` 减到未聚合的 `claimed_at`，sqlstate 42803。错误显示当时只有 `db error`。查询改为两端都取 `min`，错误文本带上数据库 message 与 detail。prepare 当时已经打印 `due=100 xact=27`。

`linux/run-attempt-5` 在这些修复之后通过。

## 锁文件

`Cargo.lock` 从 `tools/session-store/Cargo.lock` 复制，来源摘要 `ad43022b2fd98a158781df1f7dbb928b857683e54871a4210f3360bfff525f9a`。删去 session-store 与 session-transition 两个包条目，加入 job-worker，并按 tokio 的 signal feature 补上 `errno 0.3.14` 与 `signal-hook-registry 1.4.8`。没有运行 `cargo update`，没有改根清单，没有生成新版本。`cargo metadata --locked --no-deps` 退出码 0。

## 副作用计数

`effect_rows=107` 且 `effect_txids=107`。`pg_stat_get_db_xact_commit` 从 prepare 到 report 的增量是 7741。空领取必须提交，所以库级提交数大于效果行数。效果行上的 `txid` 来自该行所在事务的 `pg_current_xact_id()`。过期 owner 的插入为 0 行，fence 探测的 seq 31 也是 0 行。

## 仍未覆盖

没有抖动。没有目标围栏认证。没有备份与恢复。A068 的 F3、F4、F5 不在本驱动。`40P01` 只做了有限重试，没有另造锁循环。`NOTIFY` 不在 SQL 函数里。schedule 没有载荷列。任务表没有检查点列。父项与 AT-CTL 验收不由本目录签署。
