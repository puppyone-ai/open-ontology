# Handoff

Source commit `63466471a6d403e0a2a29c8d1ae064179da65bd8` on parent `377707e88b13b2b77baa6f619c2c1b364f0dab28`. Fixed base `e6bd4c7aa8c3a43cc0516a21e325765c5eecb2f6`.

Public entry: `oo_builder::compile_captured_pg_catalog(catalog_json, curated_json, reviews_json) -> Result<CapturedPgDraft, DraftError>`. Input is the serde JSON of the current `SourceCatalog`. Contract: `contracts/builder/captured-pg-draft-v1.md`.

The next review is independent. It should treat `provenance.pg_catalog_integration = not_accepted` as the historical compiler contract and `wrapper.live_catalog_review = pending` as the state of this capture. It should not mark SP07 complete from this work.

## Reproduce the focused check

Archive `e6bd4c7a` with `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `.cargo`, `examples/ontology-draft`, and `contracts/builder/structure-draft`. Overlay the six committed paths. `RUSTUP_TOOLCHAIN=1.94.0`. Run library tests, `captured_pg`, `structure_draft_cli`, and clippy `-p oo-builder --lib --test captured_pg -- -D warnings`. Do not rustfmt all of `draft.rs`.

## Reproduce the live probe

`probe/launch.sh` builds a snapshot the same way, adds `probe/` only in that snapshot's workspace, and starts one private Postgres 17.10. The probe binary calls `PgConnector::new`, `connect`, and `PgSession::catalog`. Passwords stay in `/tmp` files. `pg/audit.py` classifies `statement:` and `execute name:` lines. `START` is a transaction open, not a write.

## Cleanup names

Only these a156 objects are removed after this evidence is assembled. Nothing named a154 is included.

Containers: `oo-a156-builder-smoke`, `oo-a156-builder-focused`, `oo-a156-probe`, `oo-a156-pg`.

Volumes: `oo-a156-eef831aa-cargo`, `oo-a156-eef831aa-target`, `oo-a156-eef831aa-pglog`, `oo-a156-eef831aa-pgdata`.

Network: `oo-a156-eef831aa-net`.

Temp: `/tmp/oo-a156-eef831aa` and `/tmp/oo-a156-eef831aa-dba4-4af2-a78b-541bc9e0a156`.

The result of that removal is `cleanup.txt`. No prune and no foreign container removal.
