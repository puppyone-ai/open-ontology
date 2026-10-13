# Findings

1. The fixed tree at `6590cff4acc1cb265d0ed05d460b0582251cf360` admits a fresh PostgreSQL 17.10 budget report from the same session that then reads. Known-same admit exits 0 twice. Legacy `adapt-pg` exits 2 with `BUDGET_UNIT_UNKNOWN`. Evidence: `linux/journey/raw/gate-read.exit`, `admit-same-1.exit`, `admit-same-2.exit`, `adapt-pg.exit`, `evidence.json`, `decision.json`.

2. Decision `zero_write.proven` is false and the basis is `not_audited` on the admitted decision. The journey `audit-log` result is a different document: 181 reader statements, forbidden 0, basis `pg_csvlog`, proven true. Evidence: `linux/journey/raw/decision.json`, `audit.stdout`, `linux/journey/csv-census.json`.

3. Rows hard-bound exhaustion returned `fact=max_rows` and raw exit 0. Bytes hard-bound exhaustion returned `fact=max_bytes` and raw exit 2, because `budget_main.rs` labels every exhaust phase as `max_rows`. The raw exit is kept. Evidence: `exhaust-rows.stdout`, `exhaust-rows.exit`, `exhaust-bytes.stdout`, `exhaust-bytes.exit`.

4. Wall-clock exhaustion under a real DBA `ACCESS EXCLUSIVE` lock returned `fact=statement_timeout` and raw exit 0. Lock count is 1 and `lock-check.exit` is 0. The unlock is superuser `pg_terminate_backend` of the DBA only. `pg_isready` exit is 0. This review did not write `pg-ready.exit`. Evidence: `timeout.stdout`, `lock-granted.count`, `lock-check.exit`, `unlock.exit`, `pg-isready.exit`, `pg-ready.flag`.

5. Half-ready held. The stub admit exited 2, the producer printed `admit_exit=2`, and it printed no `rows=` line. Evidence: `half-ready.stdout`, `half-ready.exit`.

6. Identity, settings, unit, and budget mutations of the producer bytes were denied, as were R2, R3, and unapproved R1w. Unknown and missing identity also carry `IDENTITY_MISMATCH` and `PERMISSION_UNKNOWN` beside `IDENTITY_MISSING`. Admitted is false in each case. A missing evidence file exits 1. Evidence: `admit-*.exit` and `admit-*.stdout`.

7. Caller cancel reached the backend. `pg_reader_cancel_live` recorded `receiving_fact=cancelled` from the caller token, left the backend active when the next-batch future was dropped, and refused to finish the old run after the admin terminate. The csvlog field parse shows reader `pg_cancel_backend` 3 and reader `pg_terminate_backend` 0. The test process still exited 101. Its audit scans for `app=` and `user=` text and then panics with `reader sql log was empty`. That panic is preserved. It does not erase the phase notes or the csv counts. Evidence: `cancel-live.exit`, `cancel-live-outcome.txt`, `cancel-live-docker.log`, `retained-cancel.csv`.

8. On the cancel log the reader issued 6 `SAVEPOINT <ident>` and 5 `RELEASE SAVEPOINT <ident>`. The audit allow-list does not include those verbs, so an `audit-log` of that file would use basis `audit_write`. The statements are transaction markers. The reader's first verb is not in the forbidden DML/DDL set. Writer `INSERT`, postgres `DROP`, and postgres `LOCK` belong to other users. Evidence: `linux/journey/csv-census.json`.

9. Attempt 1 failed closed. Offline `cargo test -p oo-connectors --lib --locked` exited 101 looking for `chrono-tz`. After the locked online tests passed, clippy exited 1 because component `cargo-clippy` was not installed for `1.94.0-aarch64-unknown-linux-gnu`. `build.exit` is 1 and the journey was not started. Evidence: `linux/build/attempt-1-clippy-missing/`.

10. Attempt 2 clippy on the same toolchain exited 0 with `-D warnings`. `rustfmt --check` exited 1. The diffs are import layout in `crates/oo-connectors/src/lib.rs`, `sql.rs`, and admission `main.rs`, `lib.rs`, `admit.rs`, and `pg_adapt.rs`. `budget_report.rs` and `budget_adapt.rs` produced no diff hunk. Rustfmt is recorded and does not change the verdict. Evidence: `linux/build/raw/rustfmt-*.exit`.

11. Author preserved flags are unchanged: `lock-granted.exit` 0, `lock-granted-retry.exit` 1, `lock-released.exit` 1, `pg-ready.exit` 1. Inventory mismatches are 0 across 285 sums entries.

12. No ledger, author packet, or production source file was edited. Cleanup removed only this task's containers, volume, network, and secrets.
