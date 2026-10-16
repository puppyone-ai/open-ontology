2026-10-16 当前健康复查更正：两版默认真实Gateway/CLI查询均成功且hubs计数12002，但四条serve中隔离MySQL8 g4的PID92824已退出2，native-fire原始错误unexpected EOF；其余三PID仍存在、默认MySQL8 g13未受此退出影响。不能保证当前完整链路持续稳定可用。此前固定整链/8随机问答证据保留为历史结果；本次未重跑模型、未重启掩盖失败、无DML，根因/恢复待处理。总Goal/其他源暂停及计数不变。回执 execution/reviews/native-luna-full-source-20261015/mysql-mainline-current-health-20261016.json。

# MySQL 已验本地 Rust 演示入口

当前固定核验：[主线接收回执](mysql-mainline-batch-reader-intake.json)。源码 b60bd6fb5；正式 oo-connectors::mysql、可选 feature 和真实 C1 消费已经接线。MySQL 5.7.44-log/8.4.10 各110019行、5表43列 typed0；新隔离scope共8轮随机对话已核源gold/CLI cells/原QID WAL。原两条默认链是g9/g13，分别经typed/当前CLI/quiet验证；不套用新scope问答身份。

在本机执行，进入现有受信operator容器；查询通过真实只读Gateway，支持任意合法SQL，不限示例。

```sh
docker exec oo-mysql-rust-runtime-linux-v8 oo-mysql57 tables --json
docker exec oo-mysql-rust-runtime-linux-v8 oo-mysql57 query "SELECT COUNT(*) AS row_count FROM native_mysql57_20261016.hubs" --json
docker exec oo-mysql-rust-runtime-linux-v8 oo-mysql8 query "SELECT COUNT(*) AS row_count FROM native_mysql_20261012.hubs" --json
```

停用14个旧Gateway后，两条COUNT已真实执行并核完整QID WAL，预期均12002。四条当前Gateway和原卷/证据保留。

默认 `source-onboarding` 已指向最终Rust ELF，私有registry仅登记原两source的新operator；两版不传 `--operator` 的 `check --input SIX_FIELDS --out NEW_PRIVATE_DIR` 均实际通过。隔离scope保持显式operator，不替换原身份。已有每binding唯一serve，不重复启动。输入/官方重入与边界见[操作说明](../../../tools/mysql-preview/MAINLINE-OPERATIONS.md)。

四条serve周期1000ms，有界运行至2026-10-17 20:59:46（中国时间），exit仍pending，无boot daemon。客户端令牌TTL仅1小时，必要时按操作说明官方显式续签到独立0600文件，无自动续签；服务期限不等于令牌期限。

限制：初始C1静态baseline提交时间NULL时freshness会拒绝，尚不能承诺任意客户六字段即完整通过。仅当前单整数主键有界读取与两实测版本；非生产认证，原父项/十源/整版/统一复审未签收。总Goal与其他源暂停。
