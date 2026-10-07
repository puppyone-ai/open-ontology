# Safe Rust

The reviewed crate and this review's oracle are Safe Rust. No Python file decides a gate, a hash, or a decimal.

- Frozen `tools/metric-reconcile` at `15558917` forbids `unsafe_code` on the crate and in `src/lib.rs`. Clippy on `linux/review-3` passed `-D warnings` (exit 0) after `cargo test` (21 passed).
- Review oracle `oracle/src/main.rs` is `#![forbid(unsafe_code)]`. It compiled inside the same 1.94.0 builder (`oracle.exit` 0) and produced `linux/review-3/live/checks.txt`.
- Decimal checks in the oracle are digit vectors (`plain_diff`). Receipt checks use `serde_json` plus `sha2` on `MetricReport` with `receipt_sha256` cleared. PostgreSQL values arrive as text from a `SELECT ::text` or from the component JSON.
- `tools/sha256.rs` and `tools/select-debs.rs` are Rust copies of the a316 helpers. They hashed the lock and selected libclang debs. Shell scripts start containers and copy exits. They do not compute the metric difference.
- The author's `src/live.rs` stayed `not_invoked`.

A host byte search compared the evidence tree with the fixture password files before those files were deleted. It found no copy. That search is not a product test. Config JSON stores password file paths only.
