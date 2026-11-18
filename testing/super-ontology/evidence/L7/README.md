# L7 企业与云 · OO 侧证据（D80 阶段一，**未签收**）

状态：**未签收**。原因：真实的 OSS、COS、R2 桶与云队列需要 owner 提供凭证，这部分验收（验收 1）无法在没有凭证时完成。OO `feat/super-ontology` @ 4390b2cb4（独立复核基线见下）。独立复核运行于 `acf198b17`（分支 `so/V1-signoff`，2026-11-18，验证员与执行者分离），结果 `result-verify-signoff.json`：状态“通过，含一次 flake”，62 项通过、0 项最终失败，另有 1 项首次运行失败的 flake 记录（见下）。**没有使用真实的 OSS、COS、R2 桶**，也没有真实云队列；所有云端行为都是对仿真存储、假服务和本地套接字验证的，下表逐项标明。Desktop 的企业桶登记流程与 puppyone-cloud 转发控制 API 按 30 §7 在阶段二补齐。

## 独立复核里的 flake（如实记录，未隐藏）

`share_live::expiry_revocation_and_refs_credentials` 在第一次完整运行时失败（`share_live.rs:136`，创建共享的状态码不是 201，响应体没有被捕获）；单独重跑通过，整个 `share_live` 套件重跑 10/10 通过。没有复现，原因未查明，因此按“flake、未解释”记录。

## 30 §L7 验收（OO 侧口径，30 §7.2）

| # | 验收项 | 结果 | 证据 |
|---|---|---|---|
| 1 | 三家对象存储（oss/cos/r2）各跑通“新增 → 修改 → 删除” | **未做（需要 owner 提供真实桶）**。仿真层面通过：S3 兼容路径（OpenDAL，无新依赖）下 oss、cos、r2 的端点与寻址、区域、CRC64 头（`x-oss-hash-crc64ecma`、`x-cos-hash-crc64ecma`）由 `COMPAT_TABLE` 数据化；仿真存储上 CRC64 校验、坏传输重取、三次后 `ChecksumMismatch`、R2 式存储（无 CRC、无版本、多段 ETag）、时钟偏差都通过（206 个连接器测试；本地 MinIO 上的 `s3_rustfs` 7/7、`cloud_mimic_minio`、`download` 3/3 来自执行者）。真实桶测试 `crates/oo-connectors/tests/cloud_live.rs`（`#[ignore]`，凭证只经 `secret_ref`）已能编译，**没有运行**；“未确认”的兼容单元格仍然没有被真实存储确认 | `result-verify-signoff.json`、`README` 历史部分的命令 |
| 2 | 远端 Worker 拿到的 Pack 与本地编译逐字节一致 | 通过：两个真实 `oo serve`（本地引擎 A、带云入口的远端引擎 B）上，经云入口的 `--remote` 挂载与同一引擎的本地挂载 `pack_digest` 相等（`fac816b68ceff95d`），15 个文件逐字节相同（`SHARE.json` 仅 share id、时间、凭证不同）；云令牌不出现在任何输出与挂载文件里；摘要不符时不留下挂载并撤销共享。**限定：相等性是同一引擎内“本地 vs 经云入口”；两个不同引擎对相同文件的 Pack 不相等**（Drive id 与本体历史进入摘要，AIGC 标签嵌入摘要） | `result-verify-signoff.json`、`l7-e2e-result.json` |
| 3 | 审计能回答“谁在何时把什么共享给了哪个 Agent” | 通过：经租户 acme 云入口给 agent-7 的共享，审计记录 actor、agent、grantor、drive、范围与角色、时间、refs 读取，撤销也在答案里；哈希链完整（4 行，伪造行可被发现）；审计表只能插入，所有者也不能删改或截断；审计不含凭证；`oodc_` 令牌只存哈希，回环入口拒绝云令牌（401），云入口拒绝控制令牌（401），非白名单路由与令牌 Drive 之外一律 403 | `audit-who-shared-what.json`、`result-verify-signoff.json` |

