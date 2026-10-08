# SP02-W01 Rust jobs 运行时闭环

会话 `eef831aa-dba4-4af2-a78b-541bc9e0a390`。源码提交 `1c4797e2f846da7744565c0f95103b68c1d01846`，路径只含 `tools/job-worker`。证据在本目录。`crates/oo-jobs`、`crates/oo-control`、迁移、合同、规格与总账保持只读。

私有 PostgreSQL 17.10 上的领取、执行、成功、失败、重试、取消、杀死进程后按检查点恢复，已在 `linux/run-attempt-5` 跑通。父项 SP02-W01、AT-CTL-01、AT-CTL-02、目标围栏、备份恢复和发行都未签署。

## 通过的一轮

目录 `linux/run-attempt-5`。同一 builder 进程内 100 个 Rust worker。数据库时钟决定 30 秒租约和 10 秒心跳。`NOTIFY oo_jw_wake` 只唤醒，1 秒轮询是退路。

| 检查 | 结果 |
|---|---|
| `cargo clippy --locked -D warnings` | `clippy.exit=0` |
| `cargo test --locked` | `test.exit=0`，6 个测试通过，live 测试保持 ignored |
| `OO_JW_LIVE=1` 的 `closed_loop` | `closed-loop.exit=0`，92.31 秒，1 passed |
| builder | `docker-run.exit=0`，随后容器已删除 |
| PostgreSQL | `pg.exit=0`，17.10 aarch64 |
| `cargo metadata --locked --no-deps` | `metadata.exit=0` |
| 第一次 serve | `unix_wait_status(9)`，测试主动 SIGKILL |
| 第二次 serve | `unix_wait_status(0)`，`kill -TERM` 后正常退出 |

原始输出在 `linux/run-attempt-5/` 与 `linux/run-attempt-5/live/`。`live/` 是测试写入的目录，内容与阶段副本一致。`report.json` 是断言输入。

## 验收数字

`linux/run-attempt-5/live/report.json`：

| 项 | 观测 |
|---|---|
| 100 个到期任务同时处于 running，然后各自成功 | `bulk_running_peak=100`，`bulk_succeeded=100`，`bulk_owners=100`，`bulk_effects=100` |
| 同一身份再次入队 | `replayed=1` |
| 入队与意图同一事务回滚 | `rollback_tasks=0`，`rollback_intents=0` |
| 无人 LISTEN 时的 NOTIFY | `notify_left_queued=1`，随后 `notify_loss_state=succeeded`，`notify_loss_effects=1` |
| 同一 schedule tick 并发触发两次 | `schedule_occurrences=1`，`schedule_tasks=1`，`schedule_effects=1` |
| 失败一次后重试成功 | `retry_attempt=2`，`retry_state=succeeded`，`retry_effects=1` |
| 达到尝试上限 | `dead_state=failed`，`dead_error=attempt_exhausted`，`dead_letters=1`，`dead_effects=0` |
| 终态失败 | `terminal_state=failed`，`terminal_error=worker_failed`，`terminal_effects=0` |
| 非法载荷 | `bad_state=failed`，`bad_error=payload_rejected`，`bad_effects=0` |
| 排队取消 | `queued_cancel_state=cancelled`，`queued_cancel_effects=0` |
| 运行中按当前 owner 与 generation 取消 | `running_cancel_state=cancelled`，`running_cancel_effects=0`，`cancel_follow_terminal=1`，`cancel_follow_succeeded=0` |
| 检查点：generation 1 写满 2 笔后被杀死，过期后旧 owner 不能完成，新 generation 收尾 | `checkpoint_generations=2`，`checkpoint_effects=4`，`checkpoint_state=succeeded`；fence before 为 `lost_lease` / `lost_lease`，插入 0 行；fence after 为 `terminal` / `terminal`，插入 0 行 |
| 租约与心跳用数据库时钟 | `lease_span_seconds=29`（25 到 30），`beat_gap_seconds=10`，`beat_lease_seconds=29` |
| 副作用按事务号 | `effect_rows=107`，`effect_txids=107`，`xact_commit_delta=7741`（不低于 107） |
| 目标围栏保持未认证 | `target_fence_true=0` |
| 成功事件不重复 | `duplicate_succeeded_events=0` |
| 四份迁移校验和与文件一致 | `checksums_match=true` |

107 行效果的组成是批量 100、检查点 4、重试 1、schedule 1、丢失通知后的那一次 1。取消、死信、终态失败、坏载荷、回滚和 fence 的 seq 31 都是 0 行。

`xact_commit_delta` 大于效果行数，因为没有领到任务的 `jobs.claim` 仍会提交，心跳和控制命令也会提交。效果证明是 `jw.effects` 的行数等于不同 `pg_current_xact_id()` 的个数。

第一次 serve 的日志有 100 个不同 worker 的领取、一次 `barrier running=100`，以及检查点与 hold 在 generation 1 上的心跳。第二次 serve 只领取检查点的 generation 2 并完成它。

## 运行环境

改代码之前的 smoke：`linux/smoke-before`，`rustc 1.94.0 (4a4ef493e 2026-03-02)`，`aarch64-unknown-linux-gnu`，`rustc.exit=0`，`cargo.exit=0`。通过轮的 builder 同样是这条工具链，`RUSTUP_TOOLCHAIN=1.94.0`。

镜像未拉取：

- Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- PostgreSQL `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

通过轮容量文件：`own=0 global=0 host_cargo=0 cap_global=10`。Builder 6 GiB、1 CPU、`CapDrop=ALL`、非特权、私有网络 `oo-a390-jw`、pids 4096。PostgreSQL 内存 1073741824、非特权、无宿主端口、`max_connections=128`、`shared_buffers=64MB`。日志中的服务器版本是 PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1) aarch64。

配方来源写在 `linux/run-attempt-5/recipe-origin.txt`。相对已审 `pg-wrap.sh` 的偏差只有 `max_connections=128`。密码留在 `/tmp/oo-a390-eef831aa-dba4-4af2-a78b-541bc9e0a390/secrets`，不在本目录。

## 源与证据摘要

源文件 26 个、5392 行。逐文件 SHA-256 在 `source-sha256.txt`，清单在 `source-inventory.txt`。失败轮次在 `failed-attempts.txt`。证据树的逐文件摘要在清理之后写入 `evidence-sha256.txt`。
