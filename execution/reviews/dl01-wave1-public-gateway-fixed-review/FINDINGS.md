# DL01-W1-76 发现
判定：`REQUEST_CHANGES`。阻断项只有 SDK 示例 `public_call` 的密钥文件门。其余现场切片按原始退出成立。完整阶段表在文末，原始文件在 `raw/`。
## 阻断
`public_call` `read_file` 跟随 symlink，不看 mode 与 owner。现场 accept 计数：SDK mode 644 为 1，mode 000 为 2，symlink 为 3。三次进程退出都是 4，stderr 都是 `transport connect failed; the client stopped`。同一监听上 CLI 的 mode 000、mode 644、symlink 退出 4 且 accept 保持 0。mode 600 的 CLI 对照把 accept 加到 4。详见 REVIEW.md 的决定性失败表。
## 对齐
`raw/compare.json`：CLI、SDK、oracle 各 50 行，位置相等，`note_id` 对齐相等，`positional_mismatch` 0。谓词 41/41/41，对齐相等。列类型 int8、numeric(38,4)、bytea、uuid、timestamptz、text。第 1 行 amount 与 receipt 为 null。第 2 行 amount `-0.0001`，receipt `6162`。第 3 行 amount `3.2500`，`signed_at` null。知识版本 2，策略版本 2。对应进程退出：`cli-query`、`sdk-query`、`oracle`、`cli-predicate`、`sdk-predicate`、`oracle-predicate` 都是 0。
作者 `journey/driver.exit` 仍是 1，那次 oracle 按文本排序。本次比较不是那次 driver。审查夹具在监听器拆除时退出 143，没有写下自己的 `driver.exit`。gold 路径的进程退出与对齐数字如上。
## 取消与审计
CLI 取消 `q_QKOzcPrTMnLw6IzYS1BVI6VoNF` / `s_AOVW8uweoqlLVUqdF3uQEwypfI`：`durable` `sent`。原查询 hint `sqlstate 57014`。完成 WAL `returned_rows` 0，`rollback` true。迟到 server 取消写入 `q_QKOzcPrTMnLw6IzYS1BVI6VoNF-cancel-2.wal`，`outcome` `not_sent`，相位 `cancel-2`。复用查询 `q_MxQIgUg4uKstEL5R1BgQjXk2fM` 的迟到取消写入该 id 的 `cancel.wal`，`not_sent`，相位 `cancel`。状态查询返回 `succeeded`。SDK 取消 `q_L0dkuZKxq6LVBrOnhXvNFx8Ymv` 的 `cancel.wal` 为 `sent`，完成 WAL `rollback` true，`returned_rows` 0。跨用户取消 `SCHEMA_INVALID` / `query was not found`。没有 containment，没有 `pg_cancel_backend`。
WAL 目录共 37 个文件：admit 15，complete 15，cancel 3，freshness 2，`cancel-2` 1，alert.log 1。WAL 不可用查询 `q_ctVS5PCNFtVHeAG8XVeOIPSSSC` 无文件。`wal-down-active.txt` 为 0。`tup_returned` 6063384 到 6070804 不能归因。collector backlog 名见 `raw/collector-backlog-names.txt`。失败标记两条 `audit_collector=failed`，重放标记再加两条 `audit_collector=replayed`。
## 新鲜度、撤销、隔离
CLI 与 SDK freshness 的 `publication` 都是 `not_ready`。`freshness` 长度 0，`sources` 长度 0。警告一条，code `STALE_DATA`。消息含 generation 1 bound false、generation 2 bound true、publication not_ready，以及 table_freshness、source_watermark、table_quality absent，apply_receipts 与 full_import_* not_readable。pack digest `c175f0970faccdb0ff1347da43993360b7e2345521dd55630cc5e7ed9bdfeb79`。
撤销：before 剩余 4999，immediate 剩余 4530 且退出 0，睡眠 5330 后退出 4。令牌库：warm 剩余 3950，during 剩余 3749 且退出 0，睡眠 4549 后退出 4，恢复后退出 0。`store-warm-slept.txt` 为 `kept`。
describe 列名为 amount、memo、note_id、note_token、receipt、signed_at。validate `valid` true，type oid 20、1700、17、2950、1184、25。知识页 sha256 `0ca4276c37013f2f5f8e8e96a7886a3ecafaed2363dd8b75f886198501805e87`，正文含 `Row security: none`。隐藏列与 `_oo_pos` 为 `COLUMN_NOT_ALLOWED`。旧物理名、残缺 SELECT、UNION、注入形语句、浮点字面量为 `STATEMENT_NOT_ALLOWED`。读者令牌 `knowledge pack is not loaded`。公开 stdout/stderr 泄漏扫描 0 条。物理名只出现在 `raw/reviewer-only/inflight-activity.txt`。
数据面查询窗口的远端客户端记录在 `raw/data-clients.txt`：`local postgres active` 与 `192.168.80.5/32 oo_dl01w1_46_target idle`。调用方地址 `10.76.76.5` 不在其中。
## 作者套件与本次 actual run
固定提交里 `linux/suite/skipped.txt`、`linux/suite2/skipped.txt`、`linux/repair-cli-1/skipped.txt` 四行相同：`policy_pg_live`、`reviewed_policy`、`reviewed_policy_cli`、`reviewed_policy_quantity`，原因 `foreign-builder-or-mutating-live`。那些跳过不是下表的退出。下表是 `oo-dl01w1-76` 上本次进程退出。
## 阶段退出
| 阶段 | 退出 | 注 |
|---|---|---|
| `cli-after-revoke` | 0 |  |
| `cli-audience` | 4 |  |
| `cli-bad-cert` | 4 |  |
| `cli-bad-sql` | 2 |  |
| `cli-ca-link` | 1 |  |
| `cli-cancel-cross-user` | 2 |  |
| `cli-cancel-inflight` | 0 |  |
| `cli-cancel-late-query` | 0 |  |
| `cli-cancel-late-server` | 0 |  |
| `cli-closed` | 2 |  |
| `cli-collector-replay` | 0 |  |
| `cli-collector` | 0 |  |
| `cli-deadline` | 2 | TIMEOUT，消息 query deadline exceeded，无 57014 |
| `cli-describe` | 0 |  |
| `cli-expired` | 4 |  |
| `cli-export` | 2 |  |
| `cli-float` | 2 |  |
| `cli-freshness` | 0 |  |
| `cli-help` | 0 |  |
| `cli-hidden` | 2 |  |
| `cli-inflight` | 2 | TIMEOUT，hint sqlstate 57014，消息 query canceled |
| `cli-inject` | 2 |  |
| `cli-knowledge` | 0 |  |
| `cli-mcp` | 2 |  |
| `cli-no-token` | 4 |  |
| `cli-old-physical` | 2 |  |
| `cli-operations` | 0 |  |
| `cli-pem-000` | 1 |  |
| `cli-pem-link` | 1 |  |
| `cli-pos` | 2 |  |
| `cli-predicate` | 0 |  |
| `cli-query-clients` | 0 |  |
| `cli-query` | 0 |  |
| `cli-reader` | 2 |  |
| `cli-reuse` | 0 |  |
| `cli-revoke-after-deadline` | 4 |  |
| `cli-revoke-before` | 0 |  |
| `cli-revoke-immediate` | 0 |  |
| `cli-search` | 0 |  |
| `cli-sql-000` | 1 |  |
| `cli-sql-bare` | 2 |  |
| `cli-sql-link` | 1 |  |
| `cli-status` | 0 |  |
| `cli-store-after` | 4 |  |
| `cli-store-during` | 0 |  |
| `cli-store-restored` | 0 |  |
| `cli-store-warm` | 0 |  |
| `cli-tables` | 0 |  |
| `cli-token-000` | 4 |  |
| `cli-token-600-connect` | 4 | 对照。非 TLS 监听，accept 4，证明计数器有效 |
| `cli-token-644` | 4 |  |
| `cli-token-link` | 4 |  |
| `cli-union` | 2 |  |
| `cli-usage` | 2 |  |
| `cli-validate` | 0 |  |
| `cli-wal-down` | 1 | AUDIT_UNAVAILABLE，retryable true，无结果行 |
| `cli-wal-restored` | 0 |  |
| `cli-wrong-ca` | 4 |  |
| `install-control` | 0 |  |
| `issue-audience` | 0 |  |
| `issue-east` | 0 |  |
| `issue-east2` | 0 |  |
| `issue-expired` | 0 |  |
| `issue-reader` | 0 |  |
| `issue-revoke` | 0 |  |
| `oracle-predicate` | 0 |  |
| `oracle` | 0 |  |
| `pg-ready` | 1 | 内容 1 表示 pg_isready 成功，不是进程失败 |
| `pg-role` | 0 |  |
| `revoke-sleep` | 0 |  |
| `revoke-token` | 0 |  |
| `sdk-after-cancel` | 0 |  |
| `sdk-cancel-inflight` | 0 |  |
| `sdk-deadline` | 1 | 同一 deadline，SDK 进程退出 1 |
| `sdk-describe` | 0 |  |
| `sdk-freshness` | 0 |  |
| `sdk-inflight` | 1 | 同一取消，SDK 进程退出 1 |
| `sdk-knowledge` | 0 |  |
| `sdk-predicate` | 0 |  |
| `sdk-query` | 0 |  |
| `sdk-search` | 0 |  |
| `sdk-token-000` | 4 | 返工。accept 2 |
| `sdk-token-644` | 4 | 返工。accept 1。传输失败，不是文件拒绝 |
| `sdk-token-link` | 4 | 返工。accept 3 |
| `sdk-validate` | 0 |  |
| `secret-files` | 0 |  |
| `store-disconnect` | 0 |  |
| `store-sleep` | 0 |  |
| `store-warm-check` | 0 |  |

阶段数 89。`pg-ready` 的 1 是就绪标记。其余退出码来自进程 `$?` 或探针记录的命令状态。审查脚本自身没有 `driver.exit`。
