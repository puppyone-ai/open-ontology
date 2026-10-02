# HANDOFF — R2 stream wire analysis

Session `eef831aa-dba4-4af2-a78b-541bc9e0a118`. Sole executor, no subagent, no worktree. Read-only against product, manifests, lock, vendor, policy, docs, spec, plan, and older evidence. No builder, cargo, rustc, container, or PostgreSQL. No push and no release.

This handoff does not approve R1 and does not authorize R2 implementation. The reader is not delivered.

## Fixed input

| | |
|---|---|
| Commit | `2b6dd57a0dc3d36e098268638c0364af247d07fb` |
| Tree | `9c75b25cf7e539cc043c7c90ee750aad9a2c3ccc` |
| Parent | `e5a066622e0409762fe44f11c4d303d2468a3290` |

Product files match that tree. `HEAD` at analysis time was `3963fc2b0ece54cabe62071153918b2df815b1c7`, which only adds later coordination commits. Source SHA-256 and blob ids are `FINDINGS.md` section 7. Design is `REPORT.md`. Allocation trace is `FINDINGS.md`.

Functional inputs cited, not re-approved: source fix `34fae5ddd73ff250fde6f51aa681fb0f7fbd0362` with the a097 receipt, types `3ca5357930a2c44c42d3061db5fabe577881269a` with the a110 functional scope, and `execution/decisions/a110-functional-development-gate.md` (`9266f39e157d0d5690d3c61aa121a6bfca5dfdabb07c0a691e35293cd5c37eb2`). Supply `request_changes` stays open.

## What a later R2 writer may touch

Only after a new dispatch freezes these paths and an input SHA. Not this commit.

| Path | Change |
|---|---|
| `crates/oo-connectors/src/snapshot.rs` | `control_rows`, `LIVE_OWNERS`, `read_run` / `read_table`, phase and `protocol_sync`. `read_probe` body stays a probe. |
| `crates/oo-connectors/src/snapshot/stream.rs` | New. Pager, `StreamLimits`, `RunEvent`. |
| `crates/oo-connectors/src/error.rs` | Static facts in section 6 of `REPORT.md`. Match `PgIncomingLimit` by enum, not by driver string. |
| `crates/oo-connectors/src/lib.rs` | Export the new types. Keep the crate docs saying a missing stream method is not the bounded reader until the method exists and review signs it. |
| `crates/oo-connectors/src/sql.rs` | No new SQL text if the scan is built from quoted idents already stored on `BoundTable`. |
| `crates/oo-connectors/src/types/read.rs` | No mapping or canonical edit. Do not route the stream through `fetch_limit` or `compose_select`. |
| `crates/oo-connectors/src/types/batch.rs`, `binary.rs`, `arrow_batch.rs`, `mapping.rs` | Read only. |
| `crates/oo-connectors/tests/snapshot_stream_live.rs` | New. The section 7 cases in `REPORT.md`. |
| `contracts/source/pgsql/snapshot-stream-v1.md` | New contract for the pager, facts, and EOF rule. |
| `crates/oo-connectors/README.md` | One paragraph pointing at that contract. |

`config.rs` stays free of stream knobs so `oo-pg-config-identity/v1` stays put.

## What this dispatch must not touch

Root and crate manifests, `Cargo.lock`, `ci/deny.toml`, `vendor/**`, jobs, pipeline, SDK, CLI, docs, spec, plan, ledger, and any evidence directory other than this one. Slots a116 and a117 were not written.

## Dependency and vendor handoff

Required before the stream can enforce the reserve. The connector-only list above cannot implement it, because the socket is private to `PgConnection`.

Vendor writer, separate explicit assignment:

