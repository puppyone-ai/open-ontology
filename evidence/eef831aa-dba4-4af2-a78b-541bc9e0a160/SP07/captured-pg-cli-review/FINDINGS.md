# Findings

Verdict: **APPROVE LIMITED**.

## What this command proves

`oo-builder captured-pg-draft` on commit `3353380b03a5585a349fddd0038d0a79877c51e1` reads the a157 `ledger` catalog and calls `compile_captured_pg_catalog(input, None, None)` (`crates/oo-builder/src/bin/oo-builder.rs:86`). Stdout is protocol `oo.builder.captured-pg-draft-command/v1` with `wrapper`, `evidence_envelope`, and `draft` as JSON objects. Those objects equal the public API strings. The stdout has no newline byte.

Recomputed seals match the wrapper and the draft. `draft_hash` is `d6980c138fa4939195bd673582938c94e9adca72a9acf710ddcebae407b138ef`. `wrapper_hash` is `e525520f010677050108f4aed9d899ecca28e2c56cbcb91e10d9485d20e3267e`. Both equal the a157 seals for this fixture. `evidence_envelope_hash` is `415da260e8e229f9b0a9b07e03a6a106d2cda4cf8ebb4b38c6444b627fea8eae`. `captured_catalog_hash` is `5c17683d74bcc7916954dfb23d22004e5ccbd4058f54bb1d3699ca679a8b90b0`. `manifest_hash` is `25b3533d37badc024ea46e393bfc33cc6711616590b4f0888c1202197eb04f03`.

The review queue is on the draft. `pii/ledger.party/email` / `pii_unknown` and `semantic/ledger.party/email` / `semantic_unknown` are pending and not publishable. Email PII and semantic type are null. `invoice_party_fk` is `many_to_one`, provenance `declared_fk`, status `draft`, confidence null, join hint `left`. `wrapper.row_import` is `not_proven`. `wrapper.grants` is `not_authorized`. `live_catalog_review` stays `pending`. Historical `pg_catalog_integration` stays `not_accepted`.

The fixture comment has no body. The sentence `a157-review-comment-body-do-not-carry` is absent from the fixture and from stdout. Adding a `body` field makes the command exit 1 with `field.unknown`, empty stdout, and a stderr that does not contain the planted sentence.

Both tables in the evidence envelope keep owner `a157_owner` and nine privilege records. The string `a157_owner` occurs 18 times in the command stdout because the envelope is one of the three objects. It does not occur in the draft object or the wrapper object. `grantable` does not occur in those two objects. The command result has no top-level `owner` field.

Stdin of 1048577 bytes, with the pipe left open, exits 1 in 22 ms on `json.budget.bytes`. A document of 1048576 spaces exits 1 on `json.syntax`. Malformed JSON, invalid UTF-8, and the planted body exit 1 with empty stdout and a static stderr. No arguments, an unknown command, and an extra argument exit 2. No-arguments exits in 20 ms while stdin is still open, and the stderr does not contain the marker written to that pipe.

`structure-draft` still writes the customer golden bytes and exits 0. A duplicate key exits 1. The production library and the lock are the a157 blobs.

## Documentation residual

`crates/oo-builder/src/bin/oo-builder.rs:13` is:

```rust
eprintln!("oo-builder: process entry is not implemented (SP00-W01 skeleton)");
```

The no-argument process prints `oo-builder: process entry is not implemented (SP00-W01 skeleton)` and then names `structure-draft` and `captured-pg-draft`. Exit is 2. The command contract records this sentence as the current no-argument text. The named `captured-pg-draft` command is the behavior approved above.

## What stays open

Row import is `not_proven`. Grants are `not_authorized`. Publication is `unavailable`. Certification is `none`. `live_catalog_review` stays `pending`. Historical `pg_catalog_integration` stays `not_accepted` at commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`. Curated overlays and review decisions are not arguments of this command. SP07 stays open. This review does not approve an agent dialog, a gateway, or a row import.

The library diff does not edit `apply_reviews`, `relationship_gap`, or `nfc.rs`. This review did not rerun the closure matrix or the Unicode campaign. The 28 library tests are the existing crate smoke.

## Failures kept

`linux/smoke/attempt-1/oracle.exit` and `linux/smoke/attempt-1/clippy.exit` are 101. The independent oracle used `serde_json::json!` for its summary and hit the compiler recursion limit. `cargo test -p oo-builder --lib` was already 0 (28 passed). `structure_draft_cli` was already 0 (4 passed). `captured_pg_cli` was already 0 (3 passed). The container exited 1. The rerun compiled the oracle without that macro and exited 0. Product sources were not changed on either attempt.
