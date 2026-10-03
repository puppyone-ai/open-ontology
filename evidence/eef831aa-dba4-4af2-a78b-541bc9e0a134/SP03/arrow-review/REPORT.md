# a134 fixed R2-S Arrow review

Verdict: REQUEST_CHANGES

Scope of this review: the R2-S bounded Arrow pull on fixed commit `75b993d0b578fa863a32c64abc0073ed92d51a5d`. Whole R1, whole R2, the reader, and the bounded TLS integration stay open. The a133 `init_mut` TLS-tail gate was not rerun and is not approved.

## Composition

This run compiled `git archive 75b993d0`. That commit's parent already contains panic-correction `79515fcf7e7032c8c212540eaae4b48a5957e3a2`, so the archived `buffered.rs` git blob is `ba2e7dc31b1e8bd133c3a4827f55255312dde522`. Content sha256 is `33d460f5005050a9cc3db6f90b022ef51b872e57a7552eec5103b31e10f0e1ba`.

The a131 author run built overlay on `ccd93e1adc34b24ab998d90972549f8cffb59f0d`, whose `buffered.rs` blob is `f41e0e4dd6baca062fb332ab66da3f5e9c0600fd`. The uncommitted a133 TLS tail in the worktree is blob `3ba2b87507096bab77464eaab7797712e4bf02cc`. The archive matches neither. Tree `a1250d01ae3c8ac2e8ee81ce29ae9ecfafe71966`. `stream.rs` blob `e7b5a6cc1714c67e818899788a09e0315b93b3ed`. `types/mod.rs` blob `46402e2e9c37064f7a81a3374d7fd5073c0ced58`.

The parent of `75b993d0` changes the twelve stream files and does not change vendor. The vendor difference from the author base is the already-accepted panic fix sitting between `ccd93e1` and `75b993d0`.

## Environment

Linux arm64, Rust `1.94.0` host `aarch64-unknown-linux-gnu`, image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL `17.10 (Debian 17.10-1.pgdg12+1)`, image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Private network `oo-a134-eef831aa-net`, alias `oo-a134-pg`, no published ports, cap-drop `ALL` plus the image's `CHOWN DAC_OVERRIDE FOWNER SETUID SETGID`, not privileged. TLS SAN `DNS:oo-a134-pg`, client `VerifyFull`. Database `oo_a134`. Reader role `oo_a131_reader` because the original tests filter that role name. Secrets stayed under `/tmp` and are not in this directory. `pointer.json` here has paths only.

Containers: smoke `02b497d0eeaea9c019d5bbd99245232dd7f50959c510dda1401dd4e107e0660d`, review `0a5c174ba5fb34c45533a717c708f0fb8bea87b93a03055a9fd81ce3d140daaa`, postgres `9a9fbd9c2f860aa4359b401351508b1cc7c363e3041e035f6cdc1d99f3b8178e`. Own cargo volume `oo-a134-eef831aa-cargo`, target `oo-a134-eef831aa-target`.

## Smoke, before the review overlay

`linux/smoke` on the archive, which contains `stream.rs` and `contracts/source/pgsql/empty-catalog.json` and does not contain `pg_stream_review.rs`. Empty-catalog content sha256 `044022fcabb7587632133cb8f81c3665292c32acc7af3432b5bfe4d066679a3c`.

| phase | exit |
| --- | --- |
| toolchain | 0 |
| snapshot | 0 |
| clippy-component | 0 |
| fetch | 0 |
| lib | 0, 57 passed, 0 failed, 0.01s |
| clippy `-p oo-connectors --all-targets -- -D warnings` | 0 |
| docker-run | 0 |

Elapsed 53s. Clippy printed two unfulfilled `dead_code` expectations in vendor `sqlx-postgres`. They did not fail the oo-connectors deny-warnings run.

## Original tests and the independent overlay

Functional builder joined the private network, offline, same cargo and target volumes. Product blobs were checked against the smoke archive before the tests. The only added file was `tests/pg_stream_review.rs` in the `/tmp` archive.

Two earlier review compiles failed in the overlay itself (`attempt1`, `attempt2`) and did not execute the review assertions. The decisive run is the third. Original tests were green on every run, after the live fixture's late `stream.parent` id 5 row was deleted between runs so the author's insert could run again.

