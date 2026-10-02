# FINDINGS — R2 wire allocation path

Status: read-only analysis. This file does not approve R1, does not authorize an R2 patch, and does not deliver a reader.

Fixed product input: commit `2b6dd57a0dc3d36e098268638c0364af247d07fb`, tree `9c75b25cf7e539cc043c7c90ee750aad9a2c3ccc`, parent `e5a066622e0409762fe44f11c4d303d2468a3290`. Product blobs below were read from that tree. At analysis time `HEAD` was `3963fc2b0ece54cabe62071153918b2df815b1c7` (coordination commits after the fixed input). `git diff --stat 2b6dd57 HEAD -- crates docs contracts Cargo.toml Cargo.lock` was empty. No `cargo`, `rustc`, builder, container, or database was run. No credential, cache, argv, or environment capture.

## 1. The inbound frame is allocated before any value check

PostgreSQL frontend/backend messages are one type byte plus an Int32 length. The length counts itself and excludes the type byte. `PgStream::recv_unchecked` (`crates/oo-connectors/vendor/sqlx-postgres/src/connection/stream.rs` lines 79–118) reads 5 header bytes, takes `message_len` as `u32 as usize` (line 92), computes `expected_len = message_len + 1` (lines 94–99), and if the buffer is short returns `ControlFlow::Continue(expected_len)` (lines 101–102).

`BufferedSocket::try_read` (`crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs` lines 69–80) passes that length to `ReadBuffer::read` (lines 278–300). `ReadBuffer::reserve` (lines 303–307) calls `BytesMut::reserve` for the missing bytes before the socket read. The only overflow check in `recv_unchecked` is `message_len + 1` against `usize`. There is no configured maximum.

`DataRow::decode_body` (`vendor/sqlx-postgres/src/message/data_row.rs` lines 31–84) runs only after `split_to(message_len).freeze()` (stream.rs line 111). It reads a `u16` column count (line 39) and, per column, an `i32` length (line 63). `-1` is NULL and has no payload (lines 72–76). A positive length is a range into storage that already exists. The range vector is `Vec::with_capacity(cnt)` (line 41). That vector is bounded by the `u16` count. The payload allocation is not.

`PgConnection::run` (`vendor/sqlx-postgres/src/connection/executor.rs` lines 243–271) binds the unnamed portal in binary format, sends `Execute` with `limit: 0` (lines 254–259), then `Close` of that unnamed portal, then `Sync`. The comment at lines 256–258 records that a non-zero Execute limit disables parallel workers (SQLx issue 3673). Limit `0` is the protocol's "no row limit" (`message/execute.rs` lines 12–14). The read loop (executor.rs lines 295–356) yields one `PgRow` per `DataRow`, and each `DataRow` still contains the whole frame in `storage: Bytes` (`row.rs` lines 12–16, `data_row.rs` lines 10–16).

`sqlx_core::executor::Executor::fetch_all` (`vendor/sqlx-core/src/executor.rs` lines 123–131) is `fetch(query).try_collect()`. `oo-connectors` uses that path:

- `session::fetch_all` (`src/session.rs` lines 489–505) applies `within_rows` after `query.fetch_all` returns the `Vec`.
- `types/read.rs` `fetch_limit` (lines 333–357) does the same, then compares `rows.len()` with `budget.max_rows()`.
- `PgSession::read_typed` (lines 200–249) fetches with `LIMIT $1` (`compose_select`, line 87), then `charge_bytes` on `raw.as_bytes()` (lines 242–246).
- `SnapshotOwner::read_probe` (`src/snapshot.rs` lines 447–517 and `decode_row` lines 1156–1185) calls `session::fetch_all` and then `charge` on `bytes.len()`.

`as_bytes` (`vendor/sqlx-postgres/src/value.rs` lines 64–68) borrows the frame. `PgValueRef::to_owned` (lines 101–107) copies if the row `Bytes` is absent. The size check in `decode_row` and `charge_bytes` (`types/read.rs` lines 360–368, `snapshot.rs` lines 1380–1388) therefore runs after the frame and, for `fetch_all`, after every row of the result is stored.

