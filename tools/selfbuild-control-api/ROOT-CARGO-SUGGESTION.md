# Root Cargo suggestion for `derive_source_bound`

This file is a request to root. It does not change the root manifest or lock.

`oo_core::source_bound::derive_source_bound` at `8397fd5b984df364cc3638ffa1e69ea06e125d74` is:

```text
pub fn derive_source_bound(catalog_json: &str, bound_json: &str) -> Result<DerivedSourceBound, SourceBoundError>
```

in `crates/oo-core/src/source_bound.rs`. It accepts an oracle sealed-import catalog plus a dictionary observation whose protocol is `oo.source-semantics.source-bound/v1`. It emits caveats and `native_note`. It does not emit an OOS `data_type`.

This crate cannot path-depend on `crates/oo-core` at the dirty workspace HEAD. That package pulls `schemars`, `utoipa`, `jsonschema`, `serde-saphyr`, `chrono`, `chrono-tz`, and `thiserror`. `CONTRACT.md` forbids new crates, version bumps, and edits to the root `Cargo.toml` or `Cargo.lock`. A `serde_json` reimplementation would accept duplicate keys that the fixed parser rejects, so it is not the same function.

What this crate does instead: read the fixed profile `oo.builder.source-semantics-profile/v1` from evidence `1e7798c0-f799-4d4b-ab47-5e15599f60d9` byte for byte, record its sha256 and the semantics commit `8397fd5b984df364cc3638ffa1e69ea06e125d74`, and keep null `consistency_grade` and `delete_detection` as unknown. The OID map in `src/adapt.rs` cites `target_type` in `crates/oo-core/src/sealed_import.rs` at that same commit. OID 17 (`bytea`) is not mapped. This is not a claim that `derive_source_bound` ran inside this process.

The paragraph above is the historical request. It is not the current build rule.

The coordinator allowed this crate, and only this crate, to path-depend on existing `oo-core` at `983577c9b89453b08ab4e1a02cab88df78c3e005`. That commit contains the `8397fd5b` source-bound derivation. The path is `../../../crates/oo-core`. It is compiled from a read-only git archive of `983577`, with this crate overlaid at `stage/w1/selfbuild-control-api`, so the relative path lands on the archive's `crates/oo-core`. This crate's own `Cargo.toml` and `Cargo.lock` record that path and the locked dependencies it requires. The root `Cargo.toml` and root `Cargo.lock` are still not edited. There is no new public package and no crates.io publish.

`derive_source_bound` is called on the Oracle sealed catalog and dictionary in the fixed `batch-positive` request. PostgreSQL capture bytes are not passed to that function. PostgreSQL storage names follow the same OID table as private `target_type`. `bytea` stays outside the closed OOS set.
