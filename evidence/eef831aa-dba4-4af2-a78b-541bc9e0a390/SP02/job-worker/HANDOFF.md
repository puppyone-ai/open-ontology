# 交接

会话 `eef831aa-dba4-4af2-a78b-541bc9e0a390`。实现、测试和复查由同一人完成，没有子代理，没有 worktree。

## 已固定的代码

提交 `1c4797e2f846da7744565c0f95103b68c1d01846`。

```text
job-worker: run the SP02-W01 lease loop on the published jobs functions
```

该提交只包含 `tools/job-worker` 的 26 个文件。逐文件 SHA-256 见 `source-sha256.txt`。冻结合同是 `tools/job-worker/CONTRACT.md`，写于实现之前，本轮没有改它。

## 已跑通的证据

通过轮是 `linux/run-attempt-5`。`closed-loop.exit=0`，`report.json` 的数字写在 `REPORT.md`。失败轮是 `linux/run` 到 `linux/run-attempt-4`，原因写在 `failed-attempts.txt` 与 `FINDINGS.md`。改代码前的工具链 smoke 在 `linux/smoke-before`。

复跑命令是 `bash tools/job-worker/ops/launch.sh test`。它使用本机已有的固定镜像，不拉取。全局 Rust builder 上限 10，本任务上限 1。容量已满时脚本以退出码 3 停下。宿主上不运行 cargo 或 rustc。

## 清理

拥有资源的名字以 `oo-a390-jw` 为前缀，标签 `oo.owner=a390` 且 `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a390`。清理命令是 `bash tools/job-worker/ops/launch.sh down`。`linux/cleanup.exit` 为 0。`linux/cleanup.txt` 记录删除了容器 `oo-a390-jw-pg`、卷 `oo-a390-jw-pgdata`、`oo-a390-jw-cargo`、`oo-a390-jw-target`、网络 `oo-a390-jw`，以及 `/tmp/oo-a390-eef831aa-dba4-4af2-a78b-541bc9e0a390/secrets`。Rust builder 在测试结束时已经删除。没有 prune，没有留下带本会话标签的容器、卷或网络。

`evidence-sha256.txt` 列出本目录里除它自己和 `evidence-sha256.self.txt` 以外的每个文件。`evidence-sha256.self.txt` 是清单文件自己的 SHA-256。

## 不要代签

本交接不接受父项 SP02-W01，不接受 AT-CTL-01 或 AT-CTL-02，不接受目标围栏、备份恢复或发行。证据提交只覆盖本目录。没有 push。

## 下一手如果要动核心

要补的是只读 SQL，而不是再包一层驱动：函数内通知、schedule 载荷列、检查点与副作用的正式位置、`jobs.enqueue` 在读已提交下的 `OO016`，以及 jitter。那些改动属于 `crates/oo-jobs` 与合同，本任务的可写范围不包括它们。
