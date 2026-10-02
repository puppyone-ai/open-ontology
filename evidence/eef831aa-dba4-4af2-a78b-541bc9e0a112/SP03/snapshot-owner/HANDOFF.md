# HANDOFF — a112 snapshot owner R1

Local commit for an independent fixed-SHA review. Slot B is reserved for review a114. No push, no pull request, no release, no customer action. Do not treat this commit as a signature on R1, the bounded reader, or any parent.

The commit that contains this file is the delivery. Read it with `git rev-parse HEAD` and `git rev-parse 'HEAD^{tree}'`. At authoring, the parent was `e5a066622e0409762fe44f11c4d303d2468a3290`, tree `5071b838764780759ee1ed016417b6d37b0bc9f8`.

## Review this

- `crates/oo-connectors/src/snapshot.rs` — the owner.
- `crates/oo-connectors/src/session.rs` — `begin` rejects a nested transaction.
- `crates/oo-connectors/src/sql.rs` — snapshot statements, still read-only.
- `crates/oo-connectors/src/error.rs` — `privilege` fact.
- `crates/oo-connectors/src/lib.rs` — public re-exports.
- `crates/oo-connectors/src/types/mod.rs` — `pub(crate)` re-export of existing `decode_payload` and `guard_plans` only. No mapping or decode change.
- `crates/oo-connectors/tests/snapshot_live.rs` — the live proof. It fails if `OO_PG_SNAPSHOT_FIXTURE` is absent.
- `crates/oo-connectors/README.md` — R1 API and unsigned R2–R4.
- `contracts/source/pgsql/snapshot-owner-v1.md`.
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a112/SP03/snapshot-owner/` except `smoke/archive/`, which is a local frozen tree and is not staged.

`config.rs`, `catalog.rs`, `access.rs`, manifests, the lock, and `vendor/` are not in this commit.

## Proof to read

`REPORT.md` has the exits and the measured 17.10 cut. Final builder exit 0. Lib tests 49 passed. `snapshot_owner_live` passed. Clippy `-D warnings` exit 0. Frozen smoke package exit stays 101 and is not-run. Package `live_pg` and `lossless_live` were not run.

Same-cut rows matched an independent repeatable-read oracle. The new generation saw the writer commit. In-flight `ADD COLUMN` stayed on the old cut (`schema_inflight=old_cut`); the next generation returned `catalog_identity`. In-flight `REVOKE SELECT` failed the probe (`privilege_inflight=insufficient_privilege`); the next generation returned `select_not_granted`.

## Cleanup

Own resources removed after the logs and `postgresql.log` were copied, and before this commit:

- containers `eef831aa-dba4-4af2-a78b-541bc9e0a112-pg`, `eef831aa-dba4-4af2-a78b-541bc9e0a112-rust-r1`, `eef831aa-dba4-4af2-a78b-541bc9e0a112-rust-base`
- volumes `eef831aa-dba4-4af2-a78b-541bc9e0a112-cargo`, `eef831aa-dba4-4af2-a78b-541bc9e0a112-target-base`, `eef831aa-dba4-4af2-a78b-541bc9e0a112-target-r1`, `eef831aa-dba4-4af2-a78b-541bc9e0a112-src-r1`, `eef831aa-dba4-4af2-a78b-541bc9e0a112-pglog`
- network `eef831aa-dba4-4af2-a78b-541bc9e0a112-net`
- secret directory `/tmp/oo-a112-eef831aa-dba4-4af2-a78b-541bc9e0a112`

The cleanup command output is `live/cleanup.txt`. Foreign containers were not removed. `smoke/archive/` stays on disk and is not in the commit.

## Stop

Independent review of the fixed commit only. R2, R3, and R4 stay unsigned.
