# Findings — a116 snapshot owner R1 review

Verdict: **APPROVE LIMITED R1**.

Reviewed commit `2b6dd57a0dc3d36e098268638c0364af247d07fb`, tree `9c75b25cf7e539cc043c7c90ee750aad9a2c3ccc`, parent `e5a066622e0409762fe44f11c4d303d2468a3290`.

The live oracle file ends with `fail_count=8` and `oracle_result fail`. Those eight marks stay failed. The product behavior they measured matches `contracts/source/pgsql/snapshot-owner-v1.md`.

## F1 — fixed source identity — approve

The review compiled a git archive of `2b6dd57`. Archive sha256 `1c9304da2ce6dd9524cdc19b1487fcc8ed2ccb453ed287dd77b4739162ed2bae`. Smoke and the live builder both printed `product_sha_exit=0`.

Product blobs from `git ls-tree 2b6dd57`:

| Path | Blob |
| --- | --- |
| `crates/oo-connectors/src/snapshot.rs` | `53aa65f239497d5cad451a2c518669a4e6ec770c` |
| `crates/oo-connectors/src/session.rs` | `5211832e3f9b6b297e6db21cfcd0e72d920d4704` |
| `crates/oo-connectors/src/sql.rs` | `07febf17b415abcb785644f60690c47939e0e50f` |
| `crates/oo-connectors/src/error.rs` | `16143c3f809af5f73810d49fa2051a92bef14472` |
| `crates/oo-connectors/src/lib.rs` | `6bba418242225640a620792f30234dba10b6e3d2` |
| `crates/oo-connectors/src/types/mod.rs` | `32c2541d7e63687000738521fa1c225557db9221` |
| `crates/oo-connectors/tests/snapshot_live.rs` | `a7c795fbe9a9b4dd258f19c0dccebf77917aceda` |
| `crates/oo-connectors/README.md` | `626214bc2ec1f3cba44000867fa4ffd74e5aa2fa` |
| `contracts/source/pgsql/snapshot-owner-v1.md` | `6306ae1cc3c5ff84809c9150212a66844986ba9a` |

## F2 — smoke — approve

Linux arm64, Rust 1.94.0 `(4a4ef493e 2026-03-02)`, cargo 1.94.0. Container `42d149c46b4610938945975252687a025ed29d6697e33750f5fc675a9bda7a06` exited 0. Lib tests: 49 passed, 0 failed, `lib_exit=0`. Package `live_pg`, `lossless_live`, and `snapshot_live` stayed not-run. Author a112 frozen smoke exit 101 stays in the author report.

## F3 — compile and Clippy — approve

Attempt 4: `cargo test --test a116_oracle --no-run` exit 0, then `cargo clippy -p oo-connectors --all-targets -- -D warnings` exit 0. The oracle then ran. Attempt 2 is a probe compile failure. Attempt 3 is `clippy::needless_borrow` in the probe. Neither opened an owner.

## F4 — lifecycle and release — approve

Observed on PostgreSQL 17.10 through `SnapshotOwner::open`, `read_probe`, `finish`, `abort`, and `Drop`.

- Quiet generation 41: `SnapshotFinished`, backend gone, `generation_reused`.
- Abort generation 43: `SnapshotAborted`, backend gone, `generation_reused`.
- Drop generation 44: the outcome line is `drop_returned_snapshot_finished no`, backend gone at 54 ms, `generation_reused`.
- Terminate generation 47: one matching `(pid, backend_start::text)` row, probe `57P01`, finish `Connect` / `io`, backend gone, `generation_reused`, later generation 71 `SnapshotFinished`.
- Wrong principal, database, postmaster, recovery, startup GUC, wrong CA, and wrong SAN host each returned the contract fact and released the backend.
- Tampered type oid: `catalog_identity`. Prior `PgSession::catalog`, then `REVOKE SELECT`, then open: `select_not_granted`.
- In-flight table `REVOKE SELECT`: probe and finish `insufficient_privilege` / `42501`.
- `GRANT INSERT`: `write_capable`.
- Inheritance parent plus child: probe and finish `probe_key`.
- Empty key and a text value on an int key: `probe_key`. Finish and abort after those errors were `SnapshotAborted` or `probe_key`, and the backend was gone.

`CLEANUP_OBSERVATION_BOUND_MS` is 5000 in `snapshot.rs` line 40. Every recorded release was inside that bound.

## F5 — same cut — approve, three oracle marks stay fail

`expected-outcomes.md` mtime is 2026-10-02T22:05:15Z. `live/expected/cut.txt` mtime is 2026-10-02T22:22:23Z. The owner opened at 2026-10-02T22:54:54Z.

