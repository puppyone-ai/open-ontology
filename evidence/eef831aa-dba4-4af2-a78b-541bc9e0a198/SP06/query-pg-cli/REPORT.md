# a198 query-pg command

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a198`. Source commit `4f3f329aa0dabb3f848e6040fdc5379df921c2f9` on baseline `78a60fdeabcada0d024fc6f9a81e0e6b184260e8`.

`query-pg` is a local trusted SQL command. It reads one versioned request file, opens one TLS session to a PostgreSQL endpoint named in that file, loads the current column catalog on that session, and runs the guarded statement as the restricted `NOLOGIN` role. It does not install grants, does not open a source database, and does not take a password from the arguments or the environment.

The policy catalog used by this command is author-approved. It is not independently reviewed. That review is queued and is not this delivery.

## What was added

- `crates/oo-gateway/src/bin/query-pg.rs`
- `crates/oo-gateway/tests/query_pg_cli.rs`
- `contracts/gateway/query-pg-cli-v1.md`
- `crates/oo-gateway/Cargo.toml` pins `serde` `=1.0.229` with `derive` and `serde_json` `=1.0.151` with `arbitrary_precision`
- `Cargo.lock` adds `serde` and `serde_json` to the `oo-gateway` dependency list only

No library, policy catalog, policy, sql guard, query executor, parser worker, control, pipeline, or other manifest file was edited.

## Command

`query-pg --request <path>` reads `oo.gateway.query-pg/v1`. `query-pg --help` exits 0. No arguments, unknown flags, and extra arguments exit 2 with `query-pg: usage`.

The password is a separate regular file. The request is refused if those password bytes appear in the request text. Diagnostics do not print the password, the SQL text, or a DSN. `PGPASSWORD` is not read.

The same connection calls `load_policy_catalog` and then `DedicatedSession::bind`. `identity.role` must be `oo_r_` plus the policy slug. A claims row filter is refused before a connection. The guard runs in process. The public parser worker is not used.

Stdout is one JSON object per row, then one summary. Integer cells are decimal strings. Rows are not collected into one buffer before they are written.

The audit file is created with `create_new` on the first admission. Each admission and each completion is `flush`ed and `sync_all`ed. Admission failure means the user statement is not started. The file is a local receipt. It is not a production audit WAL and it is not SP06-W06.

Exit 0 is only an accepted executor summary whose status is `succeeded`. Truncation with `succeeded` is exit 0. Other request, TLS, policy, guard, audit, and query failures use exits 3 through 8.

## Proof

Pre-edit smoke, before these files existed, on Linux arm64 Rust 1.94: library tests 15 passed, Clippy clean, exit 0. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

Focused proof on the frozen archive plus this overlay, same image, PostgreSQL `17.10 (Debian 17.10-1.pgdg12+1)` image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Container `eeae0db2a0b6b5aa38cbcd26f2622d9374a7d02292418fea4576576a7cafcd43` exited 0, not privileged, not OOM, network `oo-a198-net`.

| Phase | Exit |
| --- | --- |
| libclang load | 0 |
| `cargo check --offline -p oo-gateway --bin query-pg --tests` | 0 |
| lock edge | 0 (`lock_edge=serde,serde_json`) |
| `cargo test --locked --offline --test query_pg_cli` | 0 (5 passed) |
| `OO_A198_LIVE=1` live test | 0 (1 passed) |
| `cargo clippy --locked --offline -D warnings` on the bin and the test | 0 |
| `cargo test --locked --offline -p oo-gateway --lib` | 0 (15 passed) |

The live fixture is database `oo_a198`, not the a190 `billing.invoice` fixture. The test prepares the shadow table and the two roles. The command under test does not. Desk sees `lane, rank, stamp, note, open`. Peek sees `note`. `seal` is absent from the loaded desk columns. Admin typed reads match the command output for the explicit select, `count(*)`, star expansion, and the peek select, including int64 minimum and maximum, the Unicode note, null, and bool. A byte cap returned one row with `truncated: true` and status `succeeded`. Hidden column, `UPDATE`, and `pg_sleep` exited 6 and left no audit file. An existing audit path, a parent that is a regular file, and a garbage CA left `pg_stat_database.sessions` unchanged. A different CA exited 4 and wrote no audit file. Two stdin turns on `oo_r_peek` wrote four audit lines. The row digest that includes `seal` was `b65719fe03dd197cd03088ac44e2d566934461c523d518b623e86b0887675f6c` before and after. `n_tup_ins, n_tup_upd, n_tup_del` stayed `(3, 0, 0)`.

Failed attempts 001 through 007 are under `focused/attempts/`. The final green logs were not rewritten over those copies.

## Not claimed

This does not close a parent SP06 item. It does not authenticate callers. It does not expose an HTTP entry. It does not make the catalog independently reviewed. The fixed review of this command is next.
