# DL01-W1-52 budget connected review

Verdict: approved for the fixed source `6590cff4acc1cb265d0ed05d460b0582251cf360` as the shared native PostgreSQL admission and budget boundary, and as a setup dependency for consumers of that boundary.

This approval stops there. DBX-W02 stays open. The whole source, the DL, integration, release, and any support-matrix grade stay unaccepted. `matrix_mutated` is false. No other engine was promoted.

Reviewer session `4a6650e8-873d-4736-ab9a-680ad3e02a7f`. Author packet `evidence/2ecaf08e-eb23-4b7a-a720-e07459e6f2a6/DL01/budget-report`. Seal `6a732aaae6a27c6cf074e16df3e1cbb8c83cda0d`. Archive base `577160c22de63adb9b38e0147f3bc2da893c4d9d` plus the 32 owned paths at the source commit. Author status `author_complete_review_deferred` is the author's status. It is not this review.

## What was replayed

One Linux arm64 Rust 1.94 builder, 6 GiB, 1 CPU, jobs 1, then one PostgreSQL 17.10, 1 GiB, 1 CPU, then admission runners on the same pinned images with `--pull never`. The resource lease was `fcntl` on `/tmp/oo-dl01-wave1-resources.lock`, held only for the capacity snapshot and the start. The second builder was admitted at 14 of 16 CPUs, global Rust builders 1, host `rustc`/`cargo` 0. A later runner waited on a container die event, then started at 15+1 CPUs.

The producer was `oo-source-admission-pg-budget` on one configured `PgSession`. The policy command was the same build's `oo-source-admission admit-pg-budget`. Passwords stayed in files under `/tmp` and were removed in cleanup. Password scan of the evidence tree hit 0.

PostgreSQL 17.10 (`server_version_num` 170010), TLS verify-full, caller CA only, hostname `oo-dl01w1-52-pg`, negotiated TLSv1.3. `log_statement` is `all` from the configuration file. Roles `oo_dl01w1_38_reader`, `oo_dl01w1_38_dba`, and `oo_dl01w1_52_writer` are login roles with superuser, createdb, createrole, replication, and bypassrls unset. DBA setup ran as the superuser before statement logging. The DBA lock and the later superuser `pg_terminate_backend` of that DBA are a separate identity from the platform reader.

## Admission result

The fresh evidence is `oo.source.pgsql.budget/v1`. Identity is known and aligned for `oo_dl01w1_38_reader`. Permission is known and read-only on `app.item`. Compat mode is `postgres-native` and known. Shadow is `none`. Target is `postgres-17-native`. Driver is in-process `sqlx-postgres` 0.9.0 feature `_tls-rustls-ring-no-roots`, digest `d510f5df0eddeb651367bbdf62f30b795c0d747f379104d990f307f8fd0664a5`. Cost model is `oltp_accounting` with units Rows, Bytes, WallClock, estimate confidence unknown, estimate basis not_collected, billing none. The companion sha256 matches the legacy access bytes, protocol `oo.source.pgsql/v1`.

`admit-pg-budget` on those bytes exited 0 twice. The decision admits Catalog, BatchFull, and Budget Rows, Bytes, and WallClock at designed / R0 / S0. `matrix_mutated` is false. `zero_write.proven` is false and `zero_write.basis` is `not_audited`.

`adapt-pg` on the legacy access report exited 2 with `BUDGET_UNIT_UNKNOWN`.

Gate read printed `rows=2` only after admit exited 0. A stub admit that exits 2 returned `admit_exit=2` and printed no `rows=` line. Raw half-ready exit is 2.

Rows exhaustion raw exit is 0, `fact=max_rows`. The producer exhaust phase asserts `max_rows` only, so the bytes cap is the raw exit 2 with `fact=max_bytes`. That raw exit is preserved. Wall-clock timeout under a granted DBA `ACCESS EXCLUSIVE` lock raw exit is 0, `fact=statement_timeout`. This review's lock count is 1 and `lock-check.exit` is 0. `pg_isready` exit is 0.