`PgBudget` (`src/config.rs` lines 11–13 and 217–278) is the inspection budget: timeouts at most 120 seconds, `max_rows` at most 100000, `max_bytes` at most 32 MiB. `max_bytes` is documented on the getter as UTF-8 bytes retained from names and evidence. It is not a wire-frame cap. `SESSION_GUARDS` (`src/sql.rs` lines 6–10) sets `statement_timeout`, `lock_timeout`, `idle_in_transaction_session_timeout` (same value as `statement_timeout`), and `default_transaction_read_only=on`.

`wait_until_ready` (`vendor/sqlx-postgres/src/connection/mod.rs` lines 94–105) drains until `ReadyForQuery` by calling `recv`. Each skipped `DataRow` is still fully buffered, then dropped. `Connection::close_hard` (lines 174–177) shuts the socket down without a `Terminate` message.

`PG_COPY_MAX_DATA_LEN` (`vendor/sqlx-postgres/src/copy.rs` lines 132–133) is `(1 GiB - 1) - 1 - 4`. It chunks outbound `COPY FROM STDIN`. It is not consulted by `recv_unchecked`.

`PgConnectOptions` exposes `statement_cache_capacity` and the explicit-options constructor. It has no incoming-message setter. PATCH.md hunks are private CA and `from_explicit` only.

Conclusion: the vendored driver does not expose an enforceable preallocation limit. A caller that stops at `fetch`, `fetch_many`, or a SQL `LIMIT` still lets the server choose the `DataRow` length, and the client reserves that length first.

## 2. What the frame actually contains

One `DataRow` is one result row. Column payloads sit in that single message: 2 bytes of count, then for each column 4 bytes of length and the value bytes. Many small columns allocate as one sum. A toasted value is not a toast pointer on this path.

Upstream PostgreSQL `printtup` (postgres/postgres `src/backend/access/common/printtup.c`, master view retrieved 2026-10-03, not a vendored 17.10 tree) calls `slot_getallattrs`, then `pq_beginmessage_reuse` for `DataRow`, then for binary output `SendFunctionCall` and `pq_sendint32` of `VARSIZE(outputbytes) - VARHDRSZ` plus `pq_sendbytes`. Detoast happens before those bytes are appended. `MaxAllocSize` in `src/include/utils/memutils.h` on that same view is `(Size) 0x3fffffff` (1 GiB - 1). `PQ_LARGE_MESSAGE_LIMIT` in `src/include/libpq/libpq.h` is `MaxAllocSize - 1` and applies to messages the backend is willing to read, which is the other direction. The tested server string in this crate is `17.10` (`src/lib.rs` `TESTED_VERSIONS`). Those upstream line numbers are from master (`PqMsg_DataRow`), so this finding does not claim a 17.10 line binding. The wire shape the client implements is the one in `data_row.rs`.

A single-row `LIMIT` fixes how many `DataRow` messages the simple-query or unbounded Execute will emit. It does not fix the Int32 length inside the one message. `read_probe` uses `LIMIT 2` (`snapshot.rs` `bind_probe`, the bound `2_i64` at line 1109) and then rejects `rows.len() > 1` (`decode_rows` lines 1138–1139). That is a point probe. The contract says so (`contracts/source/pgsql/snapshot-owner-v1.md` lines 35–44 and 63–65).

## 3. Allocator growth on the accepted path

`ReadBuffer` starts `available` at `DEFAULT_BUF_SIZE` 8192 (`buffered.rs` lines 10 and 41–44). `Socket::read` (`vendor/sqlx-core/src/net/socket/mod.rs` lines 65–80) fills while `has_remaining_mut` is true, so one syscall can fill the reserved capacity, including bytes past the current message when `BytesMut::reserve` rounds up.

`Cargo.lock` at the fixed commit pins `bytes` 1.12.1, checksum `fc652a48c352aef3ea3aed32080501cf3ef6ed5da78602a020c991775b0aff04`. The bytes crate is not vendored here. The docs.rs source view retrieved 2026-10-03 for `BytesMut::reserve` (`bytes_mut.rs` around lines 590–663) returns immediately when `additional <= capacity - len`, and otherwise `reserve_inner` rebuilds the `KIND_VEC` and calls `Vec::reserve(additional)`. `Vec::reserve` may allocate more than `additional`. A high-water number that restates the configured max, without reading `capacity()` after this call, does not measure the allocation.

Kernel socket memory is outside that `BytesMut`. Process RSS does not include the whole receive buffer. A client bound is a statement about the process heap path above, not about backend `palloc` of the detoasted datum.