- `vendor/sqlx-postgres/src/connection/stream.rs` — bounded `DataRow` read and message-length refusal before `ControlFlow::Continue(expected_len)`.
- `vendor/sqlx-postgres/src/connection/executor.rs` — new portal methods only. `run`'s `limit: 0` and unnamed `Close` stay.
- `vendor/sqlx-postgres/src/connection/mod.rs` — `incoming_high_water`, `protocol_sync` flag, refuse `wait_until_ready` while unsynchronized.
- `vendor/sqlx-core/src/net/socket/buffered.rs` — counter inside `ReadBuffer::reserve` after `BytesMut::reserve`.
- `vendor/PATCH.md` Hunk 3. No new crate, no feature flag, no lock edit.
- A vendor unit test that a declared length above the cap returns `PgIncomingLimit::Message` and the high-water `reserved_additional` does not include that length.

`bytes` stays 1.12.1 (`fc652a48c352aef3ea3aed32080501cf3ef6ed5da78602a020c991775b0aff04`). Growth is `Vec::reserve` inside `BytesMut::reserve_inner`. The counter must read `capacity()` after that call.

Optional manifest line, also a separate handoff, only if the public type must implement `Stream`: direct `futures-core` `=0.3.34`, checksum `92d699e522242e69e3003b94ecc1f960f3a5e015aa7c5d7486e65ad01dd94f5e`, already in `Cargo.lock`. Until that line is frozen, ship `next_batch` on a concrete type and do not edit `Cargo.toml`. Do not enable `tokio` `sync`. The pull pager needs neither.

No new license exception. a110's deny failure still blocks release and default artifact admission.

## No-key admission

`selection_matches` (`snapshot.rs` lines 659–664) and `key_oids` (lines 929–931) return `probe_key` for an empty primary key, and `open` returns no owner. Full mode without a stable key needs that gate relaxed:

- Empty `primary_key` passes `open` when every other admission check passes.
- `key_oids` returns an empty vec for an empty key list.
- `read_probe` on that table still returns `probe_key`.
- Non-empty keys outside the R1 oid set, and keys longer than 8, still return `probe_key` and still return no owner.
- `snapshot-owner-v1.md` would have to say this. That contract edit waits on R1 review. Until it is accepted, a no-key stream cannot be opened and R2 must not claim that case.

## Unresolved

1. R1 independent review is open. This analysis does not close it.
2. Empty-key `open` is an R1 contract change. See above.
3. `max_connections` is enforced at `read_run` in this design. Enforcing it inside `open` would change R1 success conditions. Left for that review.
4. `StreamLimits` stay off `config_identity` v1. A v2 preimage is not proposed.
5. Idle-in-transaction timeout stays equal to `statement_timeout` (≤ 120 s). A slower consumer is `statement_timeout`, not a larger GUC.
6. Non-zero `Execute` limit disables parallel workers, as `executor.rs` lines 256–258 already note. Accepted for this pager.
7. Mid-page failure cannot confirm `ROLLBACK`. R2 returns `cleanup_failed` after `close_hard`. R3 adds cancel by `BackendKey` and the activity probe. The phase flag is the seam.
8. Catalog `fetch_all` during `open`, and `read_probe`, still reserve a full message before `charge`. The 2 MiB cap is on the new pager only, so catalog text up to 32 MiB can still allocate. Closing that hole is a separate cap on `run`, not part of the connector-only patch.
9. Whether the public API is `next_batch` or `futures_core::Stream` depends on the one-line manifest handoff.

## Review order

1. Coordinator accepts or rejects the vendor hunk and the empty-key contract edit.
2. A later writer implements only the frozen paths on a frozen SHA.
3. Linux arm64 Rust 1.94 focused smoke from `linux-msrv-focused-checks.md`, one builder, one PG 17.10 fixture, cases in `REPORT.md` section 7.
4. Independent fixed review of that implementation commit. Review of this directory is not R2 acceptance and is not R4.

## Evidence hashes

SHA-256 of the sibling files is in `HASHES.txt`, written after `REPORT.md`, `FINDINGS.md`, and this file. `HASHES.txt` does not hash itself.
