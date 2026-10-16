# MySQL 已验本地 Rust 演示入口

固定核验：[签收回执](mysql-rust-native-complete-intake.json)。源码 e3e83b399（常驻运行链）+ac6332654（六字段默认安装），非生产认证。两版当前各110019行、5表43列typed0，四组八轮真实GPT问答已按source gold/CLI cells/原QID WAL验证。

在本机执行，命令进入现有受信 operator 容器；数据查询仍通过真实只读 Gateway。

```sh
docker exec oo-mysql-rust-runtime-linux-v8 oo-mysql57 tables --json
docker exec oo-mysql-rust-runtime-linux-v8 oo-mysql57 query "SELECT COUNT(*) AS row_count FROM native_mysql57_20261016.value_probe" --json
docker exec oo-mysql-rust-runtime-linux-v8 oo-mysql8 query "SELECT COUNT(*) AS row_count FROM native_mysql_20261012.range_gap" --json
```

上述两条查询已在旧Gateway停用后实跑，预期计数分别为 8, 5。任意合法查询由原Rust OO CLI/SQL Guard处理，不限制为这些示例。

六字段接入入口已安装为 `source-onboarding`，配置由operator私有注册，用户不传recipe/receipt/--operator。`check --input SIX_FIELDS --out NEW_PRIVATE_DIR` 两版已实际验证；当前每个binding已有唯一Rust serve，不重复启动。长期重入/正常停启遵循 [操作说明](../../../tools/mysql-preview/RUST-RUNTIME-OPERATIONS.md) 与 [默认安装说明](../../../tools/mysql-preview/RUST-RUNTIME-NATIVE-INSTALL.md)。

客户端令牌今日17:34左右（中国时间）到期，需按操作说明显式续签至新文件；无自动续签。现有有界服务至次日16:38左右（中国时间），无boot daemon。客户/生产/正式源与整版门槛不由本fixture代签。