## S11、S12 其余交付（OO 侧，30 §3 L7 条目）

| 条目 | 结果 | 证据 |
|---|---|---|
| 事件通知 + 对账（S11-02） | 通过：事件去重（`raw_key, version_id, op, sequencer` LRU 加每键最高 sequencer）、乱序与重复投递（300 段历史属性测试，视图等于存储）、S3/R2/OSS 解析器、COS 解析器；独立复核 27 个事件测试、5 个 COS fixture 测试；真实引擎上关掉轮询后新文件在事件 4.6 s 后 `published`，重复事件计为 duplicate、迟到旧事件计为 stale，删除事件只上报。**COS 通知格式只来自公开文档的合成 fixture，未被真实桶确认** | `result-verify-signoff.json` |
| SQS、Cloudflare Queues、阿里云 MNS 拉取适配器 | 仅对假服务与本地套接字通过（11 个测试：SigV4 向量、处理后才确认、重投递被去重）。**没有对真实队列试过，MNS 签名尤其未确认；owner 事项** | `result-verify-signoff.json` |
| 海量列举分页（S11） | 通过：按一级前缀并行列举，每个前缀独立代际计数，一个前缀失败不被清扫也不拖住其他前缀；一级范围划分 2 个、并行上限 2 个、`prefix_sync` 3 个测试通过 | `result-verify-signoff.json` |
| Drive 级访问策略（S11-03） | 通过：`drive.config.policy {allow, deny}` 在入库（全量、定向、事件）和可见图两处执行；两个引擎上 `deny Payroll` 的 Drive 里只有两个允许的文件被列出并发布，整 Drive 挂载不含被拒文件；变为被拒的文件被清扫；`policy_live` 2/2、单测 7 个 | `result-verify-signoff.json` |
| 文档实体只读投影到 Agent DB 并链接（S11-04，D12） | 通过：幂等（第二次不写入、摘要相同）、可重建、人的拒绝在重建后保留、规则够不到 `_oo*`；演示里澄海物流的合同文档链接到客户行 17。演示合同是 Markdown，**PDF 合同需要 sidecar 与模型参与，未验证** | `projection-demo.json`、`result-verify-signoff.json` |
| Cloud 代理模式（S12-01，OO 侧） | OO 侧通过：云入口第二监听、令牌与路由白名单、审计；**puppyone-cloud 转发控制 API 是另一仓库的工作，阶段二** | `result-verify-signoff.json` |
| Desktop 企业桶登记流程 | **阶段二** | — |

## 横切门禁（L7，OO 侧）

- 形式化：D81 取消了 FM3 多云事件；事件语义由 `events_live`、`prefix_sync`、适配器单测与 L7 脚本覆盖。快速门禁沿用 L4 的完整运行，见 `formal.md`。
- V2 审计：`audit`、`events_live`、`policy_live`、`projection_live` 等 `#[ignore]` live 目标没有持续门禁入口，由签收复跑手工执行；26 条新增 `secret-review.json` 条目为作者自审，需要非作者复核。

## owner 事项与已知缺口

- 提供真实 OSS、COS、R2 桶与最小权限凭证（只放文件、经 `secret_ref` 引用）后运行 `cloud_live`：`OO_CLOUD_BACKEND=oss|cos|r2|s3 OO_CLOUD_BUCKET=… OO_CLOUD_REGION=… OO_CLOUD_SECRET_REF=file:/绝对路径 OO_CLOUD_EVIDENCE=/tmp/… cargo test -p oo-connectors --features objectstore-oss,objectstore-cos --test cloud_live -- --ignored --test-threads=1`。
- 真实 SQS、R2 Queues、MNS 队列试验，COS 事件格式确认，MNS 签名确认。

## 历史记录（已被签收结论取代）

下面是签收前执行者的 L7 证据（`a7-l7rem`，41/41）。

# L7 evidence (30 section L7 and 7; B-S11-01 .. 04, B-S12-01, B-S12-02)