## 4. Copies after the frame

`decode_payload` (`src/types/binary.rs` lines 20–28) borrows the frame and then owns the decoded form.

- `decode_text` (lines 111–117) allocates two `String`s (logical and `ArrowScalar::Utf8`).
- `decode_bytes` (lines 120–128) allocates two `Vec<u8>`s.
- `decode_jsonb` (lines 143–155) parses a tree and renders a new string.
- `decode_array` (lines 457–535) rejects `count > 100_000` at line 487 only after the array payload is already in the frame. The element cap bounds the decoded `Vec`, not the earlier reserve.

`assemble_batch` (`src/types/batch.rs` lines 150–246) stores `cells: row.clone()` (line 228) and pushes `arrow_of(cell)` into column vectors. `record_batch` (`src/types/arrow_batch.rs` lines 70–83) clones UTF-8 and binary values again into Arrow 60 arrays (`arrow-array` `=60.0.0`). The canonical hash is `type-policy/v1` over logical fields (`batch.rs` lines 210–229), not over Arrow buffers. `type-conversion-v1.md` freezes that split.

Holding one `PgRow` plus one `TypedBatch` therefore keeps the frame and about two owned copies, then a third copy inside `record_batch`. A batch queue multiplies the batch, not the frame, if the frame is dropped at the end of the row.

## 5. Catalog, type plan, and the no-key gate

`SnapshotOwner::open` (`snapshot.rs` lines 378–437) reserves the generation, connects once, and runs `BEGIN` as `START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY` (`sql.rs` line 101) on that session. It does not call `PgSession::catalog` or `PgSession::read_typed`. `plan_columns` (lines 899–915) calls `load_type_graph`, `map_column`, and `guard_plans`. Those plans stay on `BoundTable`. `types/mod.rs` lines 29–33 re-export `decode_payload` and `guard_plans` for the owner. Mapping semantics stay in `binary.rs` and `mapping.rs`.

`selection_matches` (lines 649–664) returns `probe_key` when `primary_key` is empty or longer than 8. `key_oids` (lines 918–932) returns `probe_key` when the oid list is empty or a key oid is outside bool/int2/int4/int8/text/varchar/bpchar. `quote_table` (lines 962–964) returns `probe_key` when no lossless column is selected. An owner for a table with no primary key is not returned today. The snapshot contract (`snapshot-owner-v1.md` lines 27 and 63–65) says the same: R1 key shapes are that set, and `read_stream` is not implemented.

`config_identity` (lines 295–328) hashes source id, endpoint, username, secret reference, the five `PgBudget` fields, and selected names. The password and CA PEM are absent. Adding stream knobs to `PgBudget` changes `oo-pg-config-identity/v1`.

`map_sqlx` (`src/error.rs` lines 177–213) keeps kind, a static fact, and a five-character SQLSTATE. `Error::Protocol` becomes fact `protocol`. `err_protocol!` (`vendor/sqlx-core/src/error.rs` lines 338–351) appends `module_path` and line to the string, so a prefix match on driver text is not a stable fact. `Error` is `#[non_exhaustive]` (line 31). Display of `SourceError` is the closed form in lines 137–147.

## 6. Backpressure that already exists, and the hole it leaves

If the application stops polling `run`'s stream, `recv_unchecked` stops asking for the next message. TCP window then stalls the backend in send. That bounds how many small frames the client holds, down to the 8192-byte buffer plus whatever `reserve` already rounded up, plus one in-flight message. It does not bound the first message whose length prefix is larger than any batch budget. The stock `run` also pipelines `Close` of the portal before the client has read a row, so the client cannot leave a named portal suspended at `ReadyForQuery` between pages. The next `wait_until_ready` will drain the rest through `recv`.

`read_typed` and `read_probe` collect before they charge. A slow consumer of those APIs does not exist: the `Vec` is already complete.

## 7. Input hashes

SHA-256 is of `git show 2b6dd57:path`. The middle column is the git blob id.

