# MySQL Rust 默认安装入口补充

代码 `ac6332654d41dd7a2bb45cd69c7f4d4ac0da8339` 仅补正常输入到私有安装配置的选择。用户现在可对 `check|capture|follow|serve` 只提供 `--input SIX_FIELDS --out PRIVATE_DIR`；原显式 `--operator INSTALLED` 完整兼容。原 e3/v10b 产品运行、Pack/gold、S5、问答和 49 pins 不改，也未伪造旧 Python whole 协议。

安装者在受信 Linux operator 容器预置固定路径 `/etc/open-ontology/native/mysql-operators.json`：

```json
{
  "protocol": "oo.mysql.native-operators/v1",
  "operators": [
    "/ABS/private/mysql57-operator-long-v10b.json",
    "/ABS/private/mysql8-operator-long-v10b.json"
  ]
}
```

真实 fixture 模板为 `/tmp/oo-mysql-connection-whole/sol-20261016/rust-native-install-v1/mysql-operators.install-template.json`，引用原两个私有 long 配置。registry 与各 operator 文件都必须绝对路径、当前运行 UID 所有、普通非 symlink 文件、无 group/other 权限；建议目录 0700、文件 0600。复用原 `private()` 与 `connection()`，不增加环境变量搜索或 recipe/receipt 用户参数。固定路径安装仅在 Linux 容器完成；host 只保留私有原件/备份，不写 macOS 系统 `/etc`。

registry 严格限定上述两个键及 protocol。六字段必须精确为 type/host/port/database/user/secret_ref，type 必须 mysql；host/port/database/user/secret_ref 必须完整精确匹配唯一安装配置。未知或多匹配在读取 Secret、创建输出和调用 worker 前拒绝；选择成功后仍执行既有 Secret 绑定、私有路径、ELF pin 与后续身份检查。任一 registry/operator 不安全即拒绝，不跳过坏配置。没有新增来源或安装授权。

Linux 1.94 ELF：
`/tmp/oo-mysql-connection-whole/sol-20261016/rust-native-install-v1/bin/8b7693575c099dfd859b2e2a2fc0f10b16e457deeb2296b162c0c9c652e8236c/source-onboarding-linux`

安装固定 registry 后的正常命令（输出目录须为新的私有目录）：

```sh
source-onboarding check --input /ABS/private/mysql57-six-fields.json --out /ABS/private/NEW-check
source-onboarding serve --input /ABS/private/mysql57-six-fields.json --out /ABS/private/NEW-serve
```

当前已有两个 v10b Rust writer，禁止重复启动 serve/capture/follow；须遵循原 OPERATIONS 的正式 binding disable、ACK 静点与精确 PID 停启流程。本补充未重启或替换任何现有 serve/API/Gateway。root 负责把新 native ELF/registry 安装为默认入口、归档移除旧 Python mysql 注册及治理 legacy Gateway；作者没有修改全局安装或旧注册。

验证：隔离容器实际默认 check 两版 exit 0、显式旧入口两版 exit 0；unknown/ambiguous 各实际 exit 2，且无输出目录。Linux 1.94 locked/offline 七测试与 release build exit 0；host Clippy `-D warnings`、fmt exit 0。单测覆盖未知/重定向、额外字段、重复匹配、宽权限文件与 symlink 拒绝。最初 Cargo cache 挂载遮住 image cargo（exit 127，未进测试）、Linux image 缺 Clippy component（exit 1）均保留 raw；修正缓存挂载后测试通过，Clippy 在原 host 工具链执行，未装依赖或宣称 Linux Clippy 通过。

独立固定 archive SHA256 `de2af365941cb8c626e1983b5e0f8c3fca66a9483ef2ab180fc03ebcc2927312`。逐文件枚举确认相对 e3/v10b 原 archive 唯一修改是 `source/tools/source-onboarding/src/main.rs`；未读 foreign WIP。完整 source/ELF/原始命令与退出见同目录 `source-and-elf-provenance-native-install-v1.json`、`fixed-reference-native-install-v1.json`；原始验证在 `/tmp/oo-mysql-rust-native-install-sol-medium-20261016/`。

新增 DML/S5/模型调用 0；新随机问答 0。复用原同身份完整链。正式源增量 0，未宣称生产或全 Goal 完成。