| phase | exit |
| --- | --- |
| pg_stream_live | 0 |
| pg_stream_bounds | 0 |
| pg_stream_review | 101 |
| docker-run | 101 |

`pg_stream_live` on the decisive run: oracle before the writer was parent 4, child 3, empty 0, zero 2, payload 59. Measured executes 13, reserved 13, consumed 9, decoded payload 59, arrow preimage 132, arrow assembly 5528, wire view peak 8192. Held state `active` / `ClientRead`. Admin saw the late parent. Finish, abort, and drop released pids 202, 205, and 207. Reader `dml_ddl_commit=0`.

`pg_stream_bounds`: `max_value` on `bounds_wide`, `max_row` on `bounds_row`, displays are `source error: budget fact=...` with no row text. Slow consumer stayed `active` / `ClientRead`. Executes moved from 1 to 2 only after the next demand (`log_delta_after_demand=1`).

## Independent R2-S results that held

`rev` fixture, oracle taken on the reader as `REPEATABLE READ READ ONLY` before the writer.

One owner, generation 1, source `rev_main`, tables parent, child, empty, zerocol. Same `xact_start` `2026-10-03 16:07:41.487819+00` and `backend_xmin` 804 from the held parent batch through the child batches. Protocol `oo.source.pgsql.snapshot/v1`. `queue_batches` is 1.

Arrow values matched the pre-writer oracle, including the NULL label, the NULL child id, and the NULL child note. Parent plan key is the `id` column and each parent row has a key digest. Child plans have `key` false and no key digest. Empty table: zero batches, `TableEof` rows 0, columns 1, `zero_column` false. Zerocol: two presence rows, record batch has 0 columns, `TableEof.zero_column` true, scan SQL is `SELECT 1 FROM rev.zerocol`. Second `next_batch` after `Exhausted` is `Exhausted` again. Finish returned `SnapshotFinished` for the same pid, then the backends were gone. Reopen of generation 1 returned fact `generation_reused`.

Slow consumer: first parent batch left the backend `active` / `ClientRead` on `SELECT id,flag,label FROM rev.parent`. From 16:07:41.515 through the 400ms wait the reader log did not gain an Execute. The writer then inserted `rev.parent` id 99 and `rev.child` (99, late). The next reader lines are `execute fetch` on the same portal, then `ROLLBACK TO SAVEPOINT oo_stream_lock`, then `RELEASE SAVEPOINT oo_stream_lock`. Admin count of id 99 is 1. The oracle transaction did not see id 99. Parent `rows_emitted` is 3.

Accounting at exhaustion: executes 12, reserved_rows 12, consumed_rows 8, decoded_payload_bytes 45, oracle payload 45, arrow_preimage_bytes 143, arrow_assembly_bytes 8160, retained_rows 0, retained_batches 0, wire_view_capacity_peak 8192, wire_accepted_logical_payload 12461, wire_assembly_bytes 942939. Consumed 8 is parent 3 + child 3 + empty 0 + zerocol 2. Reserved 12 is those 8 plus one completing portal fetch per table that settled to zero rows. `decoded_payload_bytes` is accepted field payloads (int4 4, bool 1, text byte length, NULL 0, presence int4 4). Arrow sizes are `RecordBatch` preimage and `get_array_memory_size`, not RSS. Wire fields are connection `BytesMut` view peaks sampled while the portal is not borrowed. The message floor makes the view peak 8192 on this fixture. `wire_assembly_bytes` is the driver assembly counter, not backend heap.

Configured main budget: `batch_rows=1`, `batch_bytes=1048576`, `max_value_bytes=1024`, `max_row_bytes=8192`. Actual parent and child batches were one row. `queue_batches` is fixed at 1 and is not part of config identity.

`execute_control` and `begin_tracked` call `try_charge_rows(source, 1)` and do not refund on failure (`session.rs` 659 and 684). That charge is inside the source aggregate. This fixture never approaches `MAX_ROWS_PER_SEC` 50000, so the window was not exhausted here.

Batch-byte split on `rev.split`, budget rows 8, bytes 5, max value 64, max row 64. Values exactly `aa` then `bbbb`, two batches, first `retained_rows=1`, eof rows 2. Executes on the two batches were 2 then 3. The log shows one portal execute and two fetches, then `ROLLBACK TO` and `RELEASE`, so the third execute is the completion fetch in the same savepoint, not a second snapshot. Both values appear once.

