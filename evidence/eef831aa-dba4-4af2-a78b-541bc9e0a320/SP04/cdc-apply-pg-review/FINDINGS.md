# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a320`. Scope is the frozen CDC writer `tools/cdc-apply-pg` at `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96`. No new product unit was opened. No product file was edited.

Limited verdict: the fenced writer, on the fixed archive, passed the author suite and `linux/overlay`. Parent acceptance is false. SP04 stays open. CDC-D04 stays open. This review does not release the crate.

## Boundaries that held on linux/overlay

The green test log is `linux/overlay/test.stdout` (2 passed, 0 failed, 20.15s). Expected row text and the position decimal come from `overlay/a320_apply.rs`. The author helpers in `tests/common` are not called.

Installed versions present after `oo_pipeline::target_installer::install`: `physical-mapping/v1`, `target-bootstrap/1`, `target-fence-receipts/1`, `target-fence-receipts/3`. A bare database returned `capability_absent` with generation count 0. Drift was an `INSERT` of `target-fence-receipts/2` with checksum `cdcd` repeated to 64 hex digits, which is not the frozen v2 digest, into a template copy. That call returned `migration_drift`. Generation count stayed 0 on the copy and on the original. An unregistered `flag` column returned `mapping_absent`. The config digest stayed equal to the registered digest. The orders snapshot stayed `(320, 11, kept320, false, 7, 9)`.

Both physical relations were `relpersistence=p`. `pg_type.typname` was `int8`, `int8`, `text`, `bool`, `int2`, `int4` for orders and `int8`, `int8`, `int2`, `int4` for lines. The applied batch wrote qty `9223372036854775807`, left note `kept320` when the note cell was unavailable, set active `true`, n2 `-32768`, n4 `-2147483648`, deleted line 70, inserted line 90 with amount `-9223372036854775808`, n2 `32767`, n4 `2147483647`, and inserted order 321 with qty SQL NULL, note `row321`, active `false`, n2 SQL NULL, n4 `0`. Bool `::text` was `true` and `false`.

The receipt for `r320` stored fence `2`, payload digest equal to `digest_text(payload_preimage(recovered event hashes))`, applied position `18446744078004518944`, lineage `a320` repeated to 64 hex, codec `pg-commit-lsn/v1`, `cross_table_consistent` true, `partial_txn` false, `txn_complete` true, both segment fields null, and an empty bitmap. Exact replay returned `replay`, `row_writes` 0, and the same `commit_at`, payload, and fence. The same batch id with qty `4` returned exit 2, fault `payload_conflict`, sqlstate `OO010`. The stored receipt and the int8-max qty stayed.

A required insert of unavailable qty for order 322 returned `unavailable_insert` and stored neither the row nor a receipt. Library `apply` of a delete for missing line 91 returned `inserted`. Line 90 remained and line 91 stayed absent.

While each child was parked, `pg_stat_activity` for `oo_a320_runtime` on `oo_a320` had one backend, and the granted advisory lock pid was that backend. Before commit, the admin session saw no receipt and no row 400. After commit and before the response, it saw both. `Child::kill` was signal 9 in both windows. The pre-commit kill left the row and receipt absent and left the adopted generation in place. The resume inserted the row once. The post-commit kill was followed by a replay with the same receipt timestamp and fence.

`CDC_APPLY_INJECT=hide_commit` left one row 402. `CDC_APPLY_INJECT=hide_commit` with `CDC_APPLY_RECONCILE=down` exited 3 with kind `commit_unknown`, `target_applied` false, and `db_cross_table_consistent` false. The following normal apply left one row 403.

200001 events, one past `APPLY_LIMIT` 200000, returned `needs_segments` in 2579 ms. The password path did not exist afterward. A segment object returned `needs_segments` on the same absent path. Declared `numeric`, `float8`, `jsonb`, and bare `int` returned `unsupported_type` before credentials. An int cell declared against the text column returned `type_mismatch`.

The receipt lineage pattern `^[a-z][a-z0-9_.:-]{0,127}$` is in the installed function definition, the installed check constraint, and `src/model.rs`. Intersected with `^[0-9a-f]{64}$`, a 64-hex lineage must start with `a` through `f`. The live lineage starts with `a` and was stored. A 64-hex lineage starting with `0`, a non-hex lineage starting with `g`, and the receipt-legal token `billing` returned `identity` before connect. A 64-hex lineage starting with `f` passed that check and then returned `spool_io` for a missing spool.

Output flags from `closed` stayed false: `ack_allowed`, `source_cross_table_consistent`, `upstream_commit_lsn_verified`, `debezium_normalizer`, `p2_verified`, `p11_verified`, `control_lease_consulted`. Provenance stayed `caller_declared`. `db_cross_table_consistent` was true on the inserted batch and on exact replay. The scanned writer sources do not contain `XACK`, `pg_advisory_lock`, `GRANT `, or `TRIM `.

## Adopted generation

`adopt_target_generation` is its own statement before `BEGIN`. The author FINDINGS item 1 says the live test does not query `generation_count` again after OO010 and the row failure. This overlay did.

The first adopt, on the injected row failure, inserted count 1 and generation 1. The rollback removed neither. The receipt stayed absent and qty stayed 11. Each later connection that reached adopt advanced `target_generations.generation` by one and left the row count at 1. Printed values: applied fence 2 with stored generation 2; replay stored generation 3 with receipt fence still 2; OO010 stored generation 4 with receipt fence still 2. The holder adopt printed generation 7, and the blocked `55P03` call left that value. The pre-commit SIGKILL printed generation 8 still present after signal 9, with the row and receipt absent. The post-commit receipt fence was 10. The settled row 403 receipt fence was 14. Refused applies that reached adopt also advanced the value. The transaction rollback does not roll the generation back.

## sqlx and OO013

The stale probe on the holding connection returned `DatabaseError` code `OO013`. `Display` of that error did not contain `OO013`. The overlay reads `DatabaseError::code()`. This matches author FINDINGS item 6 and is a client-display fact, not a writer defect.

## a299 sentence

Author REPORT, FINDINGS item 4, HANDOFF, and `CONTRACT.md` lines 229–230 say a299 spool approval is still pending. The a299 review at `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65` already recorded a limited spool verdict. That sentence is documentation lag. This review did not open a spool review and does not widen the a299 verdict.

## Registry

All 284 `checksum` entries in `tools/cdc-apply-pg/Cargo.lock` are present in the union of the root lock, `tools/cdc-spool/Cargo.lock`, and `tools/session-store/Cargo.lock` from the fixed archive. Both runs printed `lock_subset_ok tool_checksums=284`. The lock SHA-256 stayed `67d47b32d081d28504a3d0fe0fcb431c49c618dd1f5599fa188d6f8878e1e78f`. `cargo update` and `cargo generate-lockfile` were not run. Core and migration hashes were unchanged. a313 remains the root dependency writer.

## Still parent-required

ACK, XACK, TRIM, and a control-database mirror are not in this writer. Segmented apply of 500000 events is not this child. Source and WAL cross-table certification stay false. A Debezium normalizer is not in this crate. Router, segments, customer delivery, and release stay parent-required. Full SP04 and the CDC-D04 parent goal stay open.
