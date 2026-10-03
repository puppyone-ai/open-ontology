# REPORT — current catalog guard

Session `eef831aa-dba4-4af2-a78b-541bc9e0a125`. Protocol `oo.source.pgsql.snapshot/v1`.

This is an implementation record. It is not an R1 acceptance, a supply acceptance, or a customer release.

## Fixed commit

| | |
| --- | --- |
| Commit | `2254eab39c14a1902e65e72eb3674debc1c54f03` |
| Tree | `69fad83bf3c5b030f3a6eda9a05be04239de812b` |
| Parent | `9e515695d3c7e8b94936365a5f97fbc98124c345` |
| Subject | snapshot: guard the current catalog on a second read-committed session |

Design-freeze input recorded for this card: `ae8f9bb371997445da3b1110c8baec31c250f383`. Tree recorded before this work: parent `3479e440f0e4858983e55014e5f01a4fe733b40e`. Reviewed source compared before the edit was `2b6dd57a0dc3d36e098268638c0364af247d07fb`. Those blobs are the pre-edit baseline. The commit above is the implementation.

Owned paths in the commit:

- `crates/oo-connectors/src/session.rs`
- `crates/oo-connectors/src/sql.rs`
- `crates/oo-connectors/src/snapshot.rs`
- `contracts/source/pgsql/snapshot-owner-v1.md`
- `crates/oo-connectors/README.md`
- `crates/oo-connectors/tests/snapshot_live.rs`

`catalog.rs` was not edited. The rate bound for `collect_catalog` is the proposal in `catalog-rate-proposal.md`.

## What the owner does

One data connection stays in one `REPEATABLE READ READ ONLY` transaction. One guard connection stays in one `READ COMMITTED READ ONLY` transaction (`BEGIN_CURRENT`). The client does not send `COMMIT`, a second `BEGIN`, or a reconnect. Two connection permits are reserved before either socket. Every public `PgConnector::connect` reserves one. The cap is 4 per source. The tracked-source map stops at 64 and does not evict an in-window debit. Row charges happen before SQL. A probe `SELECT` runs inside `SAVEPOINT oo_probe_lock`. The lock drop is `ROLLBACK TO SAVEPOINT oo_probe_lock` and then `RELEASE SAVEPOINT oo_probe_lock`.

The guard observation is the single static statement `CURRENT_CATALOG`. Pre-probe, post-probe, and finish, including a finish with zero probes, compare every selected relation: oid, relkind, attribute attnum, name, type oid, typmod, nullability, and primary-key attnum/name order. Drift returns `schema_drift` and does not return `SnapshotFinished`. SQLSTATE `55P03` stays `budget` / `lock_timeout`.

## Toolchain

