# Safe Rust inventory

Product and tests are Safe Rust.

- `tools/metric-reconcile/src/lib.rs` and `src/main.rs` both `#![forbid(unsafe_code)]`.
- `Cargo.toml` sets `unsafe_code = "forbid"` and denies `unused_must_use`. Clippy denies `dbg_macro`, `todo`, and `unimplemented`. The overlay-4 Clippy run also passed `-D warnings`.
- Tests are `#[cfg(test)]` modules in the same crate: `behavior.rs`, `live.rs`, `lock_subset.rs`, plus unit tests in `numeric.rs`, `decimal_cmp.rs`, `template.rs`, `report.rs`, and `lib.rs`. There is no `tests/` directory and no dev-dependency.
- The live fixture, the numeric decoder, the decimal comparison, and the receipt hash are Rust. No Python oracle decides a gate.
- Evidence helpers `tools/sha256.rs` and `tools/select-debs.rs` are Rust. `sha256.rs` hashed the fixed `Cargo.lock` inside the builder. `select-debs.rs` is the preoverlay libclang helper and was not required again on overlay-4 because the cargo volume already held libclang.
- Shell scripts start containers and record exits. They do not compute metric values, tolerances, or receipts.
- A host scan compared evidence bytes with the fixture password files and found no copy. That scan is not a product test.

No `unsafe` token appears in the crate except the forbid attributes. Dependencies are the pinned root-lock crates listed in `CONTRACT.md`. `bytes` and `rust_decimal` are not direct dependencies. Numeric OID 1700 is decoded in `src/numeric.rs`.
