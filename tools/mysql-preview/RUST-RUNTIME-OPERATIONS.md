# Rust Runtime Operations

仅用于本次 owned fixture 的 operator 环境。普通输入文件只有六字段；私有 operator 配置由安装者提供。不得用该 fixture 模板重定向到客户账号。

最终 Linux 入口：
`/tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/bin/b0808d42b885366d7825aef1ea266d3120a614180dd8e943ba58a57263e84d68/source-onboarding-linux`

## 只读检查、冷进程启动与重入

容器 `oo-mysql-rust-runtime-linux-v8` 是受信任控制区，持 Docker socket 与必要私有挂载；其命名不代表正在运行旧 ELF。新数据面 Gateway 不持这些挂载。Docker CLI 29.4.1 为官方预编译文件，仅私有挂载，无新 Docker daemon。

```sh
docker exec oo-mysql-rust-runtime-linux-v8 /tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/bin/b0808d42b885366d7825aef1ea266d3120a614180dd8e943ba58a57263e84d68/source-onboarding-linux check --input /tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/mysql57-six-fields.json --operator /tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/mysql57-operator-long-v10b.json --out /tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/NEW-57-check
```

正常冷进程/继续跟随用同一个入口、`serve` 及 `operator-long-v10b.json`，输出必须新目录；8.4 使用 `mysql8-...` 文件。现有两个长 writer 仍运行，不能重复启动。先按下面的 ACK 静点停止，再用原六字段与相同安装身份重入。

当前 cut 的维护重入使用 `operator-v10b.json`（含严格 pinned published cut，after_generation 为当前代减 1）；它只适用于当前已确认 cut。长期配置 `operator-long-v10b.json` 已去掉 refresh-only reuse pin，after_generation 为当前代，未来变化走真实 Loader/新发布，不把旧 cut 套到新代。

```sh
docker exec oo-mysql-rust-runtime-linux-v8 /tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/bin/b0808d42b885366d7825aef1ea266d3120a614180dd8e943ba58a57263e84d68/source-onboarding-linux serve --input /tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/mysql57-six-fields.json --operator /tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/mysql57-operator-v10b.json --out /tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/NEW-57-reentry
```

## 正常停止

通过原 worker 的 `native-enable --binding ... --enabled false` 暂停对应 binding；从控制库/原 worker 确认该 project 无 queued/running capture、ACK 已持久化。核对 resource receipt 的 PID 与 `/proc/PID/cmdline` 后给**该 namespace Rust PID** SIGTERM，保留 wrapper 实际 exit（本次 v8 为 143）。不碰 API/Gateway，不手改 task、epoch、ACK 或收据。重新跟随前经原 `native-enable ... true` 启用；旧 guard 是死 PID 时新入口会留档并正常接续，不删除失败证据。

## Rust OO CLI 与 token

```sh
/tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/mysql57-oo-current tables --json
/tmp/oo-mysql-connection-whole/sol-20261016/rust-runtime-v1/mysql8-oo-current freshness --json
```

operator alias 只选择新的 ready cut，再执行既有 Rust OO CLI，经 Gateway 调用。独立外部客户端仍只使用 Gateway/token。`OO_CURRENT_CONFIG` 可指向独立私有 alias 配置；不要改 QA 原配置来伪造重放。原问答的 CLI response/原 QID WAL 保留在旧 cut，六个中间 GW 已停止；独立 verifier 核对其原 bytes，当前 v10b 另有真实 caller 证据。

管理员 Secret 只放 operator 字段 `onboarding.product.token_admin_password_file`，mode0600，位于 host_secret_root 之内。控制容器通过 `/secrets/...` 读取；新 Gateway bundle 不复制此文件。后续 token 续签也在控制容器调用既有固定 `issue-token issue ENDPOINT ADMIN_FILE CA GRANT NEW_CONTROL_OUTPUT`，写**不同私有文件**，再仅将新 client token 复制到当前 GW 私有目录 mode0600；CLI 显式 `--token-file /run-current/NEW.token`。不要把 admin 文件复制到 GW，也不要覆写原/current QA token。

实际签发 argv（只含私有文件引用）、安全挂载与不可读探针见当前 cut 的 `*-issue-token.started.json`、`isolation-probe-v10b/`；控制容器新增既有 `oo-dl01w1-73` network 仅用于正式 token store，数据面无 Docker socket、source 或 admin Secret。

完整启动参数、挂载、原镜像 digest、ELF/配置/资源 SHA 和实际期限以 `source-and-elf-provenance-v10b.json`、`control-container-inspect-v9.private.jsonl`、`retained-linux-runtime-handles-v10b.json`、`retained-linux-resources-verified-v10b.json` 为准。无 boot daemon、无自动续签；服务正常期限以资源收据中的 UTC 窄窗为准，client token 以实际 token metadata UTC 为准。。原 API/DB/物理副本、固定49pins和旧recipe保持；生产与客户门槛由 root/owner 独立处理。
