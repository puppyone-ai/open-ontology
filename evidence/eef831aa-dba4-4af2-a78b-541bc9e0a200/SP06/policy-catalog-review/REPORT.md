# Policy catalog review

Verdict: LIMITED.

Session `eef831aa-dba4-4af2-a78b-541bc9e0a200`. Reviewed source `78a60fdeabcada0d024fc6f9a81e0e6b184260e8`. Author evidence `9d658725fabfbf38694227bf462ba23d9c4d39cb`. The product tree, Cargo.lock, manifests, and the ledger were left unchanged.

This review ran the fixed gateway, including `query_executor.rs` as it is in `78a60fde`. The a190 proof compiled archive `3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6` and then overlaid the new catalog files. Its FINDINGS.md line 39 says that executor binds the role after `guard_select` and that `bound_role()` is empty after a guard refusal. That description matches the old executor. It does not match `78a60fde`.

## What was reviewed

Public installer `install_column_acl`, loader `load_policy_catalog`, guard `guard_select` through `DedicatedSession::execute_trusted_select`. Fixture database `oo_a200` on host `oo-a200-pg`. Source `depot`, namespace `warehouse`, table `parcel`. Config digest is `a200` repeated to 64 hex digits. A second digest is `b200` repeated. Generations are 3, 9, and 14. Roles are `oo_r_picker` and `oo_r_sorter`. Picker's policy order is sealed, note, units, serial, weight. Physical `attnum` order is units int2, weight int4, serial int8, note text, sealed bool, plus hidden token text and `_oo_pos`. Rows are the three warehouse parcels, including int2 minimum, int8 minimum, a null note, and both boolean values. Token values were inserted and were not selected.

Grantor was fixture superuser `oo_fixture_admin`. The login was `oo_a200_login`. TLS verified the private CA. The certificate SAN is DNS `oo-a200-pg`. No host port was published.

## Pre-overlay smoke

Linux arm64, Rust 1.94.0 (`4a4ef493e 2026-03-02`), image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Container `oo-a200-smoke`, 8 GiB, all capabilities dropped, bridge network `oo-a200-eef831aa-net`, private cargo and target volumes. Libclang came from Debian packages extracted onto the cargo volume. `libclang-19.so.19` resolved `libLLVM.so.19.1` from that root. `stdbool.h` was present.

`cargo test --locked --offline -p oo-gateway --lib` ran 15 tests, all passed. `cargo clippy --locked --offline -p oo-gateway --lib -- -D warnings` passed. The review file was absent from the archive. `policy_catalog.rs` has no `unsafe` token. Cargo.lock stayed `1e9b9377cb2bdf951ab22f7f0a4b68649cb70e563f6ef13adad5852e1c72c657`. `cargo update` was not run. Phase exits are under `linux/smoke/phases/`. Docker exit 0. The container was removed.

At smoke start, one other labeled rust builder was running. This task had none. Global count was 1, under the cap of 6.

## Live proof

PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, linux/arm64, container `oo-a200-pg`, 1 GiB, capabilities dropped except CHOWN, DAC_OVERRIDE, FOWNER, SETUID, and SETGID. Autovacuum off, track_counts on. Secrets stayed under `/tmp` and were not copied into evidence.

The proof builder was `oo-a200-proof`, started only after the smoke container was gone. It mounted a private copy of the archive. The overlay `policy_catalog_a200_review.rs` was copied only into that copy. The git archive still has no review file (`linux/proof/archive-overlay.txt`). Product hashes matched the smoke hashes (`product-match` exit 0). The test `policy_catalog_review_on_a_different_fixture` passed in 1.43s with `OO_A200_LIVE=1`. Lock hash unchanged. Docker exit 0.

Failed attempts kept:

- Attempt 1, exit 125. Docker could not create a file mount for the overlay on the read-only archive. The archive was not modified.
- Attempt 2, exit 101. The overlay did not compile: ten borrow errors in the harness.
- Attempt 3, exit 101. The fixture started, then the harness read `xact_start` from the same backend that was running the probe.

The successful oracle is `live/oracle.txt`.

## Measured behavior

Generation is the maximum `run_generation` for the same source and config digest. Digest `b200` at generation 14 was registered before the `a200` policies were installed. Generation 9 on `a200` still loaded. Generation 3 and generation 14 on `a200` returned `StaleGeneration`. Generation 9 on `b200` returned `StaleGeneration`. Generation 14 on `b200` passed the generation check and returned `MappingAbsent` because that digest has mappings and no shadow table. The SQL is `policy_catalog.rs` lines 450-451 and `policy.rs` lines 600-601.

