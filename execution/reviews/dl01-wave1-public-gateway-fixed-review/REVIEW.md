# DL01-W1-76 固定复审

判定：`REQUEST_CHANGES`。

固定提交 `91031011238c39e571a28a90831b012f225862d6` 上，独立 CLI `oo` 与 Rust SDK 示例 `public_call` 经 mTLS 连到 Gateway，读到同一 Pack。本次现场探针里，typed gold、原生 CancelToken、freshness `not_ready`、撤销与故障缓存截止、collector backlog、WAL 不可用时无结果行，都按原始退出成立。SDK 示例 `public_call` 在拒绝 mode 000、mode 644 和 symlink 之前已经对监听端口发起 TCP。这一项退回原 DL01-W1-72 工程，一次返工，能力分母不变。

本复审不批准整源签收、生产发布、客户验收、Export 或 MCP。GOAL 完整签收保持 0/10，受控预览保持 6/10。CONTROL 当前快照的集成 0/4、发行 0/6 不改。父项 SP06、SP09 不签收。发布 73 另有 owner，本报告不把 Pack 写成已发布。没有模型 bearer，本次没有 Agent 对话。

## 固定身份

复审会话 `72f2dc43-1371-481f-bbe2-85e9765847c6`。资源前缀 `oo-dl01w1-76`。源码与 Cargo.lock 来自该提交的 `git archive`，工作区 HEAD `65abb6c7f9ee8af20ecd50de5a6a2e66f15750b0` 没有参与编译。对该 HEAD 做 `git diff 91031011 -- crates/oo-gateway crates/oo-sdk crates/oo-cli tools/gateway-token-store`，无差异。后一笔作者提交 `885e52ab0575` 只改了证据清单和三份作者说明，用来删掉未跟踪的 pyc；本复审不继承它。

作者证据根在固定提交上 `git ls-tree` 1018 条，其中一条是 `hash-manifest.txt` 自身。清单 1018 行。1017 条已跟踪内容的 sha256 与 git blob 一致，`hash_mismatch` 为 0。清单自身不在清单里。唯一不在 git 中的清单行是 `driver/__pycache__/check-repair.cpython-314.pyc`，sha256 `17d34ea6fea9fc4204843332568290dcda99841265e38493103d4b66bedfcbaf`。该文件是非产品证据，缺口保留。

Linux 构建镜像 `rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`。容器内 `rustc 1.94.0 (4a4ef493e 2026-03-02)`，`cargo 1.94.0 (85eff7c80 2026-01-15)`，目标 `aarch64-unknown-linux-gnu`。`cargo build --locked --release`。第一次构建退出 101，原因是审查归档当时漏了 `tools/gateway-token-store`，`oo-gateway` 的 `build.rs` 因此报 `sha256sum failed`。补上该路径并核对 pin 之后，第二次构建于 `2026-10-12T01:04:10Z` 退出 0。Cargo.lock 内容 sha256 `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`。四份 ELF 与作者 `repair-run/elf-probe.stdout`、`linux/suite2/elf-sha256.txt` 的对应哈希一致：

| 二进制 | sha256 |
|---|---|
| oo | `1df4408e220da2e8f51f4bad967671f4c96419d45dc8aede1e93ca1896de6e80` |
| oo-gateway | `6e891bb7a3fce9838de5ab7e31d42252306576c1823b1a514860213bf42a3bbf` |
| issue-token | `17ee78fa6e70f2f9628b83564fe635952ebe20767b38bf060a6a42190d7e2748` |
| public_call | `681d1b1aeb2d65fee0d113fa43ae491e0db6615ff03edc4ad3c86a9d49127a7c` |

均为 aarch64 PIE，动态链接，未 strip。产品树 80 个文件的逐文件哈希在 `product-files.sha256`。

令牌库是本复审新建的 `oo-dl01w1-76-pg`，镜像 `postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`，PostgreSQL 17.10。数据面沿用已在运行的 `oo-dl01w1-46-tgt`，启动时间 `2026-10-10T21:24:53.463368972Z`，本复审没有重启它，也没有写它。调用方容器只在 `oo-dl01w1-76-net` `10.76.76.5`。Gateway `oo-dl01w1-76-gw` 同时在 `10.76.76.3` 与数据网 `192.168.80.5`。查询期间数据面 `pg_stat_activity` 的远端客户端是 `192.168.80.5/32`。SDK 的 Cargo.toml 没有 postgres 驱动，依赖是 reqwest 与 rustls。