`tools/oo-test/super_ontology_l7.py` run `a7-l7rem` (after the L7 remainder): 41 of 41 checks pass (`l7-e2e-result.json`, `l7-e2e-log.txt`). It runs the
cargo suites below against a labelled PostgreSQL (`postgres@sha256:839fbb5a...`, `track_commit_timestamp=on`), then starts two real
`oo-docs-engine` processes under two real `oo serve` (a local engine A and a remote engine B, each with its own database, state
directory and Drive over the same files) and drives them with the real `oo` CLI.

## What could not be done: no real bucket

No OSS, COS or R2 account exists and none may be created. Nothing here claims a real cloud was reached. The "not confirmed" cells
of 10 4.3.7 are still not confirmed by a real store; what is proved is that the code does the right thing when a store answers the
way the clouds document it, against a look-alike (an in-process server, and with an ignored test a header injecting proxy in front of
MinIO or RustFS). `crates/oo-connectors/tests/cloud_live.rs` is the real bucket test, ready for the owner:

```
OO_CLOUD_BACKEND=oss|cos|r2|s3  OO_CLOUD_BUCKET=<bucket>  OO_CLOUD_REGION=<region>  [OO_CLOUD_ACCOUNT_ID=<r2 account>]
OO_CLOUD_SECRET_REF=file:/abs/path/credential.json      # {"access_key_id": "...", "secret_access_key": "..."}; never the key itself
OO_CLOUD_EVIDENCE=/tmp/cloud-live-<backend>.json         # what the store answered, no secrets
cargo test -p oo-connectors --features objectstore-oss,objectstore-cos --test cloud_live -- --ignored --test-threads=1
```

It writes only under `oo-cloud-live-<pid>/` and removes it. (`objectstore-oss` and `objectstore-cos` are aliases of `objectstore-s3`,
patch `B-S11-01-features.patch`.) RustFS and MinIO images could not be pulled (no route to the registry), so the existing
`s3_rustfs` suite was not rerun in this round; the `cloud_mimic_minio` ignored test was not run either.

## Results

