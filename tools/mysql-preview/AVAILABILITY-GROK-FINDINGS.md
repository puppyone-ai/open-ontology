# MySQL 当前可用性发现

## 验收器自身的两处事实错误

第一次只读运行 exit 1。原文保留在 `/tmp/oo-mysql-avail-grok47-continue/verifier-run-before-index-fix.json`。四条错误原文是：

- `new-mysql57 binding serve count 0`
- `new-mysql8 binding serve count 0`
- `original-mysql57 binding serve count 0`
- `original-mysql8 binding serve count 0`

同一次运行里，四条 C7 PID、typed `different_cells=0`、quiet span 约 707 秒、token 剩余约 3215–3217 秒、八个事实、WAL、CLI、外来哈希和指导提交都已通过。binding UUID 不在进程 argv 里，它在 `--operator` 指向的 JSON 的 `onboarding.binding`。修正后按该字段计数，每条 binding 恰好一个非 `sh` 的 C7 serve。

排除列表原来把 `Cargo.lock` 写成裸文件名，也没有 `tools/agent-product-verifier/dialogue.py`。`9b9ecb5c82c50f946393007982725dd4481fa888` 的路径是：

- `crates/oo-gateway/src/public_surface.rs`
- `tools/source-onboarding/Cargo.lock`
- `tools/source-onboarding/Cargo.toml`
- `tools/source-onboarding/src/io.rs`
- `tools/source-onboarding/src/mysql.rs`

排除集再加上 `tools/agent-product-verifier/dialogue.py`。期望外来哈希未改，仍是 `cb53ce84e87327bdee00e90a0b910c5aad314c301d966601edba019fbf9d22ad`。修正前后该哈希都成立。外来 staged index 未改。

## 保留的失败

四条已发布 cut 上的新 out 在 catalog-0 退出。stderr 原文都是 `oo-pipeline: sealed import is absent`：

- `availability-v1/mysql57-live-serve-resume-v1/generation-3/0002-catalog-0.stderr`
- `availability-v1/mysql8-live-serve-resume-v1/generation-3/0002-catalog-0.stderr`
- `availability-v1/original-mysql57-recovered-serve-resume-v1/generation-11/0002-catalog-0.stderr`
- `availability-v1/root-original-mysql8-recovered-serve-v3/generation-15/0002-catalog-0.stderr`

这些 out 保留，没有再启动。当前活链是 `root-canonical-recovery-20261017` 下已在跑的四条，`after_generation` 等于已发布 generation。水位没有用 SQL 改写。

dialogue 工具失败保留在 12 份 transcript：validate exit 2 共 11 次，query exit 2 共 27 次，knowledge_cat exit 2 共 2 次。mysql8 group1 在 resume5 之前的 group1、resume、resume2、resume3、resume4、retry 都留在原目录。GPT HTTP 429 两份 `result.json` 仍在，见报告中的路径。本交付轮没有新的模型调用，也没有新的源 DML。

## 指导与验收边界

`b5a638ceffc55a6c6235d4cd58ba54d1d0c9fbfa` 只改 `tools/agent-product-verifier/dialogue.py`。root 已审阅这 26 行通用指导，认可它没有放松验收。验收器不读这 26 行的措辞，只确认该 commit 对象仍在。`verify_named_facts`、随机选题、coverage 和 gold 值没有因这次交付被修改。

Gateway strict clippy 比较文件记录 `new_strict_exit=101`、`original_520_strict_exit=101`、`new_lint_count=11`、`added_lint_count=0`，处置为既有风格 lint 延期。这里不把 101 写成 0。

四条当前 Gateway 的续签收据都是 `publication=live`、`freshness_len=5`、warnings 空。新链也是这个实测结果。

## 用量口径

唯一 response id 93 个，token 330624。加上两次已发生且本轮未重放的 S5 12536，合计 343160。目标 27411 未达到。cached token 是 prompt 的子集。按文件相加会把成功前缀重复计算。
