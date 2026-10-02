# REPORT — a116 independent R1 snapshot owner review

**Verdict: APPROVE LIMITED R1.**

Public `SnapshotOwner::open`, `read_probe`, `finish`, `abort`, and `Drop` on commit `2b6dd57a0dc3d36e098268638c0364af247d07fb` match `contracts/source/pgsql/snapshot-owner-v1.md` for the measured PostgreSQL 17.10 cut. The independent oracle process exited 101. `live/phases/oracle-outcome.txt` ends with `fail_count=8` and `oracle_result fail`. Those eight marks stay failed. Each one is accounted for below from the frozen source and from `live/phases/postgresql.log`.

This approval covers R1 lifecycle and catalog binding. It does not complete SP03-W03. R2, R3, R4, the bounded reader, supply, license, customer, owner, protected CI, runsc, and release stay unsigned. The coordinator owns the plan checkboxes.

## Identity

| | |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a116` |
| Reviewed commit | `2b6dd57a0dc3d36e098268638c0364af247d07fb` |
| Tree | `9c75b25cf7e539cc043c7c90ee750aad9a2c3ccc` |
| Parent | `e5a066622e0409762fe44f11c4d303d2468a3290` |
| Archive sha256 | `1c9304da2ce6dd9524cdc19b1487fcc8ed2ccb453ed287dd77b4739162ed2bae` |
| Captured workspace tip | `a26e7d0344cd6ef2b4c40d2f33311a86f7327050` at 2026-10-02T22:01:34Z |
| Workspace tip at commit | `3a06e6042de6dda92fd1c7e6b011fef002aeca56` |
| Slot | A |
| Executor | Grok 4.7 xhigh, no subagents, no worktree, no host cargo |

The archive is a `git archive` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `contracts` at `2b6dd57`. The evidence probe was copied onto a separate source directory for the live builder. The archive itself has no `tests/a116_oracle.rs`. Product, manifests, locks, vendor, docs, and the plan were not written.

`git ls-tree 2b6dd57` blobs:

- `snapshot.rs` `53aa65f239497d5cad451a2c518669a4e6ec770c`
- `session.rs` `5211832e3f9b6b297e6db21cfcd0e72d920d4704`
- `sql.rs` `07febf17b415abcb785644f60690c47939e0e50f`
- `error.rs` `16143c3f809af5f73810d49fa2051a92bef14472`
- `lib.rs` `6bba418242225640a620792f30234dba10b6e3d2`
- `types/mod.rs` `32c2541d7e63687000738521fa1c225557db9221`
- `tests/snapshot_live.rs` `a7c795fbe9a9b4dd258f19c0dccebf77917aceda`
- `README.md` `626214bc2ec1f3cba44000867fa4ffd74e5aa2fa`
- `contracts/source/pgsql/snapshot-owner-v1.md` `6306ae1cc3c5ff84809c9150212a66844986ba9a`

Smoke and attempt 4 both printed `product_sha_exit=0` for `product-sha256.txt`.

Author a112 `REPORT.md` and `HANDOFF.md` were read as delivery context. Author lib 49, live 1, Clippy 0, catalog hash, and row labels are author evidence. This oracle sets `author_outcome_reused no` and uses its own registered cut. Author smoke exit 101, from missing `OO_PG_FIXTURE`, stays in the a112 report. a097 source functional approval and a110 types functional approval were read as prior functional scope. `execution/decisions/a110-functional-development-gate.md` keeps a110 `cargo deny` exit 4 as `request_changes` and separates that supply failure from functional development. This review did not rerun a supply scan.

## Capacity

Slot A. Slot B stayed with a114. a115 had no builder grant. Handles 50371, 82080, and 82672 were absent at smoke start 2026-10-02T22:02:22Z and at review start 2026-10-02T22:54:50Z. Host cargo was 0 and running Rust builders were 0 at both launches. One own Rust builder at a time. One own PostgreSQL 17.10. Image `docker.io/library/rust` sha256 `0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` with `RUSTUP_TOOLCHAIN=1.94.0`. Postgres image sha256 `3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Clippy was installed inside the review container and counts as that builder. No second architecture and no whole-workspace test.

## Smoke

Container `42d149c46b4610938945975252687a025ed29d6697e33750f5fc675a9bda7a06`, exit 0, not OOM, not privileged. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, `cargo 1.94.0 (85eff7c80 2026-01-15)`. `cargo fetch --locked` exit 0. `cargo test --locked --offline -p oo-connectors --lib -- --test-threads=1` printed `test result: ok. 49 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s` and `lib_exit=0`. Console lines record `package_live_pg=not-run`. `snapshot_live` was not run. Started 2026-10-02T22:02:22Z, ended 2026-10-02T22:02:51Z.