| Card | What | Result |
|---|---|---|
| B-S11-01 | oss, cos, r2 as `objectstore-s3` Drives; compat table | 188 connector tests pass (compat 9, cloud_mimic 7, s3_mock 6, the rest of the lib). OSS endpoint `https://s3.oss-<region>.aliyuncs.com`, COS `https://cos.<region>.myqcloud.com`, R2 `https://<account>.r2.cloudflarestorage.com` with region `auto`; OSS and COS virtual-hosted only, R2 path style; a region or account id that is not a plain label is refused. ECMA CRC64 headers (`x-oss-hash-crc64ecma`, `x-cos-hash-crc64ecma`) verified, a corrupt transfer fetched again, three in a row end in `DOCS_SOURCE_CHECKSUM_MISMATCH`; an R2 like store reports no CRC64, no versions, a multipart ETag as a change signal only; a `Date` ten minutes off is reported as clock skew. `COMPAT_TABLE` says per row how it was established (`mimic`, real bucket pending) |
| B-S11-02 | event notifications and dedupe | 11 unit and property tests: 300 histories of puts and deletes, every event delivered late, out of order and partly twice, the view built from the `stat` answers equals the store; sequencer order (zero padded text compare); S3, R2, OSS formats; a local spool source. Over HTTP (`events_live`, 1 test) and on the real engines: a new file is published 5.2 s after the event with the poll set to 3600 s; a repeat is counted as a duplicate, an older event as stale, a delete event is only reported |
| B-S11-03 (remainder) | parallel listing per first level prefix | A Drive on an object store lists its first level prefixes four at a time (`config.listing.parallel` overrides, 1 lists whole; fs defaults to 1). Each prefix has its own row and generation counter in `_oo_docs.drive_sync_prefix` (migration 0149): the counter moves only when that prefix listed completely. A prefix that fails to list is not swept and does not hold the others: the complete prefixes are swept, the run is reported failed and the Drive generation stays uncommitted, so the retry lists again. A prefix whose directory has vanished lists as empty and its files are swept. `""` is the files directly under the root. Evidence: fs scopes partition the listing (2 unit tests), bounded parallelism (2), `prefix_sync` on PG (4: parallel with own counters, a failed prefix, whole listing at parallel 1, an S3 Drive on a real MinIO by default), S3 scopes on a real MinIO (`s3_rustfs` `scopes_partition_the_listing`, page sizes 1, 2, 100). Existing `ingest_fs` (15) and `conformance_sync` still pass; the formal trace conformance of a partial sweep is not extended (D81) |
| B-S11-02 (remainder) | pull adapters, COS format | `EventQueue` (receive, ack) and `pull()` give the same hints as the local spool. SQS (JSON protocol, SigV4), Cloudflare Queues (`messages/pull` and `ack`, bearer) and Alibaba MNS (batch receive and delete, HMAC-SHA1) were written against the public API descriptions and tested against fakes and one local socket only (11 tests): SigV4 matches the AWS signing key and get-vanilla vectors, SHA-1 and HMAC the RFC vectors; a message is acknowledged only after its hints were handled, an unreadable body is left for the queue, an SNS envelope is unwrapped, a redelivered message is absorbed by the deduper, SQS deletes in batches of ten. **None has been tried against the real service; the MNS signature in particular is unconfirmed.** COS: the Tencent documented notification format parsed from three synthetic fixtures (key `/appid/bucket/key`, time in Unix seconds, delete marker). Not confirmed against a real bucket |
| B-S11-03 | Drive policy | `policy_live` (2) and 7 unit tests: with `{"deny": ["Payroll"]}` on both engines the denied file is never listed, a mount of the whole Drive carries nothing of it; a targeted sync and an event for a denied path do nothing; a file that becomes denied is swept; the visible graph drops a denied path before any sweep |
| B-S11-04 | projection into the Agent DB, links | `projection_live` (2) and 3 unit tests: idempotent (second run writes nothing, same digest), rebuildable (drop the schema, same digest), a person's rejection survives rebuilds, the policy takes a denied contract out. Demonstration `projection-demo.json`: contract document of 澄海物流 is linked to customer row 17, the other document to row 19, 北辰云 (row 18) has no document and no link. The contract there is Markdown, see the test header |
| B-S12-01 | cloud tokens, audit | `audit` (2): the `oodc_` token is stored as a hash, refused on the loopback entry, the control token is refused on the cloud entry, a Drive outside the token's set and any other route are 403, revoking the token ends it; the audit answers who shared what to which agent and when (`audit-who-shared-what.json`: agent-7 through the cloud entry of tenant acme, scope, role, grantor, revoked at, reads of refs), holds no credential, cannot be updated, deleted or truncated (not even by the owner), and the hash chain verifies (a forged row is found) |
| B-S12-02 | `oo context mount --remote` | two stand-in tests and, on the real engines: the mount through engine B's cloud entry (with `--pack-digest` of the local mount) equals the local mount of the same engine byte for byte except `SHARE.json`; the token is in no output and no mounted file; a mount with another digest leaves nothing mounted and revokes its share |

## Notes for the coordinator

* Packs of two engines over identical files are not equal (a different Drive id and ontology history change the pack digest, and the
  AIGC label of every file embeds the digest). Equality is a property of one engine: its local pack and the same pack fetched
  through its cloud entry. `l7-e2e-result.json` lists the files that differ between engine A and B (informational).
* Patches in the notes directory: `B-S12-01-migration-docs-cloud-audit.patch` (0148 and its registration, needed by `audit` and the
  L7 script), `B-S11-04-migration-docs-projection.patch` (0147, a copy of `projection.sql`; `project` also creates it on first use),
  `B-S11-01-features.patch` (feature aliases), `B-L7-contract-docs-api-1.4.patch` (routes, schemas, changelog).