Oversize `zzzzzz` (6 bytes) against `batch_bytes=5`: `Budget` fact `max_bytes`, display `source error: budget fact=max_bytes` with no row text. `finish` returned that budget error, not `SnapshotFinished`. Log order is `SAVEPOINT`, `SELECT body FROM rev.oversize`, `ROLLBACK TO`, `RELEASE`, then the transaction `ROLLBACK`. Backend 224 was gone.

Numeric primary key `rev.numkey`: the stream opened, payload Arrow int32 42, every plan `key` false, no key digest. A fresh owner at generation 2, `read_probe(Int8(1))`, returned fact `probe_key` and poisoned. `abort` returned generation 2 and the backend was gone. Full-mode key relaxation and the probe refusal are separate.

Schema: after `schema_a` `TableEof`, `ALTER TABLE rev.schema_b ADD COLUMN extra int4`. The next demand returned `Catalog` fact `schema_drift`. `finish` returned that error. The log shows the READ COMMITTED guard catalog query and then transaction `ROLLBACK`, with no `SELECT` of `rev.schema_b`.

Drop of an owner after one parent batch released pid 234.

Reader SQL: 1397 statements for `app=oo-pg-source` and `user=oo_a131_reader` across the whole server log. Zero of those statements are INSERT, UPDATE, DELETE, TRUNCATE, MERGE, COPY, CREATE, ALTER, DROP, GRANT, REVOKE, COMMENT, VACUUM, CLUSTER, REINDEX, CALL, DO, or COMMIT. Writer inserts are `oo-fixture-writer` and `oo-rev-writer`. `ALTER` and `REVOKE` are `oo-rev-admin`. The log has no password text. The overlay's `portal_order_bad` flag is a parser miss, recorded in FINDINGS, and is not a reader write.

## What failed

Post-open `REVOKE SELECT ON rev.acl_b FROM oo_a131_reader` does not surface as fact `select_not_granted`.

After `acl_a` reached `TableEof`, the revoke committed. The next `next_batch` returned `Privilege` fact `insufficient_privilege`. `finish` returned the same fact and not `SnapshotFinished`. No batch of `acl_b` was emitted. The server log at 16:07:43.552 is `ERROR: permission denied for table acl_b` / `STATEMENT: SELECT id FROM rev.acl_b`, SQLSTATE 42501, mapped by `error.rs` line 193.

Before that error the RR data session ran `has_column_privilege` (`sql.rs` `SNAPSHOT_ATTRIBUTES`) and continued into `SAVEPOINT oo_stream_lock`. The attribute probe returned select=true under the repeatable-read snapshot, so `snapshot.rs` `recheck` at lines 1424-1426 did not return `select_not_granted`. The READ COMMITTED guard's `observed_attrs_match` (`snapshot.rs` 1967-1984) compares attnum, name, type, typmod, and nullability, and has no privilege field, so `guard_still_current` also returned success. `stream.rs` 509-520 then opened the portal, and PostgreSQL rejected that statement.

The row did not leak and the run did not complete. The pre-scan privilege recheck the owner claims to perform did not observe the revoke. Callers that match fact `select_not_granted` will not see this failure.

Reproducer: open one stream on `rev.acl_a` then `rev.acl_b`, read `acl_a` through `TableEof`, `REVOKE SELECT` on `acl_b` from the reader, call `next_batch`. Observed fact is `insufficient_privilege`.

## types bridge

`75b993d0` changes `types/mod.rs` from `pub(crate) use read::guard_plans` to `pub(crate) use read::{compose_presence_scan, compose_scan, guard_plans, require_binary}`. `decode_payload` was already `pub(crate)`. The re-exports stay crate-private. Public mapping and canonical decode APIs are unchanged. The coordinator already admitted this narrow bridge as review input. The original process violation of landing it inside the product commit stays on the record. This review does not treat that hunk as a new functional defect.

## Out of scope, left open

No full type matrix, security matrix, license scan, dual-arch run, R3 cancel matrix, or 60000-row TLS tail. `init_mut` growth on the old driver remains the a133 integration gate. Do not mark the bounded reader or the TLS integration accepted from this run.

Own containers, volumes, network, and `/tmp` snapshot were removed at `2026-10-03T16:18:20Z`. The ids above are the ones that ran. `cleanup.txt` is the removal record.
