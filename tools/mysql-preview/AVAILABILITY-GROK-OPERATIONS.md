# MySQL 当前可用性操作

## 复跑验收

在仓库 `/Users/redux/puppy-one/open-ontology` 可被 `git -C` 读到的前提下：

```
python3 /tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/availability-v1/verify-current-availability-grok47.py
```

exit 0 且 JSON `ok=true` 才算本脚本通过。它会读四条活进程、已发布 generation 的 typed oracle / gateway-ready / maintenance、Gateway 容器挂载、当前 token 账本、CLI `tables` 与 `COUNT(*)`、四组 verdict、transcript grounding、WAL、quiet 步骤、外来 index 和指导 commit。它不调用模型，不对源库做 DML。

quiet 要求同一 generation 的 `complete=nochange apply=false ack=false` 跨度至少 600 秒，且末步年龄不超过 90 秒。token 剩余要大于 1200 秒。客户端绝对期限到 2026-10-16T17:28:02.693Z–17:28:04.513Z 之后，这个检查会失败，直到官方续签替换 current token。

仓库副本 `tools/mysql-preview/verify-current-availability-grok47.py` 应与上面执行体同 SHA256 `914b91637fbf0bd1660a606f3454befb8f59627a3ae03d2de5d8fde50c38279e`。

## 不要做的操作

- 不要杀 PID 25907、25913、25914、25925，也不要停 `oo-mysql-rust-runtime-linux-v8` 里这条 C7 serve。
- 不要停默认全局 registry，不要再装一次 C7。
- 不要对同一 binding 再起一个 serve。四个 binding 见 HANDOFF。
- 不要改四份活 operator 的 `after_generation`，不要 SQL 改 jobs 水位。
- 不要重跑 catalog-0 已失败的那四个 out。
- 不要把 `sqlserver` 的 `loader.hex` 拷进 MySQL runtime。
- 不要跑 `renew-and-serve.py`。续签只换 token 文件。
- 不要 `git reset`、`stash`、`git add -A` 或改外来 staged index。提交用 `/tmp/oo-dl01-wave1-git.lock` 和 `git commit --only` 的精确路径。
- 不要 push。不要改 Goal、CONTROL、主 Plan、INDEX、总台账或 `HANDOFF.json`。
- 不要重打 `/tmp/oo-mysql-mainline-final-package-20261016.tar.gz`。引用其 SHA256 `30ef70b64034f317ede82e1e3d2bfe1b4129a2bc7cd28db1ed9cbdff5bec32a5`。

## 增量包核对

父级 mainline 包保持原路径。新增量、manifest 与单入口都在 `availability-v1/`，由 `DELIVERY.json` 给出 SHA。核对时对 manifest 里每个成员重新计算 SHA256，再对 tar 本身计算 SHA256。gold 文件不在增量包里；验收器仍读主机上的原 gold 路径。token 原文不在增量包里。

## 失败原文位置

- 验收器修正前：`/tmp/oo-mysql-avail-grok47-continue/verifier-run-before-index-fix.json`
- catalog-0：四份 `0002-catalog-0.stderr`，正文 `oo-pipeline: sealed import is absent`
- dialogue 工具 exit 2：12 份 `transcript.json`
- GPT 429：两份 `native-model-output/result.json`，`http_status=429`

## 时钟

serve 24 小时上限约 2026-10-17T16:22:31Z。客户端四条期限见 HANDOFF 的绝对毫秒。两者到期都不要靠改账本或改 operator 数字来延长。
