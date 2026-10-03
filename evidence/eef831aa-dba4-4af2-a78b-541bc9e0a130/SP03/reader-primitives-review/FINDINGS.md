# FINDINGS — reader primitives at ccd93e1a

Session `eef831aa-dba4-4af2-a78b-541bc9e0a130`. Reviewed commit `ccd93e1adc34b24ab998d90972549f8cffb59f0d`, tree `ef43877346afadcdccaf26fe6982c6b4e8b65902`, parent `182702e6e67d16de10f280eaf422fce4e2f5a5f2`. Compile root was `git archive` of that commit. The worktree was not the compile root.

Portal blobs for the ten named-portal paths are identical between parent `182702e6e67d16de10f280eaf422fce4e2f5a5f2` and this commit (`linkage/portal-blobs.txt`, `mismatch=0`). Schema commit `2254eab39c14a1902e65e72eb3674debc1c54f03` is the ancestor of the snapshot owner on this parent. Catalog rate is this commit.

## Verdicts

| Slice | Verdict |
| --- | --- |
| Schema and current catalog | APPROVE |
| Catalog rate | APPROVE |
| Named portal | APPROVE |

These three verdicts cover the composed primitive behavior exercised below. They do not close R1, the reader, or Arrow. The driver integration gate stays open and is recorded after the slice findings.

## F1. Schema and current catalog — APPROVE

`linux/proof/console.log` `t1_schema_guard` passed on PostgreSQL 17.10.

Same cut. Source `rv_cut` probed `rv.parent` and `rv.child` under one owner. A separate writer committed `label = spruce` and inserted child `12`. The owner's later probes still returned `oak`, quantity `2`, and an empty probe for child `12`. A second `REPEATABLE READ` session, whose snapshot was taken with `SELECT 1` before that commit, saw the same cut. `finish` returned `Ok`. Generation 2 on the same source saw `spruce`, child `12` with quantity `7`, and a different `canonical_hash`. `TypedBatch::record_batch` (`crates/oo-connectors/src/types/batch.rs:142`) produced one row with columns `id` and `label`.

Index rename. `ALTER INDEX rv.child_parent_idx RENAME TO child_parent_rv` left the in-flight probe at quantity `2` and `finish` returned `Ok`. A non-unique secondary index name is outside `AdmittedTable` identity.

Current catalog. After a returned probe of `rv.drift`, `ALTER TABLE rv.drift ADD COLUMN extra text` made the next probe and `finish` return `schema_drift`. The retained probe value stayed `pebble`. Opening `rv.quiet` and adding a column before any probe made `finish` return `schema_drift`. `SnapshotFinished` is constructed in `finish` and returned only after the guard observation, both releases, and the deadline check (`crates/oo-connectors/src/snapshot.rs:503-525`).

Connection cap. `SnapshotOwner::open` calls `session::try_reserve(source_id, 2)` before `reserve(generation)` and before `connect_reserved` (`snapshot.rs:385-407`). Two owners on `rv_cap` held four `oo-pg-source` backends for `oo_snap_reader` on `snapdb`. The third open returned `budget` / `max_connections` and the backend count stayed 4. After one owner finished, generation 3 opened and finished. The refused open had not consumed that generation.

Cleanup. Finish, abort, and drop each released both backends of that source identity. `Drop` spawns `release_session` for the data session and the guard (`snapshot.rs:700-718`). Terminating the guard backend made `finish` return an error whose fact was `io`. The fact was not `snapshot_finished`.

Author regression on the same archive, unmodified `snapshot_live`, also passed: `schema_inflight=schema_drift`, `privilege_inflight=insufficient_privilege`, `same_cut_hash=1ff301767c85e585b52df42ae0ab47d1cbfe6c15205b6a0fcef98605badd8236` (`linux/author/console.log`). That author test adds the column before the probe, so the post-probe drift claim is the proof in `t1`, not the author test. C0–C15 was not repeated.

## F2. Catalog rate — APPROVE

`t2_catalog_rate` passed.

Repeated catalogs of `rv.parent` and `rv.child`: three calls at `max_rows=1000` took 170.382417ms; four calls at `max_rows=50000` took 195.07125ms. Both are inside one second. Four 50000 catalogs succeed inside the window only when unused allowance is refunded in that same window. `RowHold` has no `Drop` (`crates/oo-connectors/src/session.rs:137-151`). `settle` refunds `reserved - observed` only when `entry.window_started` is still the hold's window and the window is under one second (`session.rs:145-186`).

The server log after `a130_rate_limits` for `app=oo-pg-source` and `user=oo_snap_reader` bound the same integers the reservation uses: keywords `80` (`PG17_RESERVED.len()+1`), tables `2`, columns `3200`, enum `4096`, domain `160`, primary keys `64`, unique keys `4096`, foreign keys `4096`, policies `256`, ACL `512`.

A locked `read_typed` on `rv.lockme` with `max_rows=50000` and a 400ms statement timeout returned `budget` / `statement_timeout`. The next connect and catalog on source `rv_deny` in that same second returned `budget` / `max_rows_per_sec` at 677.256ms. The log tail after the last `a130_deny_cut` contains no `pg_get_keywords`. `fetch_all` settles only the successful row path; the timeout and SQL-error arms leave the hold unsettled (`session.rs:879-906`).

Unmodified `catalog_rate_live` on this archive passed in 1.43s (`linux/author/catalog_rate_live.exit` 0): `big_elapsed=176.664917ms`, `reader_writes=0`. That test requires a 32-column primary key to come back complete (`catalog_rate_live.rs:97-100`) and 128 policies on `rate.pol` to return `catalog` / `catalog_truncated` (`catalog_rate_live.rs:116`, fixture at `279-285`). `filled_sentinel` is `session.rs:292`.

