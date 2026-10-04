# Findings

Verdict: **APPROVE LIMITED**.

## What this command proves

`oo-builder review-pg-draft` on commit `2283f904f71c9f04738a554f9c67439fa1db9bfc` reads one document and calls `compile_captured_pg_catalog` (`crates/oo-builder/src/bin/oo-builder.rs:123-138`). Stdout is protocol `oo.builder.captured-pg-draft-command/v1`. The objects equal the public API strings. The stdout has no newline byte.

On the a157 `ledger` catalog, catalog-only stdout is 23329 bytes, SHA-256 `45f35cf6208e59c70d4556051cf2ac1d2096c43eafd950396414180f4e0ceb08`. `captured-pg-draft` writes those same bytes. `draft_hash` is `d6980c138fa4939195bd673582938c94e9adca72a9acf710ddcebae407b138ef`. `wrapper_hash` is `e525520f010677050108f4aed9d899ecca28e2c56cbcb91e10d9485d20e3267e`. Both equal the a157 seals. An empty reviews array does not change them.

A fresh accept binds `column/ledger.party/email` and leaves PII and semantic type unknown and not publishable. A fresh accept of `invoice_party_fk` plus a fresh reject of `column/ledger.party/id` removes that foreign key from the effective layer and queues `relationship_endpoint_not_effective` with detail `to_column_not_effective`. The generated relationship stays. The curated echo keeps status `rejected` and label `Kept label`. A fresh override applies the structural label `Correspondence` and keeps curated PII `sensitive` in the echo. Effective PII is absent. `semantic_field_not_applied` records `pii`.

A decision whose schema hash is the pre-alter hash stays pending with `stale_schema_hash` on `catalog-after.json`. The draft hash is the a157 stale seal `8e00ba219f9e99044a739acb3f96eff04f7ce44784a42bec1609e32ee7ae688f`. The curated label `Correspondence` and PII `sensitive` stay in the echo. The effective label stays `email`. A decision whose fact fingerprint is 64 zero digits, and whose schema hash matches, stays pending with `stale_fact_fingerprint`. `wrapper.live_catalog_review` stays `pending`. `provenance.pg_catalog_integration` stays `not_accepted`. `grants` stays `not_authorized`. `publication` stays `unavailable`.

Stdin of 1048577 bytes, with the pipe left open, exits 1 in 21 ms on `json.budget.bytes`. A document of 1048576 spaces exits 1 on `json.syntax`. Duplicate keys, unknown fields, a non-object catalog, a null curated value, a non-array reviews value, a missing catalog, invalid UTF-8, malformed JSON, and trailing data exit 1 with empty stdout. The planted marker `a163-do-not-echo` is absent from those stderr texts. No arguments exits 2 in 22 ms while stdin is still open. The stderr is the eight usage lines and does not contain `process entry is not implemented`. An argument on `review-pg-draft` exits 2 and does not echo the argument.

`structure-draft` still writes the customer golden bytes, SHA-256 `486a22960ec2c759455bf26c1d970e44dc486a5e7225a38ec4d6c08b876cb106`, and exits 0. The production library and the lock are the a157 blobs.

The command does not open a connection, read a credential, call a model, import rows, grant, certify, or publish. It does not store an owner, an operator, a tenant, or an audit record. SP07-W05 remains open. This slice makes the existing review arguments reachable from the process.

## Diff outside the write card

The a161 write card named `crates/oo-builder/src/bin/oo-builder.rs`, `crates/oo-builder/tests/review_pg_cli.rs`, `crates/oo-builder/tests/captured_pg_cli.rs`, and `contracts/builder/captured-pg-draft-v1.md`. `crates/oo-builder/tests/structure_draft_cli.rs` is also in the commit.

`tests/structure_draft_cli.rs:21-23`:

```rust
assert!(stderr.contains("usage: oo-builder <command>"));
assert!(stderr.contains("structure-draft"));
assert!(!stderr.contains("process entry is not implemented"));
```

The parent line was `assert!(stderr.contains("process entry is not implemented (SP00-W01 skeleton)"));`. The test reads the shared no-argument stderr. The structure-draft command itself still writes the customer golden and still exits 2 when given an argument. The coordinator accepts this narrow obsolete-usage assertion for the fixed review. The diff is recorded in `source/structure-draft-cli.diff`. It is not a compiler change and not a second semantic command.

`tests/captured_pg_cli.rs` is inside the write card. Its change is the same no-argument expectation: it looks for `usage: oo-builder <command>`, names `review-pg-draft`, and stops requiring the skeleton sentence.

## What stays open

Row import is `not_proven`. Grants are `not_authorized`. Publication is `unavailable`. Certification is `none`. `live_catalog_review` stays `pending`. Historical `pg_catalog_integration` stays `not_accepted` at commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`. Owner authority, authentication, multitenant isolation, persistence, and an audited review workflow remain later SP07-W05 work. This review does not approve a semantic publication or mark SP07 complete.

The library diff does not edit `apply_reviews`, `relationship_gap`, or `nfc.rs`. This review did not rerun the closure matrix or the Unicode campaign. The 28 library tests are the existing crate smoke.

## Failures kept

The Linux builder had no failed attempt. `linux/smoke/` is the only run. Fetch, lib, the independent oracle, `structure_draft_cli`, `captured_pg_cli`, `review_pg_cli`, and scoped Clippy exited 0. The container exited 0. `OOMKilled=false`.

`launch.sh` wrote an empty `source/structure-draft-cli.diff` because `git diff -- <parent> <fixed>` treats those commits as pathspecs. The file now holds the diff from `git diff 476d21c7a031c53fb8b582fb49c6a5dafd2a0b4c 2283f904f71c9f04738a554f9c67439fa1db9bfc -- crates/oo-builder/tests/structure_draft_cli.rs`. The note is `source/structure-diff-note.txt`. Product sources were not edited.
