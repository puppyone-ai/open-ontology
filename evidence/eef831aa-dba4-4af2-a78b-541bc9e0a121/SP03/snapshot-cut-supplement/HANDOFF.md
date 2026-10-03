# Handoff — a121 snapshot cut supplement

**Decision: REQUEST_CHANGES.**

Do not mark R1 approved. The blocking measurement is attempt 6 `add_column_active_current_schema`: generation 41 finished `SnapshotFinished` after `ALTER TABLE drift.item ADD COLUMN extra text`, and the production probe was still `int:5 text:pebble`.

The coordinator owns the plan checkboxes. This review does not mark SP03-W03, R2, R3, R4, whole SP03, type approval, supply, license, or release. It does not edit checklists. Backend `Drop` was not retested and is not R3 cancellation.

## Bound input

| | |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a121` |
| Role | independent reviewer, Grok 4.7 xhigh, no subagents |
| Reviewed commit | `2b6dd57a0dc3d36e098268638c0364af247d07fb` |
| Tree | `9c75b25cf7e539cc043c7c90ee750aad9a2c3ccc` |
| Parent | `e5a066622e0409762fe44f11c4d303d2468a3290` |
| Slot | A |
| Slot B | a120 handle 7304, left in place |
| Workspace tip when these files were written | `74fe4f0b8027b9a4b4e3c300998313a596634ae0` |
| Recomputed `git archive` sha256 | `e81cb9d790aac91285d6a33981b5fb3878ccfb76f1ca1a1b270e89c3758dc62c` |

This file is inside the evidence commit, so it cannot name that commit's own SHA. The workspace tip above is the parent this evidence commit sits on.

## Times

Reports were written after cleanup. `live/cleanup/result.txt` has `cleaned_at=2026-10-03T00:31:46Z`. The report stamp is `2026-10-03T00:33:24Z`. Those are different times. Cleanup had already removed this UUID's containers, volumes, network, and `/tmp/oo-a121-<uuid>`.

## What was measured on attempt 6

- Smoke: lib 49 passed, exit 0, Rust 1.94.0, linux/arm64. Package live tests not-run. Smoke was not rerun.
- Attempt 6: product hash 0, compile 0, Clippy 0 (`-D warnings`, locked, offline, toolchain 1.94.0), oracle exit 101, one fail name.
- Same-cut sets for `lot.batch` and `lot."child row"`: side select and owner `START TRANSACTION` before the writer commit; owner, side, and expected old tokens equal; fresh generation equals the new-cut oracle.
- Column-only open is `select_not_granted`. Table revoke then yields `42501` / `insufficient_privilege`, finish is not `SnapshotFinished`, backend gone in 0 ms. Table SELECT plus a column revoke still reads and finishes, which explains a116.
- Deadline finish is `Budget` / `deadline`, gone in 1 ms. Idle finish is `Protocol` / `database` / `25P03`, gone in 0 ms.
- SQL log last window: zero `oo-pg-source` DML, DDL, and `COMMIT`. Secret-file hits 0. Canary owner delta 0, writer delta 3.
- `identity_drift` and `nested_transaction` are `not_runtime_tested`. A role rename did not change session identity. `begin` is `pub(crate)`. Source text is not a runtime certificate.

## Unresolved scope

1. Current schema stop. Next edit is `crates/oo-connectors/src/snapshot.rs` `recheck` / `read_attributes` and `crates/oo-connectors/src/sql.rs` `SNAPSHOT_ATTRIBUTES`: compare attributes to a current non-snapshot catalog read, return `schema_drift`, poison the owner, and do not return `SnapshotFinished`. The live column appeared at attnum 6. Keep that fact.
2. `identity_drift` and `nested_transaction` have no public execution path that this probe could drive. They remain untested at runtime.

## Attempts to keep

| Attempt | Result |
| --- | --- |
| smoke `51936e83…` exit 0 | lib 49, Clippy 0 |
| 1 `1e0aad5e…` exit 101 | probe compile failed; clippy component missing |
| 2 `22eb1d44…` exit 101 | Clippy `collapsible_if`; oracle skipped |
| 3 `60a05ccb…` exit 101 | harness fails plus the schema fail |
| 4 `5490a3c3…` exit 101 | writer wait and log-order fails plus the schema fail |
| 5 `f6cf1c84…` exit 101 | writer `42501` on `lot.tick`; schema fail remains |
| 6 `42792620…` exit 101 | proof run; only the schema mark failed |

`lot_writer` received `SELECT, UPDATE` on `lot.tick` as a live fixture grant before attempt 6. `init.sql` still grants only `UPDATE`. A new database needs the `SELECT` grant or the canary trigger returns `42501`.

## Cleanup already done

Removed only UUID names: smoke, rust-review, rust-review-2 through rust-review-6, and postgres. Volumes `…-cargo`, `…-target`, `…-pglog` and network `…-net` are gone. `tmp=absent`. Four `oo-a064-*` containers were still listed after the name diff. No `a120` or `7304` container name was present to remove.

`live/sql-log.txt` is the copied server log, 794687 bytes. `live/cleanup/log-secret-files.txt` is `0`.

## Allocation

One own Rust builder at a time. One own PostgreSQL 17.10. No host compiler, no second architecture, no matrix, no `cargo deny`, no product edit, no new dependency.
