# DL01-W1-76 交回 DL01-W1-72

判定：`REQUEST_CHANGES`。

交回原公共 Gateway / SDK / CLI 工程。固定源仍是 `91031011238c39e571a28a90831b012f225862d6`。一次返工只修下面这一项。不要扩大成 Export、MCP、发布、算子合同或整源签收。能力分母保持 GOAL 完整签收 0/10、受控预览 6/10，CONTROL 集成 0/4、发行 0/6。

## 返工

公开 SDK 入口 `crates/oo-sdk/examples/public_call.rs` 的 `read_file`（约 178 行）用 `fs::metadata` 再 `fs::read`。`fs::metadata` 跟随 symlink。它不检查 mode，不检查 owner，不用 `O_NOFOLLOW`。`crates/oo-sdk` 下没有任何 `O_NOFOLLOW` 或 `0o600`。库 `Client::new` 接收已载入的 bearer，不打开文件。要修的是这个示例二进制，以及任何同样自己读密钥文件的 SDK 公开入口。

拒绝必须发生在 TCP 之前，与 `crates/oo-cli/src/load.rs` 一致：

- token 文件 mode 不是 0600，包括 mode 000 和 mode 644，进程退出 4，消息 `token file rejected: mode`。
- token symlink 进程退出 4，消息 `token file rejected: symlink`。
- token owner 与进程用户不一致时按 CLI 的 permission 规则拒绝。
- 该示例若读取 client pem、CA 或 SQL 文件，mode 000 与 symlink 在连接前拒绝，进程退出 1，消息分别落到 `client pem rejected`、`ca file rejected`、`sql file rejected`。
- 拒绝时对端 accept 计数保持 0。root 或具备 `CAP_DAC_OVERRIDE` 时，mode 000 仍要拒绝。

CLI 这次已经满足上述行为，返工不要改松 CLI。

复现，审查资源 `oo-dl01w1-76`，调用方容器内 root，perl `IO::Socket::INET` 监听 `127.0.0.1:18443`，二进制是本次 ELF `public_call` `681d1b1aeb2d65fee0d113fa43ae491e0db6615ff03edc4ad3c86a9d49127a7c`：

1. token mode 644：进程退出 4，stderr `transport connect failed; the client stopped`，accept 从 0 到 1。
2. token mode 000：同样 stderr，accept 到 2。
3. token symlink：同样 stderr，accept 到 3。
4. 同一组文件用 `oo`：mode 与 symlink 都是退出 4 且 accept 保持 0；pem mode 000、pem symlink、CA symlink、SQL mode 000、SQL symlink 都是退出 1 且 accept 保持 0。
5. 对照：mode 600 的真实 token 打向同一非 TLS 端口，CLI 退出 4，消息 `transport connect failed`，accept 到 4。这说明计数器记录的是真实连接，SDK 的退出 4 是连上之后的传输失败。

原始正文在 `raw/sdk-token-644.stderr`、`raw/sdk-token-000.stderr`、`raw/sdk-token-link.stderr` 与对应的 CLI `raw/cli-token-*.stdout`。阶段记录在 `raw/phases.log` 末尾，`2026-10-12T01:17:03Z` 至 `2026-10-12T01:17:06Z`。

## 不要顺手改的部分

这些在本次探针里成立，或明确不属于本卡。改它们会超出返工。

- 运行中取消已经是公开 `oo cancel` / SDK cancel，`outcome=sent`，hint `sqlstate 57014`，完成 WAL `rollback` true，随后查询退出 0。迟到取消有独立 durable WAL，已有 `cancel.wal` 时相位是 `cancel-2`。跨用户取消返回 `query was not found`。不要用停容器或 `pg_cancel_backend` 代替。
- 用户取消与 deadline 的 code 都叫 `TIMEOUT`。正文用消息和 57014 hint 分开。docs §7 的 `TIMEOUT` 释义是执行超时。这次不要求为改名而动取消路径。
- CLI 网关错误进程退出 2，SDK 示例对非 401/403 退出 1。正文 `retryable` false。这次不要求把两边合成同一个进程码。
- freshness `publication` 是 `not_ready`，警告 `STALE_DATA`。发布修复归 73。不要在本返工里把 Pack 标成已发布。
- `oo export`、`oo mcp`、裸 `oo sql` 退出 2。原 72 已写明未实现。缺这些不是这次缩 scope，也不要在这次补上。
- `contracts/gateway/policy-catalog-v1.md` 的 54 个算子与代码里的 60 仍不一致。作者声明不改那份合同。这次也不改。
- 作者 `journey/driver.exit` 1 与 `repair-run-1` 的 `audit=not_durable` 继续保留为历史记录。

## 复审已核对、返工后仍须保持的现场结果

typed 全量 50 行与 `amount > 10` 的 41 行，CLI、SDK、独立 oracle 位置对齐且主键对齐，mismatch 0，进程退出 0。知识、搜索、describe、validate、tables 退出 0，版本都是 2。隐藏列与 `_oo_pos` 为 `COLUMN_NOT_ALLOWED`。旧物理名为 `STATEMENT_NOT_ALLOWED`。读者令牌为 `knowledge pack is not loaded`。无令牌、错误证书、错误 CA、过期、错误 audience 退出 4。撤销剩余 4530 毫秒时仍成功，5330 毫秒后退出 4。令牌库故障在截止前成功、截止后退出 4。collector 失败仍返回行并在下次 replay。WAL 不可用时退出 1、`AUDIT_UNAVAILABLE`、该 query 无 admit WAL、响应无行。

## 审查资源

已拆除五个 `oo-dl01w1-76` 容器、网络 `oo-dl01w1-76-net`、卷 `oo-dl01w1-76-pgdata` 与 `oo-dl01w1-76-target`，并删除主机临时目录。`oo-dl01w1-46-tgt` 仍是 running，启动时间 `2026-10-10T21:24:53.463368972Z`。作者缓存仍在。细节在 `cleanup.json`。
