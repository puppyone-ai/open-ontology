# SP02-W01-b jobs 运行时独立复查

会话 `eef831aa-dba4-4af2-a78b-541bc9e0a396`。产品提交 `1c4797e2f846da7744565c0f95103b68c1d01846`。作者证据提交 `86a39f8aefc9609769ab370c4b845fb8d9572656` 只读。本目录是这次复查的证据。产品树没有改。

## 范围与结论

Verdict: `request_changes`。

复查对象是原卡 SP02-W01-b 的完整运行时验收：短事务领取、30 秒租约、10 秒心跳、generation CAS、schedule 去重、数据库时钟、NOTIFY 只唤醒且轮询兜底、业务变更与入队同事务、重复计划只有一次副作用、旧 generation 不能完成。`docs/17-rust-implementation.md` 第 3.1 节还要求指数退避加 jitter。核心缺 NOTIFY、schedule 载荷列、检查点列、jitter 和已观测的 OO016，不因此缩小原卡。

已观测、不足以签收的部分：

- 作者归档 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a390/SP02/job-worker/linux/run-attempt-5` 有 100 个真实会话。`serve1.stderr` 有一次 `event=barrier running=100`。`report.json` 里 `bulk_running_peak=100`，`lease_span_seconds=29`，`beat_gap_seconds=10`，schedule occurrence 为 1，fence 先 `lost_lease` 再 `terminal`，取消与重试有记录。本复查没有重跑这轮。
- 本会话私有库上的 1 个 worker 把 `review-once` 从 generation 1 领到 `succeeded`。`jw.claims` 的租约跨度是 29 秒。再次 `complete_success` 返回 `terminal`。
- 两次 attempt 1 的 `retry_failure` 剩余时间是 `0.999760` 秒和 `0.999857` 秒。

未完成的补偿：

- 计划要求的副作用在这次实测里是 `jw.effects` 的一行，txid 为 `760`，内容是 `pg_current_xact_id()`。`jobs.leases` 插入数保持 0。作者归档的 `effect_rows=107` 也是这张自插入表；`xact_commit_delta=7741` 把空领取和心跳算了进去。
- `jobs.retry_failure` 的延迟是 `v_delay := 60` 或 `1::bigint << (attempt - 1)`。函数正文 `random_at=0`。运行时对 `jobs.tasks` 的 UPDATE 权限是 false，驱动补不上 jitter。
- 一次重叠入队的等待方返回 `replayed`，`race_rows=1`。作者通过轮日志里没有 `OO016`。这条竞争分支没有被打出来。

`max_connections=128` 是产品 `tools/job-worker/ops/pg-wrap.sh` 里的 100 worker fixture 容量。`SHOW max_connections` 为 128，`shared_buffers` 为 64MB。这是容量记录，不是生产配置认证，本复查也不把它单列成缺陷。

未签署：父项 SP02-W01、AT-CTL-01、AT-CTL-02、目标围栏、备份恢复、发行。原卡复选框里的备份没有审计。

## 固定来源

| 项 | 值 |
|---|---|
| 产品提交 | `1c4797e2f846da7744565c0f95103b68c1d01846` |
| 归档校验 | `linux/run/archive-commit.txt` 同为该提交 |
| 树 | `tools/job-worker`，26 个文件，5392 行 |
| 作者证据 | `86a39f8aefc9609769ab370c4b845fb8d9572656` |
| 镜像 | Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`，PostgreSQL `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` |
| 产品 pg-wrap | SHA-256 `57cefcd0ca6bb2a0eb0398bb6de430b65fa8de2e742dbd796199e014929dfa0d` |
| 已审 pg-wrap | `deploy/trusted-demo/pg/pg-wrap.sh` @ `10b78993641c88154c75c22219c79c1566e2cecd`，SHA-256 `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a` |

二进制里的 `PROOF_PROJECT` 是 `eef831aa-dba4-4af2-a78b-541bc9e0a390`。探针在本会话私有库里使用这个常量，去重键用 `review-once`、`review-retry-a`、`review-retry-b`、`review-race`、`review-cancel`。没有挂载脏工作区。

## 改前 smoke

`linux/smoke-before`。容器 `oo-a396-jw-smoke` 与 `oo-a396-jw-smoke-cargo` 已删除。

- `rustc 1.94.0 (4a4ef493e 2026-03-02)`，`aarch64-unknown-linux-gnu`，LLVM 21.1.8
- 文件内容是 `rustc_exit:0` 与 `cargo_exit:0`

通过轮的 builder 环境是 `HOME=/tmp`，`CARGO_HOME=/cargo-home`，`CARGO_BUILD_JOBS=1`，`RUSTUP_TOOLCHAIN=1.94.0`，同一条 `rustc 1.94.0`。

## 决定性验证

目录 `linux/run`。容量文件 `own=0 global=0 host_cargo=0 cap_global=10`。Builder 6 GiB、1 CPU、`CapDrop=ALL`、非特权、网络 `oo-a396-jw`。PostgreSQL 内存 1073741824、非特权、`pg-ports.json` 为 `{}`。服务器版本 PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1) aarch64。

| 检查 | 退出码 |
|---|---|
| `cargo metadata --locked --no-deps` | `metadata.exit=0` |
| `cargo clippy --all-targets --locked -- -D warnings` | `clippy.exit=0`，`Finished dev profile` 1m 14s |
| `cargo test --locked` | `test.exit=0`。库 4 通过，freeze 2 通过，`closed_loop` 保持 ignored |
| `cargo build --locked --bin job-worker` | `build.exit=0` |
| `reset` / `apply` | 都是 0 |
| 探针前 | `probe-before.exit=0` |
| 1 worker serve 与 wait | `serve.exit=0`，`wait.exit=0`，`wait=ok` |
| 探针后 | `probe-after.exit=0` |
| 整轮 | `launch.exit=0`，宿主日志 `LAUNCH_PROCESS_EXIT:0` |

