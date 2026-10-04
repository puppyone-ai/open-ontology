# Findings

`oo-builder review-pg-draft` on commit `2283f904f71c9f04738a554f9c67439fa1db9bfc` reads one document and calls `compile_captured_pg_catalog` (`crates/oo-builder/src/bin/oo-builder.rs`). Stdout is protocol `oo.builder.captured-pg-draft-command/v1`. The objects equal the public API strings. The a157 bare catalog seals and the a157 stale draft hash are unchanged.

A fresh accept binds `column/ledger.party/email` and leaves PII and semantic type unknown and not publishable. A fresh accept of `invoice_party_fk` plus a fresh reject of `column/ledger.party/id` closes that foreign key in the effective layer with detail `to_column_not_effective`. The generated relationship stays. A fresh override applies the structural label and keeps curated PII in the echo. `semantic_field_not_applied` records `pii`.

A decision whose schema hash is the pre-alter hash stays pending with `stale_schema_hash` on `catalog-after.json`. The curated label `Correspondence` and PII `sensitive` stay in the echo. The effective label stays `email` and effective PII stays null. A decision whose fact fingerprint is 64 zero digits, and whose schema hash matches, stays pending with `stale_fact_fingerprint`. The compiler requires that fingerprint to be 64 lowercase hex digits before it can be classified stale. `wrapper.live_catalog_review` stays `pending`. `provenance.pg_catalog_integration` stays `not_accepted`. `grants` stays `not_authorized`. `publication` stays `unavailable`.

The command does not open a connection, read a credential, call a model, import rows, grant, certify, or publish. It does not store an owner, an operator, a tenant, or an audit record. SP07-W05 remains open. This slice makes the existing review arguments reachable from the process.

## Usage text

The no-argument path used to print `process entry is not implemented (SP00-W01 skeleton)` and then name two commands. That sentence was false once the commands existed. Exit 2 is unchanged: a missing command is not a draft. The new text names `structure-draft`, `captured-pg-draft`, and `review-pg-draft` and states the limits of a review decision. The contract quotes it.

`tests/structure_draft_cli.rs` asserted the old skeleton sentence. The prompt named `captured_pg_cli.rs` as the expectation update and did not name this file. The structure-draft command bytes and exits are unchanged, and the shared no-argument text is what that test reads. The assertion now expects `usage: oo-builder <command>` and rejects the skeleton sentence. Without that one-line update, the required structure regression is red.

## Failures kept

`linux/focused/attempt-1`: `review_pg_cli` did not compile because a raw string was written `br"{}"#`. `captured_pg_cli` failed one assertion that still looked for `does not connect` while the usage text says `do not connect`. `structure_draft_cli` passed, 4 tests. Container exit 1.

`linux/focused/attempt-2`: closure and the stale-schema case reached later assertions. The compiler sorts review decisions by `item_id`, so index 0 is the column reject. A fact fingerprint that is not 64 lowercase hex digits is `field.invalid` before staleness is considered. `captured_pg_cli` passed, 3 tests. `structure_draft_cli` passed, 4 tests. Clippy passed. Container exit 1.

The green run is `linux/focused/`. `review_pg_cli` 4 passed, `captured_pg_cli` 3 passed, `structure_draft_cli` 4 passed, scoped Clippy exit 0.