Quiet probes matched the registered cedar cut, including null memo on line 22 and empty results for keys 99 and 23. Writer statements in the log are the three registered statements. Canary delta on that phase is 3. Fresh generation 70 matched willow, qty 9, unchanged line 22, and new line 23, and returned `SnapshotFinished`.

The same-cut owner `START TRANSACTION` is followed by `pg_is_in_recovery()` at 22:54:55.062. The writer commits at 22:54:55.321 through 22:54:55.325. The owner probe `WHERE id = $1` with `$1 = '23'` is at 22:54:55.349, and `same_absent_23_empty` passed. The side session's first statement after `START TRANSACTION` is a data `SELECT` at 22:54:55.341. `compare` in `live/a116_oracle.rs` requires the owner tokens and the side tokens to equal the old cut. `same_item_tokens`, `same_line_21_tokens`, and `same_side_absent_23` stay fail. The outcome file does not store the owner token text for item 7 or line 21 on generation 42.

## F6 — deadline — approve, two oracle marks stay fail

`deadline_probe` returned `Budget` / `deadline`. `deadline_probe_finish` and `deadline_finish` returned `Protocol` / `database` / `25P03`. Both backends were gone (`gone_ms=1` and `gone_ms=0`). The server log says `terminating connection due to idle-in-transaction timeout` at 22:55:16.057 and 22:55:36.336.

`sql.rs` lines 7–10 bind `idle_in_transaction_session_timeout` to `$1`, the statement timeout. The live parameter is `'20000'`. `error.rs` lines 191–200 map SQLSTATE `25P03` to `Protocol` / `database`. `snapshot.rs` `finish` at lines 457–471 calls `release_session` before `ensure_deadline`. Contract line 53: cleanup failure returns that failure, and `SnapshotFinished` is withheld. The oracle required `fact=deadline`, so those two marks stay fail.

## F7 — ACL and schema — approve, three oracle marks stay fail

In-flight table revoke is `ERROR: permission denied for table item` at 22:55:38.021, and the owner returned `42501`. `REVOKE SELECT (memo)` is at 22:55:38.141. The following owner statement `SELECT id,item_id,qty,memo ... LIMIT $2` is logged as `execute` with no permission error. `inflight_column_probe` is `fail batch`. `snapshot.rs` line 1046 returns `select_not_granted` when `attr.select != Some(true)`. Contract line 47 takes `has_*_privilege` as the server's answer. The column mark stays fail.

`ALTER TABLE rev.item ADD COLUMN a116_extra text` is at 22:55:38.538. The side count query at 22:55:38.539 is that session's first query. The owner probe at 22:55:38.541 returned a batch, and finish returned `SnapshotFinished`. The next open returned `catalog_identity`. `add_column_agreement` and `add_column_finish` stay fail. The outcome file does not store that probe's token text.

## F8 — SQL audit — approve

`A116_AUDIT_WINDOW_START` is at 22:22:22.495. The first `oo-pg-source` statement is at 22:54:54.780. `A116_AUDIT_WINDOW_END` is at 22:55:39.348. The log was not truncated. Pre-marker lines include the canary reset from attempt 1.

Classified window, from `oracle-outcome.txt`:

- oo-pg-source: `ROLLBACK=22 SELECT=797 START=25 WITH=2`, forbidden 0
- writer: `INSERT=1 UPDATE=2`
- admin: `ALTER=1 GRANT=4 REVOKE=4 SELECT=67 SET=2`
- side oracle: `ROLLBACK=2 SELECT=7 START=2`
- guc role: `SELECT=6`, forbidden 0
- other class empty
- `LIMIT` present, `"line item"` quoted, `"odd""note"` quoted

Copied log sha256 `a0f5eba6733ffd9eca97c792b1942fb44be3d8fbf097dcb1d96066d1c455a1fb`, 438706 bytes. The in-test counter recorded 438570 bytes. The extra trailer is the admin client reset after the end marker. Password-file hits across 56 evidence files: 0.

## F9 — unsigned

`identity_drift` and `nested_transaction` are `source_inspected_not_executed`. `session.rs` line 280 returns `nested_transaction` from `pub(crate) begin`. Package `live_pg`, `lossless_live`, and `snapshot_live` were not run. The live builder did not rerun `--lib`. Supply was not rescanned. a110 stays `request_changes` for `cargo deny` exit 4. `execution/decisions/a110-functional-development-gate.md` separates that supply result from functional development.

R2, R3, R4, the bounded reader, SP03-W03, customer and owner gates, protected CI, runsc, and release stay unsigned. This review does not mark the plan.