## Live attempts

The database was not recreated after attempt 1. Later attempts preserved their logs.

| Attempt | Container | Exit | What happened |
| --- | --- | --- | --- |
| 1 | own postgres | bind failed, then recovered | Seed inserts ran after `rev.bump`, so `rev.canary.n` was 3. The canary was reset to 0 before the audit marker and before any `SnapshotOwner`. The log was not truncated. |
| 2 | `…-rust-review` `id` left exited 1 | 1 | Probe compile failed. `req()` is `&str` and six `Ctx` fields were `String`. Clippy was absent from the image. Owner not opened. Database unchanged. |
| 3 | `…-rust-review-2` exited 86 | 86 | `--no-run` exit 0. Clippy exit 101, nine `clippy::needless_borrow` errors on the probe. Oracle skipped (`oracle_exit=86`). A pre-next-launch check still saw label `cedar`, canary 0, and no `a116_extra` column. |
| 4 | `…-rust-review-3` `0b90fc5aebbbe2c23fcdb468508b78b7953c5ba2e36b02c608543b01823ad7cd` | 101 | `product_sha_exit=0`, `compile_exit=0`, `clippy_exit=0` in 0.41s, then the oracle ran 44.83s and panicked at `a116_oracle.rs:331` with the eight names. |

Attempt 4 capacity at 2026-10-02T22:54:50Z: `host_cargo=0`, `rust_builders=0`, `global=0`, slot A. The container was not privileged and published no host port. `lib_retest=not-run` is in `live/console.log`. `live/phases/lib.exit` and `live/phases/fetch.exit` are smoke leftovers on the reused target volume. They are not a second lib run.

The oracle command was `cargo test --locked --offline -p oo-connectors --test a116_oracle -- --test-threads=1 --nocapture`. Package `tests/live_pg.rs`, `tests/lossless_live.rs`, and `tests/snapshot_live.rs` were not invoked.

## Preregistered cut

`expected-outcomes.md` was written at 2026-10-02T22:05:15Z. `live/expected/cut.txt` was written at 2026-10-02T22:22:23Z, after the canary reset and before the owner. The first `oo-pg-source` statement is at 22:54:54.780.

Registered old cut, used by the quiet probes: item 7 cedar, line 21 qty 4, line 22 qty 5 with null memo, line 99 absent, line 23 absent. Writer statements, matched in the log:

- `UPDATE rev.item SET label = 'willow', "odd""note" = 'kept' WHERE id = 7`
- `UPDATE rev."line item" SET qty = 9 WHERE id = 21`
- `INSERT INTO rev."line item" (id, item_id, qty, memo) VALUES (23, 7, 1, 'fresh')`

Fresh tokens after that writer passed on generation 70. The database now also has column `rev.item.a116_extra`. The cut cannot be repeated on this database.

## Oracle outcomes

Header: `author_outcome_reused no`. `identity_drift` and `nested_transaction` are `source_inspected_not_executed`. `session.rs` lines 277–280 return `nested_transaction` from `pub(crate) begin`. The public owner path was not driven into a second `begin`, and the fixture role cannot change `session_user` inside the open transaction. Those two facts are source inspection.

Local checks passed: snapshot protocol `oo.source.pgsql.snapshot/v1`, type protocol `oo.source.pgsql.types/v1`, cleanup bound 5000, connector capabilities unsupported, bounded reader unsupported, debug redaction, config identity stable with password and CA omitted, type-policy identity changes with the policy, digest tracks generation.

Pre-reserve refusals passed and left the `oo-pg-source` count stable: generation 0 `generation_invalid`, past deadline `deadline`, config identity, type-policy identity, selection mismatch. Contract lines 17–19: those refusals do not consume the generation. After reserve, the same source id and generation returns `generation_reused`. The reserve set is process-local.

Quiet generation 41, before the writer, passed binding, privileges, idle pid 369, item and line tokens, absent 99, absent 23, `SnapshotFinished`, release `gone_ms=0`, `generation_reused`, and canary delta 0. `type_conversion` stayed `unsupported` and `converted` stayed false.

Same-cut generation 42 passed binding, idle pid 370, backend start `2026-10-02 22:54:54.94519+00`, writer execution count 3, line 22 tokens, absent 99, absent 23 on the owner, `SnapshotFinished`, release `gone_ms=0`, `generation_reused`, and canary delta 3. Failed marks: `same_item_tokens`, `same_line_21_tokens`, `same_side_absent_23`.

Fresh generation 70 passed item willow, line 21 qty 9, line 22, line 23, `SnapshotFinished`, and release. An autocommit read matched the fresh item and line 23 tokens.

