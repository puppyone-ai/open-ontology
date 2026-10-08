# 发现

会话 `eef831aa-dba4-4af2-a78b-541bc9e0a396`。产品提交 `1c4797e2f846da7744565c0f95103b68c1d01846`。Verdict: `request_changes`。

## 原卡仍未完成的补偿

原计划 `specs/open-ontology-v0.1-plan.md` 第 265 到 267 行要求：短事务领取、30 秒租约、10 秒心跳、generation CAS、schedule 去重、数据库时钟、NOTIFY 只唤醒、轮询兜底、重复计划一次副作用、旧 generation 不能完成。`docs/17-rust-implementation.md` 第 3.1 节要求指数退避加 jitter，并用 `jobs.leases` 做同目标互斥。

驱动调用了已发布的七个函数，运行时不能 UPDATE `jobs.tasks`。下面几项留在核心里，驱动没有把它们补成原卡要求的行为。

### jitter

`jobs.retry_failure` 的正文没有 `random`，也没有 `pg_notify`。延迟文本是 attempt 1 等 1 秒，然后 2、4、8、16、32 秒，attempt 7 及以上等 60 秒，实现为 `v_delay := 60` 或 `1::bigint << (v_row.attempt - 1)`。

同一次探针里两个 attempt 1：

- `review-retry-a`：`retried`，`remain_s=0.999760`
- `review-retry-b`：`retried`，`remain_s=0.999857`

两次都贴着 1 秒。运行时没有 `jobs.tasks` 的 UPDATE，驱动不能在领取之后改 `run_at`。jitter 补不进现有函数。

### 副作用

`review-once` 成功后，新的工作行是 `jw.effects` 的 `(seq=1, generation=1, owner=w-000, txid=760)`。同一任务还有 `jw.claims` 一行和 `jw.checkpoints` 一行。`jobs.leases` 的插入数在 serve 前后都是 0。

这一行保存的是本事务的 `pg_current_xact_id()`。计划里的一次副作用指业务变化。作者归档把 107 行 `jw.effects` 与 107 个不同 txid 当作效果证明，同时写出 `xact_commit_delta=7741`。那个差额来自空领取、心跳和控制命令。本复查沿用这个读法：txid 计数证明插入发生过，不证明业务副作用只有一次。

检查点同样写在 `jw.checkpoints`。作者归档的杀死再拉起覆盖了两代和 4 行效果。那些效果仍是这张计数表里的单位，不是外部业务写入。

### NOTIFY

七个 `jobs` 函数的 `pg_notify` 位置都是 0。作者在入队事务里另调 `pg_notify('oo_jw_wake', project)`，并有一条 `notify_left_queued=1` 之后靠轮询成功的记录。核心函数自己不通知。

本探针的 `review-once` 在 serve 之前已经入队。`serve.stderr` 的顺序是 `serve`、`claimed`、`complete`、`serve_stop`。这里没有一条由通知叫醒的事件。

### schedule 载荷与 OO016

`jobs.schedules` 没有 payload 列。作者把载荷放在 `jw.schedule_payloads`，并用 occurrence 主键得到 1 次 occurrence、1 个任务、1 行效果。本探针没有再触发 schedule，`jw.schedule_payloads` 的插入数是 0。缺列这件事由目录查询直接看到。

一次重叠的相同身份入队：持有方提交前，等待方返回 `replayed`，表里 1 行。作者通过轮的日志没有 `OO016`。`jobs.enqueue` 在唯一约束冲突且胜者不可见时会抛 OO016，schedule 驱动把这个错误当成输掉的竞争。这条路径在两份证据里都没有被打出来。

### 已观察到的租约行为

这些行为有记录，不把整张卡签成通过。

- 作者归档：100 个会话同时 running，租约跨度 29 秒，心跳间隔 10 秒，旧 generation 完成插入 0 行，之后返回 terminal。
- 本探针：`jw.claims` 跨度 29 秒，generation 1 完成，再次 `complete_success` 返回 `terminal`。未来任务 `review-cancel` 被取消，效果行 0。
- `target_fence_true=0`。目标围栏仍在 SP04。

## 工具链

改前 smoke 与通过轮都是 Linux arm64 上的 Rust 1.94.0。锁定的 metadata、Clippy `-D warnings` 和单元测试退出码都是 0。没有 `cargo update`，没有改根清单，没有把 ignored 的 `closed_loop` 再跑一遍。

## 容量偏差

产品 pg-wrap 的 SHA-256 是 `57cefcd0ca6bb2a0eb0398bb6de430b65fa8de2e742dbd796199e014929dfa0d`。相对已审 `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`，差在 `max_connections=128`。实测 `SHOW` 与此一致，`shared_buffers=64MB`。这是 100 个会话的 fixture 容量。已审包装用 20，装不下这轮 fixture。本复查不把它写成生产配置认证，也不另开数据库攻击。