## 边界

作者层是 DL01-W1-72 在 `91031011` 的交付。`linux/suite`、`linux/suite2`、`linux/repair-cli-1` 的 `skipped.txt` 都是 `policy_pg_live`、`reviewed_policy`、`reviewed_policy_cli`、`reviewed_policy_quantity`，原因 `foreign-builder-or-mutating-live`。那些跳过不是本次 actual run。作者 `journey/driver.exit` 为 1，原因是 oracle 按文本排序 `note_id`。作者 `repair-run-1` 的迟到取消记录为 `audit=not_durable`。两份记录保留。本次 actual run 另记在 `raw/phase-exits.tsv`。

内核与数据层只作为 Gateway 的上游被读取。公开知识页写明 `Row security: none`。现场列允许集是六列业务列。隐藏列、`_oo_pos`、旧物理名被拒绝。本复审保存的探针里没有另附 `pg_class.relrowsecurity` 快照。

公共集成层是本次判定面：CLI 与 `public_call` 走 Gateway。除 SDK 密钥文件门以外，下面的现场切片成立。

集成与发布层不签。Pack 的 freshness 文档 `publication` 是 `not_ready`，警告码 `STALE_DATA`，`freshness` 与 `sources` 长度都是 0。Export 与 MCP 仍是退出 2 的未实现命令。

## 决定性失败

SDK 示例 `crates/oo-sdk/examples/public_call.rs` 的 `read_file` 用 `fs::metadata` 再 `fs::read`。`fs::metadata` 跟随 symlink。函数不检查 mode，不检查 owner，不用 `O_NOFOLLOW`。`crates/oo-sdk` 全库没有这些检查。库 `Client::new` 接收已经载入的 bearer，不打开文件。缺口在这个公开示例二进制。

调用方容器内以 root 运行，perl 监听 `127.0.0.1:18443`。accept 计数是每次调用后的累计值。

| 调用 | 进程退出 | 消息 | accept |
|---|---|---|---|
| CLI token mode 000 | 4 | `token file rejected: mode` | 0 |
| CLI token mode 644 | 4 | `token file rejected: mode` | 0 |
| CLI token symlink | 4 | `token file rejected: symlink` | 0 |
| CLI client pem mode 000 | 1 | `client pem rejected: permission` | 0 |
| CLI client pem symlink | 1 | `client pem rejected: symlink` | 0 |
| CLI CA symlink | 1 | `ca file rejected: symlink` | 0 |
| CLI SQL `-f` mode 000 | 1 | `sql file rejected: permission` | 0 |
| CLI SQL symlink | 1 | `sql file rejected: symlink` | 0 |
| SDK token mode 644 | 4 | `transport connect failed; the client stopped` | 1 |
| SDK token mode 000 | 4 | 同上 | 2 |
| SDK token symlink | 4 | 同上 | 3 |
| CLI mode 600 对同一非 TLS 监听 | 4 | `transport connect failed` | 4 |

最后一行说明计数器能看见真实 TCP。SDK 的退出 4 是连上非 TLS 端口之后的传输失败，不是文件策略拒绝。CLI 的同组拒绝发生在连接之前。

## 现场成立的切片

探针窗口 `2026-10-12T01:16:34Z` 至 `2026-10-12T01:17:06Z`。主机脚本在监听器拆除时收到 SIGTERM，退出 143，`finish` 没有写下 `driver.exit`。这是审查夹具的拆除，产品阶段文件已经写完。`pg-ready.exit` 的内容 `1` 表示 `pg_isready` 成功，不是进程失败。gold 对齐由随后的主机比较写出：CLI、SDK、独立 oracle 各 50 行，位置相等，按 `note_id` 对齐相等，`positional_mismatch` 为 0。谓词 `amount > 10` 三方都是 41 行，对齐相等。这些阶段的进程退出都是 0。`raw/compare.json` 记下列类型与前三行：`note_id` int8，`amount` numeric(38,4)，`receipt` bytea，`note_token` uuid，`signed_at` timestamptz，`memo` text。第 1 行 amount 与 receipt 为 null。第 2 行 amount 为 `-0.0001`，receipt 为 `6162`。第 3 行 amount 为 `3.2500`，`signed_at` 为 null。`knowledge_version` 与 `policy_version` 都是 `2`。pack digest `c175f0970faccdb0ff1347da43993360b7e2345521dd55630cc5e7ed9bdfeb79`。知识页 sha256 `0ca4276c37013f2f5f8e8e96a7886a3ecafaed2363dd8b75f886198501805e87`。

