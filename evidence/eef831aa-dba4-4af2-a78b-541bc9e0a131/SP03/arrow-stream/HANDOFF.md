# Handoff

## Commit

`75b993d0b578fa863a32c64abc0073ed92d51a5d` on parent `92ff8b1fdb9d3480c182689bcfda5652f4de4487`.

Owned paths in that commit:

- `crates/oo-connectors/src/snapshot.rs`
- `crates/oo-connectors/src/stream.rs`
- `crates/oo-connectors/src/session.rs`
- `crates/oo-connectors/src/error.rs`
- `crates/oo-connectors/src/lib.rs`
- `crates/oo-connectors/src/sql.rs`
- `crates/oo-connectors/src/types/read.rs`
- `crates/oo-connectors/src/types/batch.rs`
- `crates/oo-connectors/src/types/mod.rs`
- `crates/oo-connectors/tests/pg_stream_live.rs`
- `crates/oo-connectors/tests/pg_stream_bounds.rs`
- `contracts/source/pgsql/bounded-stream-v1.md`

`config.rs` was not edited. `arrow_batch.rs`, manifests, locks, vendor, `PATCH.md`, jobs, pipeline, CLI, spec, and ledger were not edited. No push.

## Proposal for `types/mod.rs`

`types/mod.rs` is not on the R2-S write list. The commit adds one `pub(crate)` re-export:

`pub(crate) use read::{compose_presence_scan, compose_scan, guard_plans, require_binary};`

`snapshot.rs` has to call the scan composers, and those composers live in `types/read.rs`, which is an owned path. The module file is the existing crate-visible bridge. The re-export does not change type mapping or canonical decode semantics. A later edit can move that bridge if the crate gains an owned seam that `snapshot.rs` can name without `types/mod.rs`.

## API a later reader should keep

`SnapshotOwner::open_stream` then `next_batch` pulls one `StreamItem` per demand: `Batch`, `TableEof`, or `Exhausted`. `finish` is still the run finish. One owner, one repeatable-read data session, one read-committed guard. The stream does not open a third connection. `begin` goes through `PgTransactionManager::begin` so named-portal admit sees transaction depth 1. The SQL text stays `BEGIN` / `BEGIN_CURRENT`.

`plan.key` is set only for a probe-supported key. Empty keys, unsupported keys, and keys of 9 to 32 columns still open. Wider than `INDEX_MAX_KEYS` (32) is config `selection`. Empty columns with a non-empty key stay `probe_key`. `read_probe` during an active stream returns protocol `stream_active` and does not poison the owner.

Receive bounds are installed after the catalog recheck and before portal open. The portal is closed before `ROLLBACK TO` / `RELEASE` of `oo_stream_lock`. A poisoned or unsynchronized connection skips rollback SQL. `StreamItem::Batch` is boxed because the batch owns the Arrow arrays.

## Evidence

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a131/SP03/arrow-stream/`

- `REPORT.md`, `FINDINGS.md`, this file
- `linux/smoke/` initial green smoke, not rerun
- `linux/functional/` final exits and the preserved failed consoles
- `pg/` fixture scripts and `up.exit`
- `cleanup.txt` after owned resources were removed
- `hash-manifest.txt` over the other files in this directory

Raw functional command, from the host, with secrets staying in the untracked secret directory:

`bash evidence/eef831aa-dba4-4af2-a78b-541bc9e0a131/SP03/arrow-stream/linux/functional/launch.sh`

The wrapper records `EXIT:$?` from `launch.sh`. That line is the builder result. A shell that only echoes the line then exits 0 is not a second result.

## Still open

R3 advanced key and cancel behavior, R4, and whole-reader acceptance are unsigned. The shrink-panic gate waits on the fixed a129 review. Large-stream validation belongs on that reviewed driver, not on a moving vendor overlay. Slot A remains the other session's builder and Postgres budget. This session's containers, volumes, network, and secret directory are removed in `cleanup.txt`.