Linux arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `cargo 1.94.0 (85eff7c80 2026-01-15)`, `RUSTUP_TOOLCHAIN=1.94.0`, `--locked --offline`. PostgreSQL image digest `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, server `17.10 (Debian 17.10-1.pgdg12+1)`, `server_version_num=170010`.

## Commands and exits

Smoke, before product edits, container `…-rust-smoke`: lib tests and all-target clippy on the frozen tree, exit 0.

Prerequisite, container `…-rust-prereq` and `…-rust-prereq-locks`: both `result=pass`. Repeatable-read snapshot established before the savepoint. Catalog-shaped reads left `locks_after_catalog=none`. After rollback-to and release, `locks_after_release=none`. Writer `ADD COLUMN` committed in 0 ms. The old repeatable-read session kept the old row. A new read-committed session saw the new column at attnum 3 on that fixture.

Library, container `…-rust-lib-3`, `lib/console.log`: `test result: ok. 52 passed`, `lib_exit=0`, `clippy_exit=0`, `worst_phase_exit=0`.

`snapshot_live`, container `…-rust-live-1`: `docker-run.exit=101`. Panic at the fresh open, `SourceError { kind: Budget, fact: Some("max_rows_per_sec"), sqlstate: None }`. `schema_and_privilege` did not run. Secret hits 0. See FINDINGS.

Public-API oracle, container `…-rust-oracle-3`: compile of the working tree, `cargo test --locked --offline -p oo-connectors --test a125_oracle`, `oracle_exit=0`, `docker-run.exit=0`, test `finished in 42.71s`, `oom=false`, `privileged=false`. `oracle_start=2026-10-03T10:41:30Z`, `oracle_end=2026-10-03T10:42:16Z`. Log first byte `0x32`. Secret scan of the outcome, SQL audit, console, and server log against the password files: 0 hits. Log contains no NUL byte.

Per-actor SQL lines in `oracle/out/sql-by-actor.txt`:

| Actor | Lines |
| --- | --- |
| `oo-pg-source` / `lot_reader` | 1723 |
| `oo-a125-admin` | 206 |
| `oo-a125-writer` | 2 |
| `psql` | 2 |

Owner verbs: `SELECT` 1508, `START` 84, `ROLLBACK` 99, `SAVEPOINT` 16, `RELEASE` 16. Marker counts on those statements: `ROLLBACK TO` 16, `REPEATABLE READ` 42, `READ COMMITTED` 42, `pg_get_keywords` 40, current-catalog `LIMIT` plus `pg_attribute` 108. After quote stripping, the owner SQL has no `INSERT`, `UPDATE`, `DELETE`, `CREATE`, `ALTER`, `DROP`, `GRANT`, or `COMMIT` token. The privilege checks that contain those words are string arguments to `has_*_privilege`. Writer statements: 2, both writes. Admin statements: 206.

## Oracle results

`server_version=17.10 (Debian 17.10-1.pgdg12+1)`, `in_recovery=false`.

Rate: `max_rows=50000` refused with `max_rows_per_sec` after both transactions and the node read, before `pg_get_keywords` and before `transaction_timestamp`. Generation 1 was consumed. Reuse returned `generation_reused`. After the 1 second window, generation 2 with `max_rows=1000` opened. Keywords were then present in the log.

Cut: preregistered sha256 `55ba220bff63ae9c0110495ffe452a7e0c218c3802e4ee9ed8a6109fb449818e` matched the finished old generation. Data pid 817, guard pid 818. Fresh generation saw title `spruce` and qty 4. Index `batch_pkey` renamed to `batch_pk_a125` in 0 ms and the same generation finished successfully.

Cap: generations 41 and 42 returned `max_connections` before a socket and were not consumed. Both later opened. Finish, abort, and drop released both backends. `prove_slots` then held four backends and released them (`slots_recovered=lotclean`).

C8: probe `protocol` / `database` / `57P01`. Finish fact `io`. Not `SnapshotFinished`. Not `schema_drift`.

C9 fact `tls_handshake`. C10 facts `principal` and `database`. C11 fact `read_node`. C12 deadline before connect, generation kept. C13 deadline before finish. Each consumed-generation case recovered its slots.

Schema drift, attnums read from `pg_attribute` during the run:

| Case | Attnum | Result |
| --- | --- | --- |
| other selected table `a125_other` | 4 | `schema_drift` on probe and finish |
| C1 `a125_c1` | 4 | `schema_drift`, ddl 1 ms |
| C15 same column, new generation | 4 | 4 plans, previously 3, finish succeeded |
| C2 `a125_c2` | 5 | drift after the returned old probe, label `still` |
| after last probe `a125_last` | 6 | finish `schema_drift` |
| C3 zero probes `a125_c3` | 7 | finish `schema_drift` |
| drop `a125_drop` | 8 | `schema_drift` |
| rename, varchar(16), varchar(40) |  | `schema_drift` |
| `a125_nn` set not null | 9 | `schema_drift` |
| primary key | old `1:id`, new attnum 10, `10:a125_pk` | `schema_drift` |
| recreate `drift.item` | oid 16464 then 16493 | `schema_drift` |

Every recorded DDL elapsed time was 0 or 1 ms.

## Left open

All 87 goals, supply, customer release, R2, R3, and R4 stay open. a121 attempt 6 stays red. No plan checkbox was changed.