Abort generation 43 passed the fresh item tokens, `SnapshotAborted`, release, and `generation_reused`.

Drop generation 44: `drop_returned_snapshot_finished no`, release `gone_ms=54`, `generation_reused`. The contract says Drop does not confirm rollback and is not the R3 cancel proof. The review observed the backend row gone. That observation is the release check. It does not sign R3.

Deadline generation 45: probe `Budget` / `deadline` passed. `deadline_probe_finish` failed as `Protocol` / `database` / `25P03`. Release `gone_ms=1`. Generation 46: `deadline_finish` failed the same way. Release `gone_ms=0`.

Terminate generation 47 passed match count 1, terminate sent, probe `57P01`, finish `Connect` / `io`, release, `generation_reused`, and a later generation 71 read of the fresh item with `SnapshotFinished`. The terminate predicate required pid, `backend_start::text`, `application_name=oo-pg-source`, user, and database together.

Admission generations 49–54 and `rev_guc` generation 1 passed: `principal`, `database`, `read_node`, `read_node`, inspect `startup_options` with no generation reserve, open `startup_options`, wrong CA `tls_handshake`, wrong SAN host `tls_handshake`. Each released.

Catalog tamper generation 55 passed `catalog_identity` and release. Generation 56 collected `PgSession::catalog`, released that session, revoked table SELECT, then open returned `select_not_granted` and released. The grant was restored.

In-flight table generation 57 passed probe and finish `insufficient_privilege` / `42501`, release, and the restoring grant.

In-flight column generation 58: `inflight_column_probe` failed as `batch`. Abort returned `SnapshotAborted` and released. The grant was restored.

`GRANT INSERT` generation 59 passed `write_capable` and release. The insert grant was revoked afterward.

`ADD COLUMN` generation 60: `add_column_agreement` failed as `batch`, `add_column_finish` failed as `SnapshotFinished`, release passed. Generation 61 passed `catalog_identity` and release.

Inheritance `rev_twin` generation 1 passed shape, probe `probe_key`, finish `probe_key`, and release. The probe SQL is `SELECT id,label FROM rev.twin WHERE id = $1 LIMIT $2`. The child table `rev.twin_more` inherits `rev.twin`, so the statement sees two rows and the owner returns `probe_key`. `compose_probe` does not add `ONLY`.

Empty key generation 62 passed `probe_key` and `SnapshotAborted`, with release. Text key on a fresh owner, generation 63, passed `probe_key` on the probe and on finish, with release.

`FailRead` on `rev_line` generation 1 passed policy identity, line 22 tokens including the null memo, `SnapshotFinished`, and release. Final quiet `rev_line` generation 2, line item only, passed tokens, `SnapshotFinished`, release, and delta 0. Item was not reopened with the pre-alter shape on that generation.

## Why the eight marks stay failed

### Same-cut side session

`live/a116_oracle.rs` `compare` marks success only when the owner token string and the side token string both equal the preregistered old cut. The side session runs `START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY` at 22:54:55.198 and issues no query until 22:54:55.341. PostgreSQL takes the repeatable-read snapshot at the first query. The writer has already committed at 22:54:55.321–325. The side session is a later snapshot.

The owner transaction's first query is `pg_is_in_recovery()` at 22:54:55.062, before those commits. The owner probe of key 23 at 22:54:55.349 returned an empty batch (`same_absent_23_empty pass`). Line 22 was not updated, and `same_line_22_tokens` passed `cut_and_side`. The outcome file does not record the owner token text for item 7 or line 21 on generation 42, so this report does not invent those strings. The fresh generation, opened after `finish`, matched the writer rows.

`same_item_tokens`, `same_line_21_tokens`, and `same_side_absent_23` stay fail.

### Deadline and idle timeout

The probe deadline check returned `fact=deadline` and poisoned the owner. `finish` in `snapshot.rs` lines 457–471 takes the session, calls `release_session`, then returns poison, then calls `ensure_deadline`. Contract line 53 says `SnapshotFinished` requires an accepted rollback, a successful close, an unpoisoned owner, and a deadline that has not passed. Cleanup failure returns that failure.

`sql.rs` lines 7–10 set `idle_in_transaction_session_timeout` from `$1`, the statement timeout. The live bind is `$1 = '20000'`. The oracle waits until 20 seconds after the client deadline. The server log records `FATAL: terminating connection due to idle-in-transaction timeout` at 22:55:16.057 and 22:55:36.336. `error.rs` lines 191–200 map an unlisted SQLSTATE, including `25P03`, to `Protocol` / `database` and keep the SQLSTATE. Both finish results are that error. Neither is `SnapshotFinished`. Both backend rows were gone inside 5000 ms.