`reltuples` started at the -1 sentinel. `UnknownRows::Refuse` returned `UnknownRows`. `Zero` stored 0. `Saturate` stored `u64::MAX`, and an unfiltered star then returned `GuardError::CostTooHigh` with the large-table threshold at 10. `reltuples` 2.5 loaded as 3. Values -2, NaN, and infinity returned `EstimateUnusable`. Setting -1 again restored Refuse. After `ANALYZE`, both roles reported `estimated_rows` 3.

Picker's catalog order was units, weight, serial, note, sealed. That is `attnum` order. Pending unclassified was 2 for picker and 6 for sorter. Type OIDs read back from `pg_type` were int2 21, int4 23, int8 20, text 25, bool 16. Each visible column OID matched `pg_attribute.atttypid` and the base type. The loader returned 54 comparison operators. Each OID and procedure OID matched an independent `pg_operator` lookup for the nine contract signatures. `count(*)` was OID 2803. `pg_sleep`, `count(any)`, int4 `+`, and both int2/int8 directions were absent from the loaded catalog. Integer literals used the int4 OID.

Both roles' guarded reads matched an administrator query of the published physical columns. The star returned 3 rows. `count(*)`, comparisons, null, text, bool, int2, int4, and int8 values matched the inserted rows. The successful star recorded `CompletionStatus::Succeeded`, rollback confirmed, socket kept, and the in-memory audit hook accepted the completion. Sorter `SELECT units` was `ColumnNotAllowed` and the display text did not contain `units`.

`GRANT SELECT` on token left it out of the catalog. Pending stayed 2. The guard refused `SELECT token` without the column name. `REVOKE` made the direct login statement SQLSTATE 42501. Revoking sealed moved pending from 2 to 3 and omitted the name. A catalog loaded before that revoke, used after the revoke, returned `ExecError::Database` SQLSTATE 42501. That completion had rollback confirmed and `is_open` false. Adding column `extra` raised pending to 3, omitted the name, and the direct select and insert returned 42501. `pg_sleep(0)`, `count(weight)`, and `weight + 1` were `FunctionNotAllowed`. `INSERT` was `GuardError::Write`.

`max_columns` 6 with 7 attributes returned `LimitExceeded`. `max_columns` 7 loaded five visible columns. `max_operators` 53 returned `InvalidLimit` before a snapshot, and the admin client was idle afterward. Empty input, mixed picker and sorter policies, and a sorter identity on a picker policy failed before use. Missing role `oo_r_ghost` returned `MissingRole`. Policy column `ghostcol` returned `MappingAbsent` and the display text did not contain `ghostcol`. A domain over the published note column returned `UnsupportedType` without the name `note` or `a200_note`.

Registering the same table identity again returned `replay`. A conflicting digest returned SQLSTATE `OO022`. Mapping row count did not change. The final user row count was 3. Loader windows reported seq_scan, idx_scan, and tuple insert, update, and delete deltas of 0.

A second admin session watched the loader client. An open `REPEATABLE READ READ ONLY` transaction had `pg_current_xact_id_if_assigned()` null and `xact_start` set. After `ROLLBACK`, and after every loader return checked by the harness, the observer saw `xact_start` null and `search_path` was not `pg_catalog`.

On a fresh dedicated session, `bound_role()` started empty. A guard refusal of `SELECT *` on the saturated catalog left `bound_role()` as `oo_r_picker`. A following insert was `GuardError::Write` and the binding stayed. A sorter statement on that session returned `RoleReuse`. A new sorter session then ran its own reads. The bind is `query_executor.rs` lines 628-642, before `guard_select` on line 642. The author test requires the opposite at `policy_catalog_pg_live.rs` line 575.

## Limits

`ColumnAclPolicy.generation` at `policy.rs` lines 91-92 says the generation is the maximum stored for the project. The installer and the loader both filter on source and digest. The live cross-digest case followed the SQL.

`ensure_committed` at `policy_catalog.rs` lines 1077-1090 checks only that `pg_current_xact_id_if_assigned()` is null. That predicate is also true inside an open read-only repeatable-read transaction that has not assigned a transaction id. The client returned by this loader was idle when a second session looked. The xid check alone does not show `xact_start`.

`AmbiguousMapping` was not forced. The primary key of `_oo.physical_mappings` makes two rows for the loader's lookup unreachable through `register_physical_mapping`. Replay and `OO022` were observed.

A database 42501 through the executor drops the dedicated client after a confirmed rollback (`query_executor.rs` lines 975-980 and 1277-1303). The catalog admin client is a different connection and was returned idle.

The audit hook was in memory. This run does not speak to WAL durability, authentication, row-level security, pool reuse, or a non-superuser grantor. SP06-W01 and SP06-W02 stay open. No release is claimed.

## Attempts and resources

Smoke and the successful proof each used one rust builder, never at the same time. One private PostgreSQL 17.10 ran for the proof. Containers were unprivileged, without host network, a docker socket, the host home directory, or a shared cargo cache. Cleanup of these resources is recorded in `cleanup/run.log`.
