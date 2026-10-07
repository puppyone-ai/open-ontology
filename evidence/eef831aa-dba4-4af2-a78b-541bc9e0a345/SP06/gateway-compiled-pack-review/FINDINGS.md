# Findings

## F1. Maintained live entrypoint fails closed on the new helper

`crates/oo-gateway/tests/http_gateway_live.rs` is still the `OO_A326_LIVE` test. `fixture_artifact` at `pack_catalog.rs` lines 149–161 ignores every argument and returns `Err(PackError { reason: "legacy_unsupported" })`.

`write_config` still does this:

```rust
let picker = fixture_artifact("depot", "oo_r_picker", "pv1", "kv1", "orders", "picker_note").expect("picker artifact");
let auditor = fixture_artifact("depot", "oo_r_auditor", "pv1", "kv1", "orders", "auditor_phone").expect("auditor artifact");
```

Smoke-3 ran `cargo test --manifest-path /target/work/Cargo.toml --locked --offline --test http_gateway_live -- --ignored` with `OO_A326_LIVE=1` against private database `oo_a345`. Exit 101. The test panicked at `http_gateway_live.rs:235:98` with `picker artifact: PackError { reason: "legacy_unsupported" }` and finished in 0.19s. `old-live-marker.txt` contains `ran=1` and does not contain `completed=1`. The same panic happened in `linux/smoke` and `linux/smoke-2`.

The same binary, without `--ignored`, reports `0 passed; 0 failed; 1 ignored` and exits 0 even when `OO_A326_LIVE=1`. That is the command a normal `cargo test` runs. It hides F1.

This is a test-only break. The loader proof is `http_gateway_pg_live`, which passed. Do not change `fixture_artifact` so the old helper starts emitting packs. The repair is in HANDOFF.md. This review did not edit the product.

## F2. Reviewer oracle failures are not product failures

`linux/smoke` exit 101 stopped in `review_pack` after the product live test had passed. The first rebind left the manifest file list on the old README SHA-256, so the loader returned `source_digest`. That classification is what the contract requires. The oracle was fixed before smoke-2. See `linux/smoke/RETRY.txt`.

`linux/smoke-2` exit 101 stopped in Clippy on the review test's useless `format!`. The archive Clippy run was exit 0. Smoke-3 Clippy on the work tree, including the corrected review test, was exit 0.

## F3. One foreign builder was visible at smoke-3 start

`containers-before.txt` lists `02949443c871 oo-a339-proof7` with owner `a339` and kind `rust-builder`, plus this review's Postgres. Own builders were 0, global builders were 1, host `cargo`/`rustc` was 0, own Postgres was 1. The global cap is 10. The foreign container was left running.

## F4. No AGENTS.md

The repository has no `AGENTS.md`. The review used `docs/07-gateway.md`, the E45 plan line, the a326/a337/a341 packets, and `testing/real-db/gateway-compiled-pack/CONTRACT.md` at `6001e18e`.