取消走公开入口 `oo cancel` 与 SDK cancel，键是运行中的 `server_id`。没有停容器，也没有 DBA `pg_cancel_backend`。CLI 运行中查询 `q_QKOzcPrTMnLw6IzYS1BVI6VoNF`，`server_id` `s_AOVW8uweoqlLVUqdF3uQEwypfI`。取消响应 `audit=durable`，`outcome=sent`。原查询退出 2，code `TIMEOUT`，消息 `query canceled`，hint `sqlstate 57014`，`retryable` false。完成 WAL `returned_rows` 0，`rollback` true。取消后 `inflight-still.txt` 为空，数据面上来自 Gateway 地址的 active 查询为 0。随后复用查询 `q_MxQIgUg4uKstEL5R1BgQjXk2fM` 退出 0，`GET` 状态 `succeeded`。跨用户取消退出 2，`SCHEMA_INVALID`，`query was not found`。对已结束 `server_id` 的迟到取消是 `durable` / `not_sent`，WAL 相位 `cancel-2`。对复用查询的迟到 `query_id` 取消是另一份 `durable` / `not_sent`，相位 `cancel`。SDK 运行中取消同样 `sent`，进程退出 1，stderr 带 `sqlstate 57014`。

撤销后立即再调仍成功，`auth_cache_remaining_ms=4530`。睡眠 5330 毫秒后 `UNAUTHENTICATED` 退出 4。另一枚 east 令牌随后退出 0。令牌库断开时缓存内仍成功，`during` 剩余 3749 毫秒；睡眠 4549 毫秒后退出 4；别名恢复后退出 0。缓存没有越过截止。

collector 目录换成失败目标时，查询退出 0，标记 `audit_collector=failed`，backlog 为 `q_lP1exU17xMfKQfKv8Vfeu2MHUs-admit.backlog` 与 `-complete.backlog`。下一次退出 0，标记增加 `audit_collector=replayed`。WAL 目录换成文件后，查询退出 1，code `AUDIT_UNAVAILABLE`，`retryable` true，消息 `audit admission failed`，query id `q_ctVS5PCNFtVHeAG8XVeOIPSSSC`，响应没有行。该 id 在 37 个 WAL 文件中不存在。当时数据面该登录的 active 后端数是 0。共享库 `tup_returned` 从 6063384 到 6070804，差值 7420，库与其他会话共用，这个差值不能用来证明有或没有源元组。恢复 WAL 目录后查询退出 0。

证书、workload、受众：无令牌退出 4；DNS-only 客户端证书退出 4，消息 `transport ended before a complete response`；错误 CA 退出 4；过期令牌与错误 audience 退出 4，`authentication failed`。关闭数据端口的 Gateway 退出 2，`policy catalog is not available`。`oo export`、`oo mcp`、裸 `oo sql` 退出 2，stderr `unsupported command is not implemented in this slice`。公开响应泄漏扫描为 0。`raw/reviewer-only/inflight-activity.txt` 含物理表名，那是审查者的 `pg_stat_activity`，不是公开响应。

## 记下、不进入本次返工

用户取消与 deadline 的 code 都是 `TIMEOUT`。取消带 hint `sqlstate 57014` 和消息 `query canceled`。deadline `q_oyNISGEtcDyJghTfIn1r4wVh5t` 的消息是 `query deadline exceeded`，没有 57014 hint。docs `07-gateway.md` §7 把 `TIMEOUT` 写成执行超时。现场正文能把两者分开。docs §6 要求 CancelToken、等待退出、ROLLBACK/RESET。完成 WAL 的 `rollback` 为 true。

CLI 把不可重试的网关错误映成进程退出 2。SDK 示例把非 401/403 的网关错误映成进程退出 1。两边正文都写 `retryable` false。退出码集合仍是 0、1、2、4。

浮点字面量 `amount > 10.5` 退出 2，`STATEMENT_NOT_ALLOWED`。要求的谓词是 `amount > 10`，该谓词三方 41 行对齐。

