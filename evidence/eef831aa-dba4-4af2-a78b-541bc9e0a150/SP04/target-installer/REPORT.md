# a150 target installer

Session `eef831aa-dba4-4af2-a78b-541bc9e0a150`. Source commit `4ed22b8c1189e72b06b20a45335860ac9a3ea44e`, tree `fafb0fe6752b8240eb2b29025fc38bccdfb7144d`, parent `0791e971f49cd8afc646d80179241ddbcfaa1a1c`. Fixed base `16ca2969cd449653a64d98d9831d3c8b8976942a` is an ancestor. SQL `0000` through `0004`, the connector, the COPY encoder, physical mapping, and the root `Cargo.toml` are unchanged from that base.

`oo_pipeline::target_installer` installs the frozen target schema on a caller-owned `sqlx::PgConnection`. The public entry points are `install` and `verify_readiness`. The crate embeds `0000`, `0001`, `0003`, and `0004` with `include_str` and stores SHA-256 of those bytes. `0002` is not embedded and is not executed. A stored `target-fence-receipts/2` row is accepted only when its checksum is `59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512`.

Fresh install runs `0000` as the administrator, then `0001`, `0003`, and `0004` as `oo_target_owner`, and inserts `target-bootstrap/1`, `target-fence-receipts/1`, `target-fence-receipts/3`, and `physical-mapping/v1` in that transaction. A valid v1 catalog goes to `0003` then `0004` and does not insert a v2 marker. A valid v2 catalog takes those same two scripts and leaves the historical rows in place. Checksum mismatch raises `OO001` and rolls back. A non-fresh catalog missing `_oo.apply_receipts` or `apply_receipts_segment_check` runs `0003`, which raises `OO002`, and no marker is inserted. `verify_readiness` compares stored rows with the compiled digests. It does not install, `SET ROLE`, or take the migration lock.

This commit is the schema installer. The live database had no publication and zero receipt rows after a fresh install.

## Images

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. Both were local. Pull was `never`.

## Smoke, before the product edit

Archive of `16ca2969`, tree `0466e01e2794716f1d6bde65ba4ea82d39b88d1b`. Workspace head at launch `0607293ad17c6c157e054355fdda480c4b587fc8`. `archive_hashes_match=1`. Package lib tests `ok. 7 passed; 0 failed`. Scoped Clippy `-D warnings` exit 0. `docker-run.exit=0`. Elapsed 39 seconds. Container `oo-a150-smoke`, network `oo-a150-eef831aa-net`, cap-drop ALL, memory 8589934592, privileged false, OOMKilled false. Raw log `linux/smoke/console.log`.

## Probe, working tree

Workspace head at launch `0791e971f49cd8afc646d80179241ddbcfaa1a1c`. The mounted tree contained the installer sources. Their SHA-256 values in `linux/probe/source-sha256.txt` match `git show` of `4ed22b8c` for the seven committed paths, and match the published SQL digests for `0000` through `0004`.

`cargo fetch --locked` exit 0. The lock change is the `oo-pipeline` dependency list only: `sha2 0.10.9`, `sqlx`, `sqlx-core`, `tokio`. No other lock package moved. `cargo update` was not run.

Green phase exits, all 0: toolchain, clippy-component, fetch, lib, embed, clippy, live, docker-run. Lib result on that run: `ok. 9 passed; 0 failed`. Embed test `installer_embeds_production_scripts_and_not_v2` passed. Clippy was `cargo clippy -p oo-pipeline --all-targets -- -D warnings`. Live test `production_installer_on_one_postgres` passed in 0.45 seconds. Container `oo-a150-probe`, same network, cap-drop ALL, memory 8589934592, privileged false, OOMKilled false. One PostgreSQL, databases `oo_a150_empty`, `oo_a150_v1`, `oo_a150_v2`, `oo_a150_miss`, `oo_a150_bad`, and `oo_a150_fail`, in that order. Password stayed in a file under `/tmp` and was not present in this evidence tree.

## Live oracle

`linux/probe/live/outcome.txt`, also printed in `linux/probe/console.log`:

```
server=PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1) on aarch64-unknown-linux-gnu, compiled by gcc (Debian 12.2.0-14+deb12u1) 12.2.0, 64-bit
empty=bootstrapped
empty_replay=replay
empty_applied_at_same=1
empty_v2_rows=0
runtime_verify=ok
runtime_install=not_administrator
v1=upgraded_from_v1
v1_v2_rows=0
v1_applied_at_same=1
v2=upgraded_from_v2
v2_history_same=1
v2_receipt_same=1
oo002=OO002
oo002_unchanged=1
oo002_ready=incomplete
oo001=OO001
oo001_unchanged=1
fail=OO021
fail_unchanged=1
fail_ready=incomplete
installer_live_ok
```

The fresh catalog assertions before that report line checked logged relations `apply_receipts`, `migration_lock`, `physical_mappings`, `schema_migrations`, and `target_generations` (`relpersistence` `p`), functions `adopt_target_generation`, `commit_apply_receipt`, `lookup_physical_mapping`, `register_physical_mapping`, `schema_ready`, and `segment_preflight_ready`, and both product roles with login, superuser, createdb, createrole, inherit, bypassrls, and replication all false. Publication count was 0. Receipt count was 0. Compiled digests on the readiness token matched the embedded scripts. Replay left `applied_at` unchanged.

## Failed attempts kept

`linux/probe/attempt-1/` is the first probe. Lib and embed passed. Clippy exit 101: unused `ABSENT_MESSAGE`, and `clippy::collapsible_if` in `checksum_mismatch`. `docker-run.exit=101`.

`linux/probe/attempt-2/` is the second probe. Clippy exit 0. Live exit 101. Bootstrap had already committed. The role query returned boolean text `false`, and the harness expected `f`. The harness expectation was changed to the server text. The third probe is the green one above.

## Dependency pins

`sqlx` `=0.9.0`, default features off, `runtime-tokio` and `postgres`. `sqlx-core` `=0.9.0`, default features off, `_rt-tokio` and `_tls-rustls-ring-no-roots`, through the existing workspace patch. `sha2` `=0.10.9`, default features off, `std`. `tokio` `=1.53.1`, default features off, library feature `time`, dev features `macros`, `rt-multi-thread`, and `time`. No webpki roots and no native roots were enabled.

## Cleanup

`CLEANUP.txt`. a150 containers, network, volumes, and the private `/tmp` directory are gone. `docker prune` was not run.
