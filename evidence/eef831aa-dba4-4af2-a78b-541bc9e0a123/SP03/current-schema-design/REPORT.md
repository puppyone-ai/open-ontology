# REPORT — a123 current catalog guard design

**Design frozen. R1 not accepted.** No product edit, no builder, no PostgreSQL, no plan checkbox.

The owner's repeatable-read `pg_attribute` reread is stale after a committed `ADD COLUMN`. The fix is a second session that reads the current committed catalog, reserved as 2 of the source cap of 4 before either socket. The data connection stays one `REPEATABLE READ READ ONLY` transaction. Locking the user table for the owner lifetime is not the check.

## Identity

| | |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a123` |
| Role | Grok 4.7 xhigh, sole analyst, no subagents, no worktree, no builder grant |
| Fixed input | `eb288ad96c9941369fc8afa98cb35612b70d23e9` |
| Fixed input tree | `8c64dcfa9831c1f08dab3d3e43394eea08657ae2` |
| Fixed input parent | `74fe4f0b8027b9a4b4e3c300998313a596634ae0` |
| Product under analysis | `2b6dd57a0dc3d36e098268638c0364af247d07fb` |
| Workspace tip while writing | `3479e440f0e4858983e55014e5f01a4fe733b40e` |
| Workspace tree | `52a2884e837a6c4b326f64a78ad9f4282b69ac9b` |
| Workspace parent | `b0d9e5fd8afcd25c0d118fc0c42c6e0dc15e350b` |
| Report stamp | `2026-10-03T08:51:01Z` |
| Slot A | Reserved for the later implementation. Not taken here. |
| Slot B | a122 driver review. Not touched. |

`git diff eb288ad..HEAD` is empty for `snapshot.rs`, `session.rs`, `sql.rs`, `error.rs`, and `contracts/source/pgsql/snapshot-owner-v1.md`. Blobs:

| Path | Blob |
| --- | --- |
| `crates/oo-connectors/src/snapshot.rs` | `53aa65f239497d5cad451a2c518669a4e6ec770c` |
| `crates/oo-connectors/src/session.rs` | `5211832e3f9b6b297e6db21cfcd0e72d920d4704` |
| `crates/oo-connectors/src/sql.rs` | `07febf17b415abcb785644f60690c47939e0e50f` |
| `crates/oo-connectors/src/error.rs` | `16143c3f809af5f73810d49fa2051a92bef14472` |
| `contracts/source/pgsql/snapshot-owner-v1.md` | `6306ae1cc3c5ff84809c9150212a66844986ba9a` |

This file sits inside the evidence commit, so it does not name that commit. `HASHES.txt` lists the other files in this directory and does not list itself.

## What was read

a121 `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, and the attempt-6 outcome cited there (`fail_names=add_column_active_current_schema`, generation 41, attnum 6, `SnapshotFinished`). a121 receipt `execution/decisions/a121-snapshot-supplement-receipt.md`. a116 receipt `execution/decisions/a116-snapshot-review-receipt.md`. Owner card `execution/prompts/v01-pg-snapshot-owner.md`. Prepared review `execution/prompts/v01-pg-snapshot-owner-review-prepared.md`. Contract `contracts/source/pgsql/snapshot-owner-v1.md`. Spec `REQ-SRC-03` and `REQ-PL-07`. Docs `03` limits, `04` snapshot budget, `12` limits, `17` connection release. Product `snapshot.rs`, `sql.rs`, `session.rs`, `config.rs`, `error.rs`, and the connector README.

The a121 oracle source, fixture grant note, and SQL log were used through those reports. `init.sql` still lacks `SELECT` on `lot.tick`. Reproduction repairs that grant in the new fixture and records live attnums.

## Frozen behavior

`try_reserve(source_id, 2)` then the existing generation reserve, then two `connect_reserved` calls. Data: unchanged `BEGIN` of repeatable read, read only. Guard: `START TRANSACTION ISOLATION LEVEL READ COMMITTED, READ ONLY`. Guard statements compare oid, relkind, attributes, and primary key with the captured shape after capture, before and after each probe, and on finish with zero probes. Mismatch is `schema_drift`, the owner is poisoned, and `finish` is not `SnapshotFinished`.

The probe `SELECT` is wrapped in `SAVEPOINT oo_probe_lock` and `ROLLBACK TO SAVEPOINT oo_probe_lock` so the user-table `ACCESS SHARE` does not outlive the statement. The snapshot stays the capture snapshot. `RELEASE SAVEPOINT` is not used.

Public method signatures and `SnapshotBinding` fields stay. New facts are `max_connections` and `max_rows_per_sec` on the existing budget kind. Cap constants are 4 and 50,000. They are not config knobs.

## Write paths for the coder

| Path | |
| --- | --- |
| `crates/oo-connectors/src/session.rs` | Ledger and reserved connect |
| `crates/oo-connectors/src/sql.rs` | Static templates |
| `crates/oo-connectors/src/snapshot.rs` | Guard lifetime and observe points |
| `contracts/source/pgsql/snapshot-owner-v1.md` | Replace the stale `schema_drift` sentence |
| `crates/oo-connectors/README.md` | One data connection plus one guard connection |
| `crates/oo-connectors/tests/snapshot_live.rs` | Package regression |

No manifest, lock, vendor, docs, spec, plan, or ledger change.

## Conditional stop

If PostgreSQL 17.10 rejects the savepoint pair inside this read-only transaction, or `ADD COLUMN` after a returned probe still cannot commit, the coder stops. Committed `schema_drift` on a relation this transaction has scanned cannot be observed while `ACCESS SHARE` is held, and ending the data transaction is forbidden. That case is the coordinator item. Every other case in `TEST-MATRIX.md` is specified.

## Not signed

R1, R2, R3, R4, whole SP03, type approval, supply, license, release. `identity_drift` and `nested_transaction` remain without a runtime path. `REQ-PL-07` stays outside this slice. a121 attempt 6 remains red.