`deadline_probe_finish` and `deadline_finish` stay fail because the oracle required `fact=deadline`.

### Column revoke

Contract line 47: `has_*_privilege` is the server's answer, and a denial is `select_not_granted` or SQLSTATE `42501`. `snapshot.rs` line 1046 returns `select_not_granted` when any attribute has `select != Some(true)`.

`REVOKE SELECT (memo) ON TABLE rev."line item"` is at 22:55:38.141. The owner statement `SELECT id,item_id,qty,memo FROM rev."line item" WHERE id = $1 LIMIT $2` follows at 22:55:38.141 and is logged as `execute`, with no permission error. Table-level `REVOKE SELECT` on `rev.item` did deny: `ERROR: permission denied for table item` at 22:55:38.021, and the owner returned `42501` on both probe and finish. The column case returned a batch. Abort then returned `SnapshotAborted` and the backend was gone.

`inflight_column_probe` stays fail.

### ADD COLUMN inside the open transaction

The owner transaction is already open when `ALTER TABLE rev.item ADD COLUMN a116_extra text` commits at 22:55:38.538. The side session's first query, at 22:55:38.539, counts `pg_attribute` and sees the new column. The owner's attribute recheck and probe run in the transaction whose first query preceded the `ALTER`. The probe returned a batch. `finish` returned `SnapshotFinished` and the backend was gone. The next generation, opened with the pre-alter admitted shape, returned `catalog_identity`.

The outcome file stores `batch` for the failed agreement mark and does not store that probe's token text. `add_column_agreement` and `add_column_finish` stay fail.

## SQL audit

The log was not truncated. `A116_AUDIT_WINDOW_START` is at 22:22:22.495, before any `oo-pg-source` statement. `A116_AUDIT_WINDOW_END` is at 22:55:39.348. Attempt 1's canary failure and the canary reset remain before the marker.

Classified counts inside the window, from the outcome file:

- `oo-pg-source` / `oo_rev_reader`: `ROLLBACK=22 SELECT=797 START=25 WITH=2`, forbidden 0, unknown empty
- writer `oo-fixture-writer`: `INSERT=1 UPDATE=2`
- admin: `ALTER=1 GRANT=4 REVOKE=4 SELECT=67 SET=2`
- side `oo-a116-rr-oracle`: `ROLLBACK=2 SELECT=7 START=2`
- `oo_rev_guc`: `SELECT=6`, forbidden 0
- other class empty

`LIMIT` is present. `rev."line item"` is quoted. `"odd""note"` is quoted. Quiet and final canary deltas are 0. The writer phase delta is 3, from the fixture writer. `rev.bump` is `SECURITY INVOKER`.

Copied log: 438706 bytes, sha256 `a0f5eba6733ffd9eca97c792b1942fb44be3d8fbf097dcb1d96066d1c455a1fb`. The in-test counter recorded 438570 bytes. The copied file includes the admin `Connection reset by peer` line after the end marker. A scan of 56 evidence files against the four password files found 0 hits. `PRIVATE KEY` and `BEGIN CERTIFICATE` occur as scanner literals in `live/a116_oracle.rs` and as a rule sentence in `expected-outcomes.md`. No PEM body is in the evidence tree. `secret-scan.txt` records the counts.

## Production locations used

- `snapshot.rs:40` `CLEANUP_OBSERVATION_BOUND_MS = 5_000`
- `snapshot.rs:447` `read_probe` poisons on error
- `snapshot.rs:457` `finish` releases, then returns poison, then checks the deadline
- `snapshot.rs:476` `abort` returns `SnapshotAborted` after successful cleanup
- `snapshot.rs:1026` `recheck`
- `snapshot.rs:1046` column `select_not_granted`
- `snapshot.rs:1055` `compose_probe`, bound `LIMIT` 2
- `snapshot.rs:1361` `release_session` issues rollback and close
- `snapshot.rs:1372` `ensure_deadline`
- `sql.rs:7` session guards, idle timeout bound to the statement timeout
- `error.rs:188` driver SQLSTATE map
- `session.rs:277` nested-transaction guard, source-inspected
- contract lines 17–19 generation, line 31 deadline, line 47 privilege, line 53 cleanup

## Limits

The generation reserve is process-local, as contract line 19 says. Drop's backend absence is the release observation and is not an R3 cancellation proof. `identity_drift` was not executed. Package live tests and `snapshot_live` were not run. The live builder did not rerun the 49 lib tests. Supply remains the a110 `request_changes` result. The eight oracle marks stay failed in `oracle-outcome.txt`.

No push, no pull request, no release, and no plan edit.
