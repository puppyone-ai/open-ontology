# Handoff

Verdict: **APPROVE LIMITED** for `oo-builder captured-pg-draft` on commit `3353380b03a5585a349fddd0038d0a79877c51e1`, tree `5d6767362bedea26d673b43b932a7776f2ecfc25`, parent `8adb9c3680b0816195fead6a698bc6a704ac751b`. Author evidence `2996562ceb332cc0ce505c703e775615fa321e5b`.

Public command: `oo-builder captured-pg-draft`. One `oo.source.pgsql/v1` catalog on stdin. No arguments. The process calls `compile_captured_pg_catalog(input, None, None)`. Stdout protocol `oo.builder.captured-pg-draft-command/v1`. Contract: `contracts/builder/captured-pg-draft-v1.md`.

The approved API underneath the command remains `oo_builder::compile_captured_pg_catalog` from a157 commit `63466471a6d403e0a2a29c8d1ae064179da65bd8`. Library and lock blobs are unchanged from that commit.

Treat `provenance.pg_catalog_integration = not_accepted` and commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` as the historical compiler contract. Treat `wrapper.live_catalog_review = pending`, `row_import = not_proven`, and `grants = not_authorized` as the capture state. The evidence envelope keeps the captured owner and privilege records. The draft and the wrapper do not copy the owner. SP07 stays open. This command does not approve an agent dialog, a gateway, or a row import.

No-argument stderr still begins with the line from `crates/oo-builder/src/bin/oo-builder.rs:13`:

```rust
eprintln!("oo-builder: process entry is not implemented (SP00-W01 skeleton)");
```

## Reproduce

Archive `3353380b` with `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `.cargo`, `examples/ontology-draft`, and `contracts/builder`. Copy `oracle/a160_oracle.rs` to `crates/oo-builder/tests/a160_oracle.rs` in that snapshot. Copy the a157 catalog to `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out/catalog-before.json` and require SHA-256 `de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9`. Set `A160_ORACLE_DIR` to a writable directory. `RUSTUP_TOOLCHAIN=1.94.0` on image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. Run:

```
cargo test --locked -p oo-builder --lib -- --test-threads=8
cargo test --locked -p oo-builder --test a160_oracle -- --test-threads=1
cargo test --locked -p oo-builder --test structure_draft_cli -- --test-threads=4
cargo test --locked -p oo-builder --test captured_pg_cli -- --test-threads=4
cargo clippy --locked -p oo-builder --lib --bin oo-builder --test captured_pg_cli --test a160_oracle -- -D warnings
```

Install the 1.94.0 Clippy component in that container before Clippy. The green logs are `linux/smoke/`. The oracle summary must not be one large `serde_json::json!` value; that form failed to compile on attempt 1.

## Cleanup names

Only a160 objects are removed after the evidence is assembled. Nothing named a159 or a154 is included.

Containers: `oo-a160-builder`.

Volumes: `oo-a160-eef831aa-cargo`, `oo-a160-eef831aa-target`.

Network: `oo-a160-eef831aa-net`.

Temp: `/tmp/oo-a160-eef831aa`.

The result of that removal is `cleanup.txt`. No prune and no foreign container removal.