Mutated copies of the producer bytes, each admitted false:

| case | raw exit | codes |
| --- | --- | --- |
| identity unknown | 2 | IDENTITY_MISSING, IDENTITY_MISMATCH, PERMISSION_UNKNOWN |
| identity missing | 2 | IDENTITY_MISSING, IDENTITY_MISMATCH, PERMISSION_UNKNOWN |
| config user mismatch | 2 | IDENTITY_MISMATCH |
| rows client cap mismatch | 2 | BUDGET_UNIT_MISMATCH |
| WallClock cap removed | 2 | BUDGET_UNIT_UNKNOWN |
| statement_timeout setting 1 | 2 | SERVER_SETTING_MISMATCH |
| R2 | 2 | WRITE_CLASS_R2 |
| R3 | 2 | WRITE_CLASS_R3 |
| R1w without approval | 2 | R1W_UNAPPROVED |
| missing evidence file | 1 | invalid input |

Producer cancel-before-`next_batch` printed `cancelled=38` and exited 0. That generation token is not a backend pid.

## Audit and cancel

`audit-log` on the journey csvlog exited 0. Reader `oo_dl01w1_38_reader` has 181 statements, forbidden 0, verbs SELECT, START, WITH, and ROLLBACK, `other_users_with_forbidden` 0, basis `pg_csvlog`, proven true. The same decision document still says `zero_write` is not audited. Those are separate results.

The in-flight cancel test `pg_reader_cancel_live` exited 101. Every phase note was written before the panic, including `receiving_fact=cancelled`, `dropped_future_backend_still_active`, `deadline_fact=deadline`, and `old_run_not_finished` after the admin terminate (`57P01`). The panic is `reader sql log was empty` in the test's own scanner, which looks for `app=` text. The csvlog has no such text. Field parsing of that log shows reader `pg_cancel_backend` 3 times and reader `pg_terminate_backend` 0 times. Postgres, the separate superuser, issued `pg_terminate_backend`. The writer role inserted one row. The reader also issued 6 `SAVEPOINT` and 5 `RELEASE SAVEPOINT`. Those two verbs sit outside the audit allow-list, so this cancel log would not receive basis `pg_csvlog`. They are session transaction markers. The reader's statement verb is not INSERT, UPDATE, DELETE, or DDL.

## Build

Attempt 1 is preserved under `linux/build/attempt-1-clippy-missing/`. Connector tests exited 0 (69), admission tests exited 0 (6 library, 6 budget gate, 17 CLI), and the pg-budget build exited 0. Offline connector resolution exited 101 because `chrono-tz` was absent from the empty cache. Clippy then exited 1 because the 1.94 toolchain image had no clippy component. `build.exit` is 1. The journey did not start. `cargo update` was not run.

Attempt 2 installed the clippy and rustfmt components on that same 1.94 toolchain, then reran the locked tests and clippy. `build.exit` is 0. Connector tests 69 passed. Admission tests 6, 6, and 17 passed. Clippy `--locked --no-deps -- -D warnings` exited 0 for the connector library, the admission crate, and the pg-budget crate. `cargo test --test pg_reader_cancel_live --no-run` exited 0. Offline exits on attempt 2 are 0 because the private cache from attempt 1 was still present. The attempt 1 exit 101 remains the failed offline attempt. `rustfmt --check` exited 1 on import layout in existing connector and admission files. That check does not gate this verdict. Clippy did.

Builder inspect: exit 0, OOMKilled false, 1 CPU, 6 GiB. Cleanup removed only `oo-dl01w1-52-*` containers, the volume, the network, and the secret directory.

## Provenance

Inventory of the author `SHA256SUMS` is 285 of 285 blobs, with the sums file itself the only content path absent from the list. Author `lock-granted.exit` is still 0. `lock-granted-retry.exit` and `lock-released.exit` are still 1. `pg-ready.exit` is still 1. Those author flags were not rewritten and were not treated as success.

Binaries, measured in this review: see `linux/binary-identity.txt`.