| SHA-256 | blob | path |
|---|---|---|
| `185f39afdd96284a7dd1f40fff1a9292fd289c85c2106c65a121045b551c00c5` | `e8a1aedc47f8de5169f0d663ff271b6f2e590ff6` | `vendor/sqlx-postgres/src/connection/stream.rs` |
| `721ead17743d912c625a56b5728669d8111c750797b6fee24e234226a5b6fbe6` | `e0f4c3d44a6c81556822796bd2739576a9ea7fdf` | `vendor/sqlx-postgres/src/connection/executor.rs` |
| `4ed9cbb4dd4fa68c6104b1e626178994ddd5e2708c7423cc137c01ea51e2829e` | `6785e70879f6111ca00a243c2e94fe2f7269f703` | `vendor/sqlx-core/src/net/socket/buffered.rs` |
| `50a13a82c07ff5cd3734cb5bbbc69eb51e85a4960a105ae357f16308a388be47` | `ae9d0d9b26ac1f7357ff41f96f46d60d4ad1d8c6` | `vendor/sqlx-postgres/src/message/data_row.rs` |
| `8be84536b93e50775654c8e7207ca144e1b78d2c12953bb43eec72690da3f0d9` | `dedfe7c1bb4e1a53555189ac2d5693d3627cbb02` | `vendor/sqlx-postgres/src/message/mod.rs` |
| `779ea6f980d56cde11b50ff37a72a96eab3837c1c1a2fe28caa73a0fac7180c2` | `f82b7884bceb950b00ec721346bf1be81865a8cc` | `vendor/sqlx-postgres/src/message/execute.rs` |
| `a65585b8c6e585f9429e3af7867bdeca3d1a2363760b452df785eb12a60edadf` | `5240a50be0533255d713b2f6c3861bf46bc9de10` | `vendor/sqlx-postgres/src/row.rs` |
| `80f9303a892fb97ac0a4084532bc03e0a344d881495cad1af2d2357150cde0c6` | `90c015bb0e9de9bf4e663393814ce351bd00ba39` | `vendor/sqlx-postgres/src/value.rs` |
| `06a2bd576ae6659023380d973c4bb7c60b659436a7441f4725218a6beb76c689` | `00c8c984464e75933e81970bc5144537255300b9` | `vendor/sqlx-postgres/src/copy.rs` |
| `efeafd9bc07c0bd20e0d2c2312bd2db45039c9d139aa754b1ff6574dd280de72` | `d594585b6cb2c8b4a30f4b31385fe8b82716c6b0` | `vendor/sqlx-postgres/src/connection/mod.rs` |
| `bd4427048da65cb279cfa900173a1d40b9d9154c049467f9f511cac63a05a651` | `e146c7a895c17e9c0e8067608d69b6296dfcf65e` | `vendor/sqlx-core/src/executor.rs` |
| `c3953bfa7ea84b879a544e71069ef6db6a9da756a2c1f13e8362485a15556ec1` | `8c6f424cdfc45372d8474a8d26a32d4373df171a` | `vendor/sqlx-core/src/error.rs` |
| `39f602dff805742661b5c6915a0634f86718443b8d5ca3f1e2f2cff53b5e206c` | `687c62b13f199bddbff5a45b3b2e73ee8ba3503e` | `vendor/PATCH.md` |
| `86361bc3254edfdc67f4fa70e2a304025da2298ecc76c86b71dd7e65b846e088` | `53aa65f239497d5cad451a2c518669a4e6ec770c` | `src/snapshot.rs` |
| `fc092922b73aae14624b773b69c6da79ebfc2f307c2bb06ddd62493850a46899` | `131a101c03306e163e5c82bbc12e85de087e9b94` | `src/types/read.rs` |
| `7883bdaa853dc2db82bee2ea1d334bf537bb7bcc1db45331989671e57ff5d2fe` | `5fb73a64875a0570125c23f4545fd10b639ee236` | `src/types/batch.rs` |
| `35fb94e82eceba79442c6816866706b60b6323bc0ac6aef70b878a360045b6ae` | `f0c506fb48eadd913991d73eb9e6e9ade294e241` | `src/types/binary.rs` |
| `a281e0479536eedeb4b3f94b45b6ce845aeb5adf2be1afe2ba6ddd1f96e8b09f` | `df9ca78cd5f6d588c7997e90bbb86dbd14c213a7` | `src/types/arrow_batch.rs` |
| `86fa06414a49e6b97425651f0ffc750bc634fe5423d1af70216b18da03be4b25` | `32c2541d7e63687000738521fa1c225557db9221` | `src/types/mod.rs` |
| `ad3a169c81bb5eb1b9d47715dd4f4f54e64382578f1f80ca231603454a9cfea9` | `99b0bf100040e350eda437f49925593bb1743249` | `src/config.rs` |
| `49b19e22888247ce40e5972ccdd95136a6e6af9757fe86fae87b30b1699a4896` | `16143c3f809af5f73810d49fa2051a92bef14472` | `src/error.rs` |
| `9434b059e24dd7ae9f113fa2e7572ed78f274144821d7fa86345e6e638e227ae` | `07febf17b415abcb785644f60690c47939e0e50f` | `src/sql.rs` |
| `70c16cea3aa12af9d09981714816f03522dcfdba4855116892c271d58c535e10` | `5211832e3f9b6b297e6db21cfcd0e72d920d4704` | `src/session.rs` |
| `460ae2460ace386f54a715a626de5a73d1ba9a9104aa6cf656ff5dfab0abab06` | `6bba418242225640a620792f30234dba10b6e3d2` | `src/lib.rs` |
| `347f21f56657900c99939d505b72d1e60e8c5a2b7c60177e0d6ff8e009f3f2cf` | `e8e3254da87a29ce90c485e8ad6566dbd4654f43` | `crates/oo-connectors/Cargo.toml` |
| `095dd3b364d8284ec4a665a9125bfe0bfdff3b62736d461ae61a4fb271e571e0` | `6306ae1cc3c5ff84809c9150212a66844986ba9a` | `contracts/source/pgsql/snapshot-owner-v1.md` |
| `c89aafc6859e079febef5e17693095cfc376cdd46578a0ab574abfcbf05fe25d` | `a86eb4595a3d71743eb2a7fd81e9e48dcfcf20ae` | `contracts/source/pgsql/type-conversion-v1.md` |
| `0504cfb524ce658814a1a5c89375a508bb37c5fb1707713b44f9126088e59cfd` | `23b7323834e8723a359c27e61d490c5f0a69d41c` | `execution/prompts/v01-pg-bounded-reader-prepared.md` |
| `9266f39e157d0d5690d3c61aa121a6bfca5dfdabb07c0a691e35293cd5c37eb2` | `8172f671502b842ad26510fe39e09c30153ac7b6` | `execution/decisions/a110-functional-development-gate.md` |
| `91648eee551c4b89631a82e35da3e323aa4a75eb39f9289acdbc4a8e56bc4e2d` | `22d3818d5ed3346de059239be4319290c7b78957` | `docs/03-sources-and-ingestion.md` |
| `d186f1743da9f24538fd5cf548383be772266c9fd5a5110f52ce7b95dfa052c3` | `9697f6c0c43f0b9e6c7f4926fbd0eccf2b81def1` | `docs/04-pipeline.md` |
| `f16f770e29dc02537a71c734b9355e21812ef65a2fc60880e22543a95bf29534` | `9852f30b40310607ada2e1549be40099a55c0ac2` | `docs/17-rust-implementation.md` |
| `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f` | `15ae1071c7236a956547e3f1d37d627b2cbebfce` | `Cargo.lock` |
| `a277789df0b8c74febf113a2acc80a0a9f4b51bdeaa662e5d4f761be2489046b` | `c78ee5d0575a11cd6078cf0055cdd2c673edf583` | `Cargo.toml` |

Lock pins beside the patched trees: `sqlx` 0.9.0 checksum `378620ccc25c62c89d8be1c819e76a88d59bdcc3304733330788948e619bfd71`; `arrow-array` 60.0.0 checksum `51dff6e4b9f158864a0aeb6a131ad53858ecbae7a2fc8989307b9b3c6b2f122e`. `[patch.crates-io]` points `sqlx-core` and `sqlx-postgres` at `crates/oo-connectors/vendor`. PATCH.md records crates.io tarball checksums `05b44e85bf579a8eeb4ceaa77a3a523baf2bf0e9bac7e40f405d537b5d2d5ccb` and `87a2bdd6e83f6b3ea525ca9fee568030508b58355a43d0b2c1674d5f79dcd65e`. `futures-core` 0.3.34 checksum `92d699e522242e69e3003b94ecc1f960f3a5e015aa7c5d7486e65ad01dd94f5e` is already in the lock and is not a direct `oo-connectors` dependency.