Window expiry is the lib tests that ran in the smoke `cargo test -p oo-connectors --lib` (55 passed, `lib.exit` 0): `reservation_settles_the_unused_allowance`, `reservation_abandon_keeps_the_debit`, `reservation_does_not_refund_after_the_window` (`session.rs` unit tests, smoke console lines 588-590). No sleep test and no large payload were added.

`MAX_ROWS_PER_SEC` is a process-local row ledger (`session.rs:21`). It is not a measurement of backend heap or of memory the caller keeps after the call returns. Wiring this ledger through the whole reader is still future work.

## F3. Named portal — APPROVE

`t3_named_portal` passed. `EXECUTE_LIMIT` is 1 (`crates/oo-connectors/vendor/sqlx-postgres/src/connection/portal.rs:27`).

On `rv.page`, the first row and the last row each returned `PgPortalOutcome::Suspended` with `execute_limit=1`, `reserved_rows=1`, and `consumed_rows=1`. While the portal was suspended after row 1, `oo_a127_writer` committed `spruce` on row 2 and inserted row 3 `cedar`. The next pull still returned row 2 `fir`. The following demanded pull returned `Complete` with `row=None` and `consumed_rows=0`. The reader transaction stayed `repeatable read`, and its later `SELECT` saw `1:pine,2:fir`. The exported snapshot id was `00000021-00000017-1`. An oracle session imported it with `SET TRANSACTION SNAPSHOT` while the exporting transaction was still open and saw the same cut.

`close` on a still-suspended portal returned ready with the caller transaction open. Isolation stayed `repeatable read` and `SELECT 1` succeeded.

`SELECT (1/$1)::int4` bound to 0 failed with sqlstate `22012`, `consumed_rows=0`, and the connection accepted a later statement after rollback. `pg_sleep(30)` cancelled by `pg_cancel_backend` failed with `57014`, `consumed_rows=0`, and the connection was not poisoned. Dropping an open suspended portal set the lease poisoned (`portal.rs:148-154`). `named_portal_poisoned` (`portal.rs:722`) was true, and the next `SELECT 1` on that connection failed.

The suspended row exposed `id` and `note`, type oids 23 and 25 (`type_info.rs:221`), the same `relation_id` (`column.rs:30`), and attribute numbers 1 and 2 (`column.rs:40`). That is the page metadata a later Arrow batch can reuse. This review did not feed portal rows into `TypedBatch`. `PgConnector`'s public methods are `new`, `connect`, `inspect_access`, and `catalog` (`session.rs:370`, `378`, `485`, `499`). The portal takes the caller's statement on an existing `sqlx` `PgConnection` transaction. No general arbitrary-SQL method was added to `PgConnector`.

Unmodified `pg_portal_bounds` passed 6 tests in 26.51s. Unmodified `pg_portal_live` passed in 0.54s with `reader_forbidden=0` and `snapshot_id=0000000F-00000019-1`.

## F4. Own reader SQL log

`pg_rotate_logfile()` returned true and left the same `postgresql.log` in place, because `log_filename` has no strftime pattern. `t1` then executed `SELECT 'a130_own_log_cut'`. `t4` classifies only the suffix after the last copy of that marker.

`linux/proof/sql-audit.txt`:

```
reader_statements=686
reader_verbs=BEGIN:4,RELEASE:10,ROLLBACK:43,SAVEPOINT:10,SELECT:585,SHOW:2,START:32
reader_forbidden=0
writer_statements=7
writer_write_hits=6
admin_statements=64
admin_write_hits=23
```

An independent classifier over that same suffix counted 686 reader, 7 writer, and 64 admin statements. Reader write tokens were empty. Writer tokens were `COMMIT`, `INSERT`, and `UPDATE`. Admin tokens were `ALTER`, `CREATE`, `DROP`, `GRANT`, and `INSERT`. The copied log is 1026266 bytes, first byte 50 (`2`), and `log-leak.txt` is `log_leak=no`. a127's omitted raw log and its cumulative 85-statement classification were not reused.

## F5. Driver integration gate — retained

`crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs:590` still panics with `incoming view shrank during advance`. The blob at this commit is `f41e0e4dd6baca062fb332ab66da3f5e9c0600fd`. The proof archive's copy matched `git show` of that path, and `sha256sum -c` of the pinned product files returned OK. a129's moving worktree edit was not compiled.

a128 verify attempt 3 already recorded the panic on a 60000-row read. That failure stands. This review did not rerun it and does not treat the primitive passes as clearance of that panic. A whole-reader or R1 completion claim would still have to pass a driver integration test that this session did not run.

## F6. Proof attempts kept

| Attempt | Exit | What it shows |
| --- | --- | --- |
| `linux/proof/attempt-1` | 101 | Overlay did not compile: `ArrowScalar` path, private `Arguments::add`, `PgPortalStep` not `Debug`, `SelectedTable::new` on `&&str`. Product hashes matched. |
| `linux/proof/attempt-2` | 101 | Overlay borrow: the pinned `pull` future outlived later uses of `portal` and `tx`. Product hashes matched. |
| `linux/proof/attempt-3` | 101 | `t2` and `t4` passed. `t1` panicked because the oracle's first statement ran after the writer commit, so its snapshot saw `spruce`. The owner re-probe had already required `oak`. `t3` returned sqlstate `42704` because `SET TRANSACTION SNAPSHOT` ran after the exporting transaction rolled back. |
| `linux/proof` | 0 | Four tests passed in 5.45s. `docker-run.exit` 0. |

The attempt 3 failures were overlay sequencing. They are not product defects. No product file was edited.
