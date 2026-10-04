# Handoff

Source commit `3353380b03a5585a349fddd0038d0a79877c51e1` on parent `8adb9c3680b0816195fead6a698bc6a704ac751b`, tree `5d6767362bedea26d673b43b932a7776f2ecfc25`. Fixed base `63466471a6d403e0a2a29c8d1ae064179da65bd8`, tree `1a6c65addb3897fe93159560b8d295949eb2542c`.

Public command: `oo-builder captured-pg-draft`. One `oo.source.pgsql/v1` catalog on stdin. No arguments. Stdout protocol `oo.builder.captured-pg-draft-command/v1`. Contract section: `contracts/builder/captured-pg-draft-v1.md`.

The next review can treat the command as a caller of the approved `compile_captured_pg_catalog`. `provenance.pg_catalog_integration` stays `not_accepted`. `wrapper.live_catalog_review` stays `pending`. `row_import` stays `not_proven`. `grants` stays `not_authorized`. This command does not mark SP07 complete.

## Reproduce

Archive `63466471` with `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `.cargo`, `examples/ontology-draft`, and `contracts/builder`. Overlay the three committed paths. Copy the a157 catalog fixture to `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out/catalog-before.json` inside that snapshot. `RUSTUP_TOOLCHAIN=1.94.0`. Run:

```
cargo test -p oo-builder --test captured_pg_cli -- --test-threads=4
cargo test -p oo-builder --test structure_draft_cli -- --test-threads=4
cargo clippy -p oo-builder --bin oo-builder --test captured_pg_cli -- -D warnings
```

Install the 1.94.0 Clippy component in that container before Clippy. The component lives in the container filesystem, not in the cargo volume.

## Cleanup names

These a158 objects were removed after the logs were copied out. Nothing named a154 was included.

Containers: `oo-a158-builder-smoke`, `oo-a158-builder-focused`.

Volumes: `oo-a158-eef831aa-cargo`, `oo-a158-eef831aa-target`.

Network: `oo-a158-eef831aa-net`.

Temp: `/tmp/oo-a158-eef831aa`.

The result is `cleanup.txt`. No prune and no foreign container removal.
