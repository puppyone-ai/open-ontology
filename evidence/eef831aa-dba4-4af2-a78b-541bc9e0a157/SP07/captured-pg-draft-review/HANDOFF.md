# Handoff

Verdict: **APPROVE LIMITED** for `compile_captured_pg_catalog` on commit `63466471a6d403e0a2a29c8d1ae064179da65bd8`, tree `1a6c65addb3897fe93159560b8d295949eb2542c`, parent `377707e88b13b2b77baa6f619c2c1b364f0dab28`. Author evidence `7539fe8f`.

Public entry: `oo_builder::compile_captured_pg_catalog(catalog_json, curated_json, reviews_json) -> Result<CapturedPgDraft, DraftError>`. Contract: `contracts/builder/captured-pg-draft-v1.md`.

Treat `provenance.pg_catalog_integration = not_accepted` and commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` as the historical compiler contract. Treat `wrapper.live_catalog_review = pending`, `row_import = not_proven`, and `grants = not_authorized` as the capture state. A caller decision inside the structure draft does not change those wrapper fields. SP07 stays open. a086 (`eebc0ea3b835655fdb3ab088e6f7351d0d316a43`, findings 3 and 4) and a094 (`3ed558a9b62ab3f290ce9674b9afc10e693a5730`, findings 1, 2, and 5) stay limited facts. This review did not rerun those matrices.

## Reproduce

Archive `63466471` with `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `.cargo`, `examples/ontology-draft`, and `contracts/builder`. `RUSTUP_TOOLCHAIN=1.94.0` on image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. Run `cargo test -p oo-builder --lib`, `cargo clippy -p oo-builder --lib --test captured_pg -- -D warnings`, `cargo test -p oo-builder --test structure_draft_cli`, and `cargo test -p oo-builder --test captured_pg`. Install clippy inside the container. Do not rustfmt `draft.rs`.

The live probe archives the same commit, copies `probe/` into that snapshot, and inserts the snapshot workspace member with `awk`. The snapshot mount must be writable so Cargo can add package `a157-captured-pg-review` to the snapshot lock. The repository lock stays unchanged. PostgreSQL image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, server 17.10, database `oo_a157`, reader `oo_a157_reader`. One rust builder at a time. The probe binary calls `PgConnector`, then `compile_captured_pg_catalog`. `probe/src/bin/sql_audit.rs` classifies `statement:` and `execute <name>:` lines. `START` is a read. Exit 2 means the reader log had zero `execute` lines.

`probe/attempt-1` is the read-only mount failure (cargo 101). The green run is `probe/out/` and `audit/`.

Author `pg/audit.py` under the a156 evidence tree is Python. Leave it in place.

## Cleanup names

Only a157 objects are removed after the evidence is assembled. Nothing named a154 is included.

Containers: `oo-a157-builder-smoke`, `oo-a157-probe`, `oo-a157-pg`.

Volumes: `oo-a157-eef831aa-cargo`, `oo-a157-eef831aa-target`, `oo-a157-eef831aa-pglog`, `oo-a157-eef831aa-pgdata`.

Network: `oo-a157-eef831aa-net`.

Temp: `/tmp/oo-a157-eef831aa` and `/tmp/oo-a157-eef831aa-dba4-4af2-a78b-541bc9e0a157`.

The result of that removal is `cleanup.txt`. No prune and no foreign container removal.
