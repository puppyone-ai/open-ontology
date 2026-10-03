# a127 handoff

Review source commit `182702e6e67d16de10f280eaf422fce4e2f5a5f2`. This file is evidence and is not in that commit. Commands, exits, hashes, and the SQL classification are in `REPORT.md`. File digests are in `hashes.sha256`. The tested overlay linkage is `linkage/source-commit.txt`.

## Commit

| Field | Value |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a127` |
| Source commit | `182702e6e67d16de10f280eaf422fce4e2f5a5f2` |
| Tree | `444c52b8fd431396c77d2e3bf7144696aa9de78a` |
| Parent | `aa192ce7c588408d109228de2d27ca7a8f7b3213` |
| Parent tree | `c61619755547199a828e1ce45ec3c1aba5db7141` |
| Subject | connectors: add a pull-driven named portal on an open transaction |
| Fixed input | `b8e7bf1b1c25fbc0f8213898eb7de84b248f8211` |
| Fixed tree | `e8f408d6f66ef5361dc7c6d6185ca7942eaf1819` |
| Fixed parent | `578a55231e4b4373db014ff1450f8e1bb17ba639` |
| Accepted driver | `ff783ac933b86e0ff068181afe0258764c5d2e5d` |

The source commit contains only the ten owned paths. Before that copy, the six existing vendor files in the worktree matched `b8e7bf1`. The same bytes were hashed again from `git show b8e7bf1` after the commit and still match `linux/final/overlay-sha256.txt` pre lines and `linux/smoke/vendor-match.txt`. The committed blobs matched the snapshot compiled by `oo-a127-msrv-final` before that snapshot directory was removed.

Slot A files are absent from `182702e`. Two later commits landed on top of it before this evidence: `ccd93e1adc34b24ab998d90972549f8cffb59f0d` (`catalog: bound each source read and settle unused allowance`, tree `ef43877346afadcdccaf26fe6982c6b4e8b65902`) and `c05de23d024df902679959e164a8e7cd211165d7` (`evidence: record a128 bounded catalog rate repair`). The ten portal blobs are identical from `182702e` through `c05de23`. Review the portal at `182702e`. Manifests, the lock, and other shared source were not in the portal commit.

## What the driver does

`PgConnection::open_named_portal` admits only an open transaction (`transaction_depth >= 1` and `TransactionStatus::Transaction`). Each `pull` reserves one row and writes `Execute` with limit 1, then `Flush`. PostgreSQL 17.10 answers `PortalSuspended` when that positive limit is filled, including the last row. A later demanded pull that receives `CommandComplete` closes the portal and leaves the caller's transaction open. The default executor still uses the unnamed portal and `Execute` limit 0.

A database error on a known transaction resumes through `Sync` and `ReadyForQuery` with status `E`, then the caller can roll back. A cancelled pull, a short read, or an uncertain connection poisons the connection. `queue_simple_query` on a poisoned connection returns without writing.

## Green run

Container `oo-a127-msrv-final` exited 0. Image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, rustc 1.94.0 aarch64-unknown-linux-gnu. PostgreSQL image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/init.sh` accepts only `server_version` 17.10*. `pg/up.exit` is 0.

Phase exits, all 0: clippy-component, fetch, snapshot, toolchain, lib, focused, clippy, docker-run. Library 49 passed. `pg_portal_bounds` 6 passed. `pg_portal_live` 1 passed (`live_named_portal_repeatable_read`, 0.58s). `pg_wire_accounting` 12 passed. `pg_wire_bounds` 13 passed. Clippy `-D warnings` on `oo-connectors` exited 0. Two pre-existing `unfulfilled_lint_expectations` warnings remain in vendor `message/flush.rs` and `message/mod.rs`. Those files were not edited.

`linux/final/sql-log-class.txt`: reader verbs `BEGIN,ROLLBACK,SELECT,SHOW`, `reader_forbidden=0`. Writer verbs include `UPDATE`, `INSERT`, and `COMMIT`. Admin verbs are `DELETE`, `SELECT`, and `UPDATE`. Oracle verbs include `SET`.

## Preserved failures

Smoke attempt 1 exited 101 because the archive omitted `contracts/source/pgsql/empty-catalog.json`. Smoke attempt 2 exited 0: 49 passed, clippy 0, `portal.rs` absent. Final attempts 1 through 10 exited 101 and are under `linux/final/attempt-*`. The passing raw logs stay at `linux/final/` top level.

## Still open

R2-P stays unchecked. Independent review of `182702e6e67d16de10f280eaf422fce4e2f5a5f2` has not been done. Arrow for the same cut, a source-wide row budget, backend teardown within 5 seconds, the full reader, and R1 stay open. The ledger, plan, and checklist were not edited. Nothing was pushed.

Own containers `oo-a127-msrv-final`, `oo-a127-msrv-smoke`, and `oo-a127-pg`, network `oo-a127-eef831aa-net`, the four `oo-a127-eef831aa-*` volumes, and `/tmp/oo-a127-*` scratch were removed. a128 and a064 containers were still present afterward. The record is `cleanup.txt`.
