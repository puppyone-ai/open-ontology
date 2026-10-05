# Mapped shadow statistics

Session `eef831aa-dba4-4af2-a78b-541bc9e0a219`. Acceptance id a219. Plan card M1-D06A / SP04-W02. Product commit `613aa981dc2c57c0005cc6589e1a8ee7757f2a69`, parent `5af5db7e9bc27a32bacb4d617bd2393c8edc2cb6`. Fixed archive `c2db9c430b822a824a1062b6c3c7345924df7dd8`, tree `8fb187147b02d6cacf45cb5a683859f698b497cf`, saved before the pre-edit smoke.

The owned product paths are `crates/oo-pipeline/src/target_statistics.rs`, the `pub mod target_statistics` export in `crates/oo-pipeline/src/lib.rs`, `crates/oo-pipeline/tests/target_statistics_pg_live.rs`, and `contracts/pipeline/target-statistics-v1.md`. `Cargo.toml` and `Cargo.lock` were not edited. The overlay observed lock digest `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. No import function, import CLI, or source-database `ANALYZE` was added.

This run does not accept a parent item, M1-I01, M1-I02, or a release. The receipt sets `published` false and `estimate_is_exact` false.

## What the call does

`analyze_mapped_shadows` takes one target connection, the mapping project, source, config digest, and source generation, and at most eight logical tables. It refuses a closed identity, an empty or duplicate list, and a timeout outside 1..=30000 ms before it uses the connection. It refuses an open caller transaction, a runtime member of `oo_pipeline_runtime`, and a session whose `transaction_read_only`, `default_transaction_read_only`, or `pg_is_in_recovery` is on.

Inside one maintenance transaction it sets local `statement_timeout`, `lock_timeout`, and `search_path=pg_catalog`. It requires the current mapping generation, a stored `pt_` name, an ordinary table in `_oo` owned by `oo_target_owner`, and `MAINTAIN` on that relation. `ANALYZE` uses `quote_physical_name` on the stored identifier. `reltuples` of `-1` is `EstimateUnknown`. Any other negative, NaN, or infinite value is `EstimateUnusable`. A finite value of at least zero is rounded half away from zero into `estimated_rows`, and the receipt stores `f32::to_bits`.

`55P03` is `LockTimeout`. `57014` is `StatementTimeout`. The live timeout case is the lock timeout. Rollback failure, a still-open transaction after rollback, a failed disposition probe, and SQLSTATE `25P02` report `FreshConnection`. A refusal before `BEGIN` leaves the connection reusable. The call does not roll back a transaction it did not open.

The idle probe is one simple query, `statement_timestamp() > transaction_timestamp()`. The extended protocol stamps the transaction at Bind and the statement at Execute, so the same comparison is true on an idle connection.

## Host and images

Linux arm64 Rust 1.94.0 (`4a4ef493e 2026-03-02`), image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL 17.10 image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Container `oo-a219-pg` id `01a2ba652c500fc7d5d5d75db187963e6e323dd1a96d43610f3a33635b307590`, alias `oo-a219-pg`, SAN `DNS:oo-a219-pg`, no host port. `up.exit` is 0.

Each rust container had 6 GiB, 1 CPU, all capabilities dropped, and bridge network `oo-a219-eef831aa-net`. Postgres had 1 GiB and capabilities CHOWN, DAC_OVERRIDE, FOWNER, SETUID, and SETGID. One rust builder at a time. One postgres. The green launch recorded `own=0 global=0 host_cargo=0`. Private cargo, target, CA, and password files lived under `/tmp/oo-a219-eef831aa-dba4-4af2-a78b-541bc9e0a219`. The builder mount listed `admin.pass`, `reader.pass`, `runtime.pass`, `readonly.pass`, `login.pass`, and `ca.crt`. It had no private key.

Recipe origins and the path edits are `recipes/ORIGIN.md`.

## Commands and exits

Pre-edit smoke, container `oo-a219-preoverlay` id `fa99a8133e1a859e44cd12f2c6aa17df38f1ad19201f685178e7d97aef6a5011`, docker exit 0, `OOMKilled=false`. The fixed archive had no `target_statistics.rs`. `linux/preoverlay/console.log`.

| Phase | Exit |
| --- | --- |
| `cargo fetch --locked --offline` | 101 |
| `cargo fetch --locked` | 0 |
| `cargo test -p oo-pipeline --lib` focused name | 0 |
| clippy component | 0 |
| `cargo clippy -p oo-pipeline --lib -- -D warnings` | 0 |

Overlay attempts 1 through 6 are preserved under `linux/overlay/attempt-N/` with their notes. The green run is `linux/overlay/`. Container `oo-a219-overlay` id `2bf60f92d340187b20d951d225ee92799ebcfee598c62083c7895fd86aa7c885`, docker exit 0, `OOMKilled=false`, memory 6442450944, NanoCpus 1000000000.

| Phase | Exit |
| --- | --- |
| overlay present | 0 |
| sha256 tool, select-debs tool, manifest tool, owned hash | 0 |
| `cargo test --locked --offline -p oo-pipeline --lib target_statistics` | 0, 2 passed |
| `cargo test --locked --offline -p oo-pipeline --test target_statistics_pg_live -- --ignored` | 0, 1.97s |
| clippy component | 0 |
| `cargo clippy -p oo-pipeline --lib --test target_statistics_pg_live -- -D warnings` | 0 |
| libclang extract | 0 |
| `cargo test -p oo-gateway --test target_statistics_catalog_proof -- --ignored` | 0, 0.14s |
| secret scan of the two fact files | 0 |
| live sha256 manifest | 0 |

The gateway test file was copied into the archive only. It is not a product path. `source/blobs/` holds the four product files and that harness. `source/blobs.txt` holds the git blob ids.

## Measured facts

`linux/overlay/live/facts.txt`: TLS on, server version 17.10, import state `awaiting_publish`, import rows 2, `reltuples` before maintenance `-1`, sentinel `-1`. Refusals: open transaction, stale generation, missing table, runtime caller, readonly caller, then after maintenance lock timeout and forged relation. After the refusals that ran before `ANALYZE`, `reltuples` was still `-1`. The receipt has `published=false`, `estimate_is_exact=false`, `reltuples_after_bits=1073741824` (finite 2.0), `estimated_rows=2`, `count_rows=2`, connection reusable, sentinel still `-1`. Source audit delta 0, reader writes 0, source `reltuples` still `-1`.

`linux/overlay/live/catalog-facts.txt`: pipeline unknown before the maintenance call, catalog `reltuples` known after it, rounded estimate 2, policy catalog loaded, `unknown_rows=refuse`, query status succeeded, query count 2, classification `operator_label`, `statistics_published=false`.

`linux/overlay/live-sha256.txt`:

```
ba353f5f4ea08289ee19ebbb1d47b6949355a322134dd3c9e3de1c14548e7da9  ./catalog-facts.txt
e2efee80b05e9180c1bd4e510bd9e5e0c8ac209e37edaa22d1b43f8dc363ba1b  ./facts.txt
```

## Evidence and cleanup

Raw exits are the `*.exit` files under `linux/preoverlay/` and `linux/overlay/`, including each `attempt-N/exits/`. Host config, mounts, and consoles are beside them. `SHA256SUMS` is `tools/sha256.rs` over this directory, skipping that file. `cleanup/run.log` removes `oo-a219-pg`, volumes `oo-a219-eef831aa-cargo`, `oo-a219-eef831aa-pgdata`, and `oo-a219-eef831aa-target`, network `oo-a219-eef831aa-net`, and `/tmp/oo-a219-eef831aa-dba4-4af2-a78b-541bc9e0a219`. The pinned images stayed. `oo-a222-pg` was still running afterward. No prune.