没有运行作者的 `inside.sh`，没有设置 `OO_JW_LIVE=1`，没有重跑 100 worker。

探针作为 `oo_a390_runtime_login`，`sslmode=verify-full`，主机名 `pg`。函数正文由本地 socket 上的 `oo_fixture_admin` 读取。

目录与权限：

- `jobs.tasks` 的 INSERT、UPDATE、DELETE 都是 false。列里没有 checkpoint。
- `jobs.schedules` 的列到 `updated_at` 为止，没有 payload。
- `enqueue`、`claim`、`heartbeat`、`complete_success`、`complete_failure`、`retry_failure`、`cancel` 的 `notify_at` 与 `random_at` 都是 0。`claim`、`heartbeat`、`complete_success`、`complete_failure`、`retry_failure` 含 `clock_timestamp`。

重试正文切片：`attempt 1 waits 1s, then 2, 4, 8, 16, 32. attempt 7 and above wait 60s`，随后是 `v_delay := 60` 与 `v_delay := 1::bigint <<`。两次实测：

| dedupe_key | status | attempt | remain_s |
|---|---|---|---|
| review-retry-a | retried | 1 | 0.999760 |
| review-retry-b | retried | 1 | 0.999857 |

重叠入队：持有方 `enqueued` 后睡眠 2 秒再提交，等待方 `replayed`。两边退出码 0。`review-race` 只有 1 行，状态 `queued`。

取消：`review-cancel` 排在一小时后，`cancel_status=cancelled`，`cancel_state=cancelled`，`cancel_effects=0`。

1 个 worker：`event=claimed worker=w-000 task=5960b635-44cd-474f-a1da-57b66c99371d generation=1`，随后 `event=complete status=succeeded`，然后 `event=serve_stop`。任务行 `state=succeeded`，`attempt=1`，`generation=1`，owner 已清空。`jw.claims` 跨度 29 秒，owner `w-000`。`jw.effects` 一行，`seq=1`，txid `760`。再次完成返回 `terminal`。`effect_rows=1`，`effect_txids=1`。

serve 前后 `pg_stat_user_tables` 的变化：`jw.effects`、`jw.claims`、`jw.checkpoints` 各从 0 到 1。`jobs.leases`、`jobs.schedules`、`jobs.schedule_occurrences`、`jw.schedule_payloads`、`jw.intents` 保持 0。`jobs.task_events` 从 10 增到 12，`jobs.tasks` 的更新数从 5 增到 7。这是领取和完成的控制行。任务在 serve 之前已经入队，日志里没有一条由 NOTIFY 叫醒的记录。

## 作者归档里可引用的闭环

`report.json` 与作者 `REPORT.md` 的数字照录，本复查没有重算那一轮：

- 批量 100 成功，100 个 owner，100 行效果。
- 回滚后 tasks 与 intents 为 0。
- `notify_left_queued=1`，随后那条任务成功并有 1 行效果。
- schedule occurrence、task、effect 都是 1。
- 重试 attempt 2 成功。死信 `attempt_exhausted`，效果 0。终态 `worker_failed` 效果 0。坏载荷 `payload_rejected` 效果 0。
- 排队取消与运行中取消的效果都是 0。
- 检查点两代，效果 4 行，状态 succeeded。fence before 插入 0，fence after 返回 terminal 且插入 0。
- `effect_rows=107`，`effect_txids=107`，`xact_commit_delta=7741`，`target_fence_true=0`。
- 107 的组成是批量 100、检查点 4、重试 1、schedule 1、通知丢失后的 1。

`serve1.stderr` 的 `event=barrier running=100` 只有一处。该目录检索不到 `OO016`。

## 保留的失败

四次启动失败都在 `failed-attempts.txt`，原始文件在 `linux/failed-attempt-1` 到 `linux/failed-attempt-4`。它们是复查脚本的错误。通过轮是 `linux/run`。

## 边界

- 没有重跑 100 worker，没有重载已审的大批量数据。
- 没有审计备份恢复。
- 重叠入队只做了一对会话，没有为了打出 OO016 去扫冲突。
- 1 worker 的任务在 serve 前入队，没有单独证明 LISTEN 被通知叫醒。
- 心跳间隔沿用作者归档的 10 秒记录。本探针的任务立即完成，`jw.beats` 仍是 0。
- 密码只在已删除的临时目录里。证据里的 `secret-sizes.txt` 是 49 字节的长度和路径。

## 复现

在仓库根目录：

```text
bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a396/SP02/job-worker-review/ops/launch.sh test
```

脚本归档固定提交的 `tools/job-worker` 与 `crates/oo-jobs/migrations`，不拉取镜像，不映射宿主端口，不挂载 docker socket 或宿主家目录。全局带 `oo.kind=rust-builder` 的容器达到 10，或本任务已有 builder，或宿主已有 cargo/rustc 时，退出码 3。清理是同一个脚本的 `down`。

## 清理

`linux/cleanup.exit=0`。删除了容器 `oo-a396-jw-pg`、卷 `oo-a396-jw-pgdata`、`oo-a396-jw-cargo`、`oo-a396-jw-target`、网络 `oo-a396-jw`，以及当轮的 stage 目录。Rust builder 在 `docker-run-serve.exit` 写入后已经删除。没有 prune，没有动其他 owner 的容器。复查结束时 `docker ps -a --filter name=oo-a396` 为空，带 `a396` 的卷和网络也没有留下。
