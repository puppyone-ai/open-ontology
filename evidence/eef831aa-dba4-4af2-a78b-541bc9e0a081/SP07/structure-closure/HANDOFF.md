# Handoff — a081 structure closure

Stop here for an independent review of the delivery commit. Do not treat this slice as a finished SP07 or a finished vertical loop.

## What to review

The delivery commit adds only:

- `crates/oo-builder/src/draft.rs`
- `crates/oo-builder/src/pgsql.rs`
- `crates/oo-builder/src/draft_tests.rs`
- `crates/oo-builder/tests/structure_draft_cli.rs`
- `contracts/builder/structure-draft/README.md`
- `contracts/builder/structure-draft/fixtures/`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a081/SP07/structure-closure/`

No `nfc.rs`, `nfc_tables.rs`, `oo-core`, manifest, lock, or older evidence.

Linux tested archive tree `93c6cc57f1ad3dc6e7dcc67ba8761316405ced91`. Base commit at archive time `911f317cde6d87ec49c247afc35681c7c64d5c91`. The committed source blobs for the five owned text files match that tree. Fixtures in the tree are the eight JSON files under `contracts/builder/structure-draft/fixtures/`. `duplicate-pk-wrong-schema-hash.json` is evidence-only.

Reviewed compiler input remains `4c989bce58407cf19d3d93dc600162d7322d92f8`.

## Commands and exits

Baseline run `3bf0c1b7-a66f-46ec-ac34-526cf13231f0`, archive of `a45c13c0e231dcbf6c7b97050b9bfbe8a11bb571`.

Fixed run `daa20988-8e6b-47b0-858d-0e29a7f7b6cc`, archive tree above.

Image both runs: `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

`rustc 1.94.0 (4a4ef493e 2026-03-02)`, `host: aarch64-unknown-linux-gnu`.

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings
```

Baseline exits: fetch 0, test 0, clippy 0, clippy component 0, docker 0.
Fixed exits: fetch 0, test 0, clippy 0, clippy component 0, docker 0.

Fixed test counts: 27 lib passed, 4 CLI passed. Raw logs are `baseline/console.log` and `fixed/console.log`. Phase integers are the `*.exit` files. They were not rewritten.

## Fixture conclusions

See `REPORT.md` and `counterexamples/conclusions.json`.

Finding 3: fresh endpoint rejection drops the effective relationship and explains it with `relationship_endpoint_not_effective`. Stale rejection does not. Unrelated relationships stay.

Finding 4: exact repeated primary-key members that the old compiler accepted now exit 1 with an empty stdout and `duplicate.key_member`. Alias repeats that the old compiler reported as `key.unknown_column` now use the same static error. A zeroed catalog hash still fails `hash.schema` before that error.

## Left open

Findings 1, 2, and 5 (`nfc-composition-equal-class`, `unicode-data-notice-missing`, `nfc-unicode-17-combining-classes`) stay open for a separately serialized dependency correction after SDK handoff.

Also open: unsigned numeric scale, physical ident length versus PostgreSQL 63 bytes, unverified foreign-key emission, in-tree UCD generator, unused `oo-jobs`, real PostgreSQL integration, RBAC, PII, LLM, publishing, and query.

No push, pull request, release, or customer access was made.
