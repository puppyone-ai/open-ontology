# Handoff

Verdict: **APPROVE LIMITED**.

Reviewed commit `2283f904f71c9f04738a554f9c67439fa1db9bfc`, tree `8e093dd327b6ba73035b1a2d0b2ea88c4b25ac42`, parent `476d21c7a031c53fb8b582fb49c6a5dafd2a0b4c`. Author evidence `aa9a083d9630bddf51d5f5fb2004b3be64a6cf6a`, manifest 93 OK. Fixed base `3353380b03a5585a349fddd0038d0a79877c51e1`. Approved compiler `63466471a6d403e0a2a29c8d1ae064179da65bd8` (a157). Approved captured command `3353380b` (a160).

Public command: `oo-builder review-pg-draft`. One JSON document on stdin. No arguments. Fields are `catalog`, optional `curated`, and optional `reviews`. The process calls `compile_captured_pg_catalog`. Stdout protocol `oo.builder.captured-pg-draft-command/v1`. Contract: `contracts/builder/captured-pg-draft-v1.md`.

`captured-pg-draft` still passes none for curated and reviews. On `catalog-before.json` its stdout matches `review-pg-draft` catalog-only, SHA-256 `45f35cf6208e59c70d4556051cf2ac1d2096c43eafd950396414180f4e0ceb08`. `structure-draft` still writes the customer golden, SHA-256 `486a22960ec2c759455bf26c1d970e44dc486a5e7225a38ec4d6c08b876cb106`.

`provenance.pg_catalog_integration` stays `not_accepted`. `wrapper.live_catalog_review` stays `pending`. `row_import` stays `not_proven`. `grants` stays `not_authorized`. A review decision binds one structural item, schema hash, and fact fingerprint. It does not approve semantics or publication. Owner authority, authentication, multitenant isolation, persistence, and an audited review workflow remain later SP07-W05 work. This command does not mark SP07 complete.

`tests/structure_draft_cli.rs:21-23` changes the no-argument assertion. The a161 write card did not name that file. The coordinator accepts the narrow obsolete-usage assertion. The diff is `source/structure-draft-cli.diff`.

## Reproduce

Archive `2283f904` with `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `.cargo`, `examples/ontology-draft`, and `contracts/builder`. Overlay `oracle/a163_oracle.rs` as `crates/oo-builder/tests/a163_oracle.rs`. Copy the a157 catalogs from that same commit to `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out/catalog-before.json` and `catalog-after.json`. Require SHA-256 `de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9` and `53e2fe7e68197f8698a2a9e2683dc2131426a0faea6bfb277594f9ba9ce52903`. `RUSTUP_TOOLCHAIN=1.94.0` on image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. Set `A163_ORACLE_DIR` to a writable directory. Run:

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --lib -- --test-threads=8
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test a163_oracle -- --test-threads=1
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test structure_draft_cli -- --test-threads=4
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test captured_pg_cli -- --test-threads=4
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test review_pg_cli -- --test-threads=4
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --lib --bin oo-builder --test a163_oracle --test review_pg_cli --test captured_pg_cli --test structure_draft_cli -- -D warnings
```

Install the 1.94.0 Clippy component in that container before Clippy. The component lives in the container filesystem, not in the cargo volume.

## Cleanup names

These a163 objects were removed after the logs were copied out. The logs were already on the bind mount.

Containers: `oo-a163-builder`.

Volumes: `oo-a163-eef831aa-cargo`, `oo-a163-eef831aa-target`.

Network: `oo-a163-eef831aa-net`.

Temp: `/tmp/oo-a163-eef831aa`.

The result is `cleanup.txt`. No prune and no foreign container removal.
