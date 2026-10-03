# REPORT — a130 reader-primitives functional review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a130`. Reviewer Grok 4.7, in process, no subagent and no worktree.

Reviewed product commit `ccd93e1adc34b24ab998d90972549f8cffb59f0d`.
Tree `ef43877346afadcdccaf26fe6982c6b4e8b65902`.
Parent `182702e6e67d16de10f280eaf422fce4e2f5a5f2`.
Schema ancestor `2254eab39c14a1902e65e72eb3674debc1c54f03`.
Workspace HEAD at linkage time was `61d2898c03c4dca726ec6cfaca9ed50f038d2dbb`. It was not the compile root.

## Verdicts

| Slice | Verdict |
| --- | --- |
| Schema and current catalog | APPROVE |
| Catalog rate | APPROVE |
| Named portal | APPROVE |

Driver integration gate: open. See FINDINGS F5. R1, the whole reader, Arrow, R3, R4, import, CLI, harness, and extensions stay open. The 87-item goal is unchanged. No checkbox was edited. No product file was edited.

## Images and toolchain

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, pull never.
`rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, cargo `1.94.0`.
`rust-toolchain.toml` in the archive says channel `1.98.1`. Every container set `RUSTUP_TOOLCHAIN=1.94.0`, and `inside` scripts refused any other rustc or host.

PostgreSQL image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.
`pg/init.out`: `server_version=17.10 (Debian 17.10-1.pgdg12+1)`.

Rust host config for the passing proof (`linux/proof/hostconfig.txt`): privileged false, `CapDrop=ALL`, `CapAdd` empty, `no-new-privileges`, memory and swap 8589934592, `NanoCpus=4000000000`, pids 4096, network `eef831aa-dba4-4af2-a78b-541bc9e0a130-net`, `/src` read-only, `/secrets` read-only, `OOMKilled=false`, exit 0. `HOME=/tmp`. No docker socket, no host network, no host HOME, no shared cargo cache. Passwords stayed in mode `0600` files under `/tmp/oo-a130-eef831aa-dba4-4af2-a78b-541bc9e0a130/secrets` and were not passed on argv. Those files are not in this evidence tree.

Capacity files for smoke, author, and each proof launch record `running_rust_image_or_label=0`, `own_rust_builder_running=0`, `host_cargo_rustc_rustdoc_rustup=0` immediately before `docker run`. One PostgreSQL container ran beside the single Rust builder. Global builder count stayed under 2.

## Commands and exits

Smoke, archive of `ccd93e1a`, owned cargo and target volumes:

- `cargo fetch --locked` — `fetch.exit` 0
- `cargo test --locked --offline -p oo-connectors --lib -- --test-threads=1` — `lib.exit` 0, 55 passed
- `cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings` — `clippy.exit` 0
- `docker-run.exit` 0, `toolchain.exit` 0, `snapshot.exit` 0

`sqlx-postgres` printed two unfulfilled `dead_code` lint expectations. They did not fail the `oo-connectors` clippy gate.

Author, same archive, unmodified tests, `--test-threads=1 --nocapture`, one private database `snapdb`:

| Test | Exit | Observed |
| --- | --- | --- |
| `catalog_rate_live` | 0 | `big_elapsed=176.664917ms`, `burn_elapsed=673.675916ms`, `reader_writes=0`, 1.43s |
| `snapshot_live` | 0 | `schema_inflight=schema_drift`, `privilege_inflight=insufficient_privilege`, `same_cut_hash=1ff301767c85e585b52df42ae0ab47d1cbfe6c15205b6a0fcef98605badd8236`, 14.39s |
| `pg_portal_bounds` | 0 | 6 passed, 26.51s |
| `pg_portal_live` | 0 | `reader_forbidden=0`, `snapshot_id=0000000F-00000019-1`, 0.54s |

`linux/author/docker-run.exit` is 0. Author `postgresql.log` first byte is not NUL (`linux/author/log-prefix.txt`) and `log_leak=no`.

Proof, same archive plus `probe/a130_review.rs` copied to `crates/oo-connectors/tests/a130_review.rs` only inside the ephemeral snapshot:

`cargo test --locked --offline -p oo-connectors --test a130_review -- --test-threads=1 --nocapture`

Passing run `linux/proof`: `toolchain.exit` 0, `snapshot.exit` 0, `a130_review.exit` 0, `docker-run.exit` 0, `inspect.exit` 0. Four tests passed in 5.45s. Container elapsed 8s. Product file hashes in the console are OK, including `buffered.rs`.

```
t1_schema_guard ... same_cut=ok index_rename=ok add_after_probe=schema_drift
  zero_probe_finish=schema_drift cap_refusal=max_connections generation_retained=3
  cleanup_rv_finish=both_backends_gone cleanup_rv_abort=both_backends_gone
  cleanup_rv_drop=both_backends_gone guard_terminate_fact=io
t2_catalog_rate ... small_elapsed=170.382417ms big_elapsed=195.07125ms
  sql_limits_match_reservation=ok keyword_limit=80
  failed_query_keeps_charge=ok burn_elapsed=677.256ms
t3_named_portal ... portal_error=22012 portal_cancel=57014 consumed_rows=0
  portal_drop_poisons=ok snapshot=00000021-00000017-1
t4_sql_audit ... reader_statements=686 reader_forbidden=0
  writer_statements=7 admin_statements=64
```

Earlier proof exits, preserved and not rewritten:

| Path | `a130_review.exit` | `docker-run.exit` |
| --- | --- | --- |
| `linux/proof/attempt-1` | 101 | 101 |
| `linux/proof/attempt-2` | 101 | 101 |
| `linux/proof/attempt-3` | 101 | 101 |

Attempt 1 and 2 failed in `rustc` before a test ran. Attempt 3 ran and failed two overlay assertions. FINDINGS F6 records the compiler and panic text. `buffered.rs` was not modified between attempts.

## SQL log

`linux/proof/postgresql.log` is the collector file. Rotation did not replace it. The independent audit is the suffix after the last `a130_own_log_cut` marker written by this proof's `t1`. `linux/proof/sql-audit.txt` matches a second classifier: 686 reader statements, zero reader `INSERT`/`UPDATE`/`DELETE`/`MERGE`/`CREATE`/`ALTER`/`DROP`/`TRUNCATE`/`GRANT`/`REVOKE`/`COMMIT`/`COPY`, 7 writer statements, 64 admin statements. `log_leak=no`. First byte of the file is 50.

## Bounds of this run

No C or Python product harness. No new dependency. No dual architecture, release, full matrix, or 60000-row read. The 60000-row panic recorded by a128 attempt 3 remains assigned to a129. Author `snapshot_live` and the lib suite carry type conversion, canonical values, CA verification, and role admission. This review did not rerun `live_pg` or `lossless_live`.
