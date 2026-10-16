# 当前 MySQL 本地 Rust 演示入口

当前接收：[固定证据与资源回执](mysql-current-availability-final-intake-20261017.json)。实测 MySQL **5.7.44-log、8.4.10**。主线 `oo-connectors::mysql` 按源独立模块、可选 `mysql` feature；实际批量读取与 C1 binlog CDC 已接入 Rust 产品消费者。

四个当前 scope（原两默认 + 两新隔离）各 **5 表、43 列、110019 行，typed 差异 0**。新隔离 scope 从真实静态数据库完成 S0–S10、Grok4.7 S5、标准 YAML/Ontology/Pack、发布、物理只读 Agent DB、Gateway/CLI，再经真实插入/更新/删除维护到 g3。两版各两组随机问答，共 8 轮实际 Grok4.7 对话，逐事实核源 gold、本轮成功 CLI 结果和原始 complete WAL。

本机命令通过当前真实 Gateway，SQL 可以按已发布 schema 和权限自由编写：

```sh
docker exec oo-mysql-rust-runtime-linux-v8 oo-mysql57 tables --json
docker exec oo-mysql-rust-runtime-linux-v8 oo-mysql57 query "SELECT COUNT(*) AS row_count FROM native_mysql57_20261016.hubs" --json
docker exec oo-mysql-rust-runtime-linux-v8 oo-mysql8 query "SELECT COUNT(*) AS row_count FROM native_mysql_20261012.hubs" --json
```

两次 COUNT 当前均为 **12002**。默认原两链为 g11/g15，新隔离两链为 g3/g3，身份和 Pack 分别绑定，不跨 scope 套用对话证据。`source-onboarding` 默认是 C7 Rust ELF，原两 source 的私有默认 operator 已更新且不传 `--operator` 的 check 实测通过；已有唯一 serve 时不要重复启动。

当前四条 Rust serve 周期 **5000ms**，已连续 quiet 超过 10 分钟；有界运行至 **2026-10-18 00:22:32（中国时间）**，不是开机常驻服务。客户端 token TTL **1 小时**，需用官方 issuer 显式续签，无自动续签；实际剩余时间见固定回执。服务期限不代表 token 期限。

初始静态源提交时间未知已可用：诚实返回 `source_time=unmeasured`、空 source freshness 和真实 target observations，不虚构 CDC commit time。既有受信部署前置/角色/凭据/发布仍必需，操作见 [MAINLINE-OPERATIONS](../../../tools/mysql-preview/MAINLINE-OPERATIONS.md)。当前仅实测单整数主键有界读取及上述两版本，不代任意客户/生产/其他数据库认证。其他九源和总 Goal 暂停；统一复审、原父项、十源、发行及整版未签收。
