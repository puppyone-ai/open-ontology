# Handoff

Source commit `2283f904f71c9f04738a554f9c67439fa1db9bfc` on parent `476d21c7a031c53fb8b582fb49c6a5dafd2a0b4c`, tree `8e093dd327b6ba73035b1a2d0b2ea88c4b25ac42`. Fixed base `3353380b03a5585a349fddd0038d0a79877c51e1`, tree `5d6767362bedea26d673b43b932a7776f2ecfc25`.

Public command: `oo-builder review-pg-draft`. One JSON document on stdin. No arguments. Fields are `catalog`, optional `curated`, and optional `reviews`. The process calls `compile_captured_pg_catalog`. Stdout protocol `oo.builder.captured-pg-draft-command/v1`. Contract: `contracts/builder/captured-pg-draft-v1.md`.

`captured-pg-draft` still passes none for curated and reviews. `structure-draft` still writes the same draft bytes. The no-argument text names the three commands and exits 2.

The next review can treat this command as a caller of the approved API. `provenance.pg_catalog_integration` stays `not_accepted`. `wrapper.live_catalog_review` stays `pending`. `row_import` stays `not_proven`. `grants` stays `not_authorized`. A review decision binds one structural item, schema hash, and fact fingerprint. It does not approve semantics or publication. Owner authority, authentication, multitenant isolation, persistence, and an audited review workflow remain later SP07-W05 work. This command does not mark SP07 complete.

`tests/structure_draft_cli.rs` changed one no-argument assertion. The prompt's write list did not name that file. The reason is in `FINDINGS.md`.

## Reproduce

Archive `3353380b` with `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `.cargo`, `examples/ontology-draft`, and `contracts/builder`. Overlay the five committed paths. Copy the a157 catalogs to `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out/catalog-before.json` and `catalog-after.json` in that snapshot. Require SHA-256 `de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9` and `53e2fe7e68197f8698a2a9e2683dc2131426a0faea6bfb277594f9ba9ce52903`. `RUSTUP_TOOLCHAIN=1.94.0` on image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. Run:

```
cargo test -p oo-builder --test review_pg_cli -- --test-threads=4
cargo test -p oo-builder --test captured_pg_cli -- --test-threads=4
cargo test -p oo-builder --test structure_draft_cli -- --test-threads=4
cargo clippy -p oo-builder --bin oo-builder --test review_pg_cli --test captured_pg_cli -- -D warnings
```

Install the 1.94.0 Clippy component in that container before Clippy. The component lives in the container filesystem, not in the cargo volume.

## Cleanup names

These a161 objects were removed after the logs were copied out.

Containers: `oo-a161-builder-smoke`, `oo-a161-builder-focused`.

Volumes: `oo-a161-eef831aa-cargo`, `oo-a161-eef831aa-target`.

Network: `oo-a161-eef831aa-net`.

Temp: `/tmp/oo-a161-eef831aa`.

The result is `cleanup.txt`. No prune and no foreign container removal.
