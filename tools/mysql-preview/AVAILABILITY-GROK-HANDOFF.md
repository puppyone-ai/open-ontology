# MySQL 当前可用性移交

四条当前 serve 转交 root 继续运行。作者本轮不停止它们，也不停止默认全局 registry。

## 进程

容器 `oo-mysql-rust-runtime-linux-v8`。ELF 目录 `availability-v1/bin/c7a216a654c650a496b43d20fc34cfccd1a8e9e8cce863d53ddea142434221a0/source-onboarding-linux`。

| PID | out |
|---:|---|
| 25907 | `availability-v1/root-canonical-recovery-20261017/original-mysql57-serve` |
| 25913 | `availability-v1/root-canonical-recovery-20261017/original-mysql8-serve` |
| 25914 | `availability-v1/root-canonical-recovery-20261017/new-static-mysql57-serve` |
| 25925 | `availability-v1/root-canonical-recovery-20261017/new-static-mysql8-serve` |

`ps` lstart：Fri Oct 16 16:22:31 2026。operator `serve_seconds=86400`，`period_ms=5000`。24 小时上限约 2026-10-17T16:22:31Z。同一 binding 不要再启动第二个 serve。不要改 jobs 水位，不要手改 `applied_pos`、generation 或 epoch。

## 客户端期限

收据 `availability-v1/token-renewal-0600-v3/receipt.json`。四条 `absolute_deadline_ms`：

- original-mysql57 `1792171682693`，2026-10-16T17:28:02.693Z，角色 `oo_r_mysql57_full`
- original-mysql8 `1792171683319`，2026-10-16T17:28:03.319Z，角色 `oo_r_mysql_full`
- new-static-mysql57 `1792171683929`，2026-10-16T17:28:03.929Z，角色 `oo_r_mysql57_avail_full`
- new-static-mysql8 `1792171684513`，2026-10-16T17:28:04.513Z，角色 `oo_r_mysql8_avail_full`

通过轮剩余 2781.1–2783.3 秒。客户端 TTL 1 小时，与 serve 的 86400 秒是两件事。期限之后进程仍可按 24 小时上限活着，但当前 token 文件不再满足验收器的账本剩余。续签走官方 `issue-token`，写入独立 0600 目录，只替换 mutable `current-client.token`，并记录旧新 SHA 与 expiry。不要改旧 ready pin。不要打印 token 原文。

## 验收

```
python3 /tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/availability-v1/verify-current-availability-grok47.py
```

脚本 SHA256 `914b91637fbf0bd1660a606f3454befb8f59627a3ae03d2de5d8fde50c38279e`。通过原文 `verifier-run-after-index-fix.json`。失败原文 `verifier-run-before-index-fix.json` 保留。0 模型，0 源 DML。外来 cached `--raw --no-abbrev` 排除固定路径后必须仍是 `cb53ce84e87327bdee00e90a0b910c5aad314c301d966601edba019fbf9d22ad`。

单入口 `/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/availability-v1/DELIVERY.json`。父级 mainline `DELIVERY.json` 与 `availability-v1/HANDOFF.json`（SHA256 `6a1b16da4092fedce2092fc24f29b714dcc761003c2992f0251675f2ab2aaaab`）保持原文件。

固定代码提交是 `9b9ecb5c82c50f946393007982725dd4481fa888` 与 `b5a638ceffc55a6c6235d4cd58ba54d1d0c9fbfa`。报告提交只含本目录四份 `AVAILABILITY-GROK-*.md` 与验收器副本。未 push。未改 Goal、CONTROL、主 Plan、INDEX、总台账。

root 若验收不通过，沿同一冻结任务继续。本移交不把正式源、发行或十源 review 计为通过。
