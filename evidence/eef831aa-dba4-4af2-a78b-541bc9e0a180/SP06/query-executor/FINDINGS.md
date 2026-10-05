# Findings

These are observations from this slice. They are not a parent-item acceptance.

## Review state

The same Grok session wrote `query_executor`, the live test, and the contract, and ran the Linux checks. No second reviewer has signed this slice. Treat the source commit as authored and tested by one party.

## Parent items

SP06-W04, SP06-W05, and SP06-W06 remain open. The contract says so. PostgreSQL restarts `statement_timeout` on each portal `Execute`. This executor sets that GUC once, to the remaining client budget minus about 250 ms, and enforces one client deadline from `execute_trusted_select` entry. The live deadline proof is one blocked `Execute` (an `ACCESS EXCLUSIVE` lock held by another session) with a 2 s client budget. Elapsed time was 1786 ms and the SQLSTATE was `57014`. That case does not show a multi-page server timer. The server timer is not a single 30 s span across portal pages.

## Untrusted admission

`UNTRUSTED_ADMISSION` is `Blocked`. The crate has no function that accepts an untrusted statement. A 500 ms process-isolated parser worker is not in this tree. The guard remains an internal dependency of the trusted runtime. a179's boundary on publishing the guard as a public read API still stands.

## Audit

`AuditAdmission::admit` runs before `BEGIN`. The live hook inserts through a second admin client and autocommits. That fixture is not a production audit WAL. Guard refusal does not call the hook. The dropped-stream case completed as `partial` with `end_audit_ok` false. Admit failure returned `AuditUnavailable` and the physical table name was absent from that backend's `pg_stat_activity` query text.

## Function-call stop proof

`track_functions=pl` is set on `oo-a180-pg`. `fixture.note_read()` is volatile plpgsql. The row-bound statement asks for `max_rows = 2` with portal page size 1 and no `ORDER BY`.

The first live assertion read `pg_stat_user_functions.calls` immediately and got 0. A later read of the same database, after that backend had gone away, showed 3. A direct check in `psql` reproduced the delay: after `ROLLBACK`, the cumulative counter stayed 0 in that session; `pg_stat_force_next_flush()` made 3 visible; disconnecting the backend also made 3 visible to the next session. Rollback alone does not publish the counter.

The green test closes the row-bound backend, then polls until the counter leaves the baseline. The oracle records `calls:3`. A count of 4 would fail the test. The type-refusal path polls the same way and recorded call delta 0 for OID 1700.

## Clippy gate

The denial gate is `-D warnings` on `--lib` and `--test query_executor_pg_live` only. A warning that exists only in `tests/sql_guard.rs` would not fail this command. The workspace clippy lints that are denied by name are `dbg_macro`, `todo`, and `unimplemented`, plus `unsafe_code` and `unused_must_use` from rustc. Everything else in the gate is a default warning promoted by `-D warnings`.

`drive`, `run_producer`, and `read_pages` carry `#[allow(clippy::too_many_arguments)]` because the producer arguments exceed 7. `classify_pg` has a `deadline_owned` flag. Current call sites pass `false`. Server `57014` is `Canceled` when the message starts with `canceling statement due to user request`, and `Timeout` otherwise.

## Lock and fetch

The private cargo volume was missing `object_store` after the workspace gained `oo-control`. `cargo fetch --locked` filled the volume. The lock file was not edited and was not part of the source commit.

## Attempts kept

| Attempt | Result |
|---|---|
| 1 | offline resolve failed, `object_store` absent from the private volume |
| 2 | lib tests passed; clippy driver missing for toolchain 1.94.0 |
| 3 | `manual_is_multiple_of` on the wall-clock check |
| 4 | integration test: `unused_mut`, `SelectStream` not `Debug`, double mutable borrow |
| 5 | `&mut` on a parameter that was already `&mut DedicatedSession` |
| 6 | `type_complexity` on the fixture row tuple |
| 7 | clippy 0; live panic, `i32` parameter versus OID |
| 8 | clippy 0; live panic, function calls read as 0 before flush |

The green logs are `linux/live/`. Earlier smoke logs under `linux/phases/` were not overwritten.

## Scope that this run did not exercise

Export, cell truncation, quality warnings, `EXPLAIN` cost, a pool, row-level security, freshness, HTTP, tokens, the CLI, and a production audit writer. The type certificate is the five bootstrap OIDs plus null. The live test is one scenario, not a full matrix.
