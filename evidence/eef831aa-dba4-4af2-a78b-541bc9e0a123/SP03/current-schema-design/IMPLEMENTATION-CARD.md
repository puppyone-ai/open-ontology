# Implementation card — current catalog guard

Slot A, after this design commit. One Rust builder, one private PostgreSQL 17.10, Linux arm64, Rust 1.94.0. No second architecture, no `cargo deny`, no host compiler, no plan checkbox. Follow `DESIGN.md`. Do not reopen the mechanism choice unless the savepoint hypothesis fails.

Input product is blob-identical to a121's review of `2b6dd57a0dc3d36e098268638c0364af247d07fb` for `snapshot.rs`, `session.rs`, `sql.rs`, `error.rs`, and `contracts/source/pgsql/snapshot-owner-v1.md`. Recompute those blobs before editing. If one differs, stop.

## Order

1. Capacity. Own builder 0, then take 1. Own PostgreSQL 17.10 verify-full, private CA, no host port. Record the capture.
2. Smoke the frozen tree before edits. Commands below. Record exits. A missing fixture is not-run, not a pass.
3. Edit only the write paths in `DESIGN.md`.
4. Lib tests, then the package live file, then Clippy, locked and offline, toolchain 1.94.0, `-D warnings`, package `oo-connectors`, all targets.
5. Live matrix in `TEST-MATRIX.md` on that PostgreSQL 17.10. Repair the `lot.tick` grant inside the new fixture. Record actual attnums from the admin session.
6. SQL audit started before the first `oo-pg-source` statement. Secret scan of the saved log is 0.
7. Own cleanup of this UUID's containers, volumes, and network only.
8. Evidence commit of this implementation's own path. Stop for a fixed-SHA review. Do not sign R1.

## Smoke, before edits

```text
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib -- --test-threads=1
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
```

`RUSTUP_TOOLCHAIN=1.94.0`. First Linux arm64 run of this implementation, not a historical matrix. a121 smoke exit 0 and a121 oracle exit 101 stay historical. Do not rewrite them.

## Code map

`session.rs`

- `try_reserve(source_id, n)` and `try_charge_rows(source_id, rows)`.
- `connect` reserves 1, then calls `connect_reserved`.
- `connect_reserved` is the current connect body. The session owns the permit.
- `begin_read_committed` sends `BEGIN_CURRENT` and sets `in_transaction`. A second begin returns `nested_transaction` before SQL.

`sql.rs`

- Add `BEGIN_CURRENT`, `SAVEPOINT_PROBE`, `ROLLBACK_PROBE`, `CURRENT_RELATION`, `CURRENT_ATTRIBUTES`, `CURRENT_PRIMARY_KEY`.
- Append them to `ALL`.

`snapshot.rs`

- `guard: Option<PgSession>` beside `session`.
- `open`: local checks, `try_reserve(2)`, `reserve` generation, `connect_reserved` data, `admit_connected`, `connect_reserved` guard, `admit_connected`, version_num equal, `begin` data, `begin_read_committed` guard, `capture`, guard observe, then return the owner.
- `read_probe`: data `recheck`, guard observe, `SAVEPOINT_PROBE`, existing probe `SELECT`, `ROLLBACK_PROBE`, decode, guard observe, return the batch only if both observes match.
- `finish`: if not poisoned and the deadline is still ahead, guard observe, including zero probes. Then release data and guard.
- `abort` and `Drop` release both.
- `fail_open` releases every session already connected and every permit not yet moved into a session.

Contract and README: the paragraphs in `DESIGN.md`.

## Stop conditions

- Savepoint statements fail inside the read-only repeatable-read transaction, or `ADD COLUMN` after a successful probe still blocks until `finish`. Stop. Leave the lock-holding design unwritten. Report that committed `schema_drift` after a probed relation cannot be observed without ending the data transaction.
- A path returns `SnapshotFinished` after a committed add, drop, rename, type, typmod, nullability, primary-key, or relation-oid change. That is the bug still open.
- A slot is taken without `try_reserve`, or a fifth socket is opened to dodge the cap. Remove it.
- The data session sends `COMMIT`, a second `BEGIN`, or a new isolation.
- Cleanup cannot account for both pids within 5 seconds on a success or failure path that closed both sockets.

## Out of this card

R2 streaming, R3 cancellation, R4 apply, `REQ-PL-07` pipeline pause, `identity_drift` runtime (role rename does not change `current_user`), `nested_transaction` runtime (no public second `begin`), supply, license, release, plan checkboxes.