`contracts/gateway/policy-catalog-v1.md` 在固定提交仍写 54 个内建比较算子。代码 `required_operator_count` 为 60，单测断言 60，作者写明原因是加入 `numeric`/`numeric`，且本卡不改合同。本复审不把改合同列入返工。

可选请求头不一致没有现场重放。CLI 没有对应旗标。固定源码 `agree()` 在可选头与令牌不一致时返回 `client header does not match the token`。

AT-GW-01 的全量 R1–R9 语料、AT-GW-07 的公平队列与 Valkey、AT-API-01 的 TypeScript SDK、AT-API-03、AT-API-04、AT-MCP-01/02/03 没有跑，保持 pending。docs §9 里的实体解析、值检索、反馈与内部 session token 也不在本次探针。

## REQ 与 AT

状态词：`rework` 本次已看到门未过；`held` 本次探针覆盖的切片成立，AT 整条仍可未关闭；`pending` 未做；`author_retained` 作者记录保留，本次不改判成通过。

| ID | 本复审判定 |
|---|---|
| SP06-W01 权限编译 | `held` 列允许集与隐藏列、`_oo_pos`、旧物理名拒绝。整库 ACL 巡检 pending。 |
| SP06-W02 SQL 规则 / AT-GW-01 | `held` 仅现场负例：残缺 SELECT、UNION、注入形语句、隐藏列。全 R1–R9 语料 pending。 |
| SP06-W02 / AT-GW-02 | `held` describe 返回六列业务列。星号、alias、CTE、JOIN USING 全矩阵 pending。 |
| SP06-W03 / AT-GW-03 | `held` 过期、错误 audience、撤销后 5330 毫秒拒绝、令牌库断开后截止拒绝。session token 与 pubsub pending。 |
| SP06-W03 / AT-GW-07 | pending。没有公平突发、Valkey 或 export 权重探针。 |
| SP06-W04 / AT-GW-04 | `held` 运行中 `server_id` 取消 `sent`、57014、rollback、连接复用、跨用户拒绝、迟到取消独立 durable WAL。断连与 portal 页 pending。 |
| SP06-W05 / AT-GW-05 | `held` typed gold 与 `publication=not_ready`、`STALE_DATA`。八类警告全表 pending。 |
| SP06-W06 / AT-GW-06 | `held` collector backlog 重放，以及 WAL 不可用时 `AUDIT_UNAVAILABLE`、无该 query 的 admit WAL、无结果行。磁盘满与准入后强杀 pending。 |
| SP06-W06 / AT-GW-08 | `held` 跨用户取消不泄露查询，公开响应无物理名。全恶意 literal 语料 pending。 |
| SP09-W01 / AT-API-01 Rust | `rework` 密钥文件。typed 结果与无数据库驱动 `held`。TypeScript 半边 pending。 |
| SP09-W02 / AT-API-02 | CLI 文件门、JSON 退出码 `held`。SDK 示例文件门是上面的 `rework`。 |
| SP09-W03 / AT-API-03 | pending。`oo export` 退出 2。Gateway 对 export purpose 返回 `export is not implemented`。 |
| SP09-W04 TS SDK | pending。 |
| SP09-W05 / AT-API-04 | pending。 |
| AT-MCP-01/02/03 | pending。`oo mcp` 退出 2。原 72 写明 MCP 不在本卡。 |
| 整源 / 客户 / 发布 | pending。不改 0/10、6/10、0/4、0/6。 |

## 资源

已拆除 `oo-dl01w1-76-caller`、`oo-dl01w1-76-gw`、`oo-dl01w1-76-gw-closed`、`oo-dl01w1-76-pg`、`oo-dl01w1-76-rust`，网络 `oo-dl01w1-76-net`，卷 `oo-dl01w1-76-pgdata` 与 `oo-dl01w1-76-target`。拆除后这三个名字下没有容器、网络或卷。主机上的 `oo-dl01w1-76` 临时目录已删。`oo-dl01w1-46-tgt` 仍为 running，启动时间 `2026-10-10T21:24:53.463368972Z`。`oo-dl01w1-46-src` 与 `oo-dl01w1-46-tools` 的启动时间未变。作者缓存 `/Users/redux/.cache/oo-dl01w1-72` 仍在。拆除前，数据面上来自 Gateway 地址的 active 查询为 0。记录在 `cleanup.json`。
