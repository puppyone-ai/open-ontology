# 交接

会话 `eef831aa-dba4-4af2-a78b-541bc9e0a396`。独立复查由同一人完成，没有子代理，没有 worktree。产品树只读。

## 结论

Verdict: `request_changes`。固定产品提交 `1c4797e2f846da7744565c0f95103b68c1d01846`。证据只在本目录。

原卡 SP02-W01-b 的范围保持完整。驱动没有把缺掉的 NOTIFY、schedule 载荷、检查点、jitter 和 OO016 补成计划要求的行为。`jw.effects` 的自插入和内存到达计数都不算业务副作用。

## 已留下的证据

通过轮是 `linux/run`，`launch.exit=0`。决定性数字在 `REPORT.md` 与 `linux/run/probe-before.txt`、`probe-after.txt`、`serve.stderr`。

作者 100 worker 闭环保持在 `86a39f8a` 的 `linux/run-attempt-5`，本复查没有重跑。引用的峰值、租约、心跳、schedule 唯一和 fence 都来自那份归档。

失败轮是 `linux/failed-attempt-1` 到 `linux/failed-attempt-4`，原因在 `failed-attempts.txt`。改前 smoke 在 `linux/smoke-before`。源清单和逐文件 SHA-256 在 `source-inventory.txt`、`source-sha256.txt`。

## 清理

名字前缀 `oo-a396-jw`，标签 `oo.owner=a396`，会话 `eef831aa-dba4-4af2-a78b-541bc9e0a396`。`linux/cleanup.exit=0`。通过轮结束时删除了 PostgreSQL 容器、三个卷、网络和 stage 目录。Rust builder 在 serve 阶段结束后删除。没有 prune，没有清理其他 owner。

复跑：`bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a396/SP02/job-worker-review/ops/launch.sh test`。清理：同一个脚本加 `down`。

## 不要代签

本交接不接受父项 SP02-W01，不接受 AT-CTL-01 或 AT-CTL-02，不接受目标围栏、备份恢复或发行。没有 push。

## 下一手要动的位置

要补的是 `crates/oo-jobs` 里的发布函数和表，驱动的权限边界补不了这些：

- `retry_failure` 的延迟要带 jitter，调用方现在不能改 `run_at`。
- 重复计划的一次副作用要落在业务写入上。`jw.effects` 只保存事务号。
- NOTIFY 若仍由驱动代发，需要一条先入队、后有人 LISTEN、并且能区分叫醒和 1 秒轮询的记录。七个核心函数目前没有 `pg_notify`。
- `jobs.schedules` 没有载荷列。
- OO016 需要一次胜者尚不可见的重叠入队。本次等待方在提交后得到 `replayed`。
- 备份恢复仍未审计。
