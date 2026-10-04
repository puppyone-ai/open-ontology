# Findings

`oo-builder captured-pg-draft` compiles the a157 `ledger` catalog through `compile_captured_pg_catalog(input, None, None)`. Stdout is protocol `oo.builder.captured-pg-draft-command/v1` with `wrapper`, `evidence_envelope`, and `draft` as JSON objects. Those objects equal the public API strings. Recomputed hashes match `draft_hash`, `evidence_envelope_hash`, `captured_catalog_hash`, `wrapper_hash`, and `manifest_hash`.

The review queue is machine-readable on that catalog. `pii/ledger.party/email` / `pii_unknown` and `semantic/ledger.party/email` / `semantic_unknown` are pending. Email PII and semantic type are null. `invoice_party_fk` is a tentative `many_to_one` relationship, provenance `declared_fk`, status `draft`, confidence null, join hint `left`. `wrapper.row_import` is `not_proven`. `wrapper.grants` is `not_authorized`. `live_catalog_review` stays `pending`. Historical `pg_catalog_integration` stays `not_accepted`.

The comment sentence `a157-review-comment-body-do-not-carry` is absent from the fixture and from stdout. `a157_owner` stays on the captured catalog inside the evidence envelope. It is absent from the draft and the wrapper. A bad document, invalid UTF-8, and an oversize stdin exit 1 with empty stdout. The diagnostic does not contain the planted catalog token or the oversize body. No arguments, an unknown command, and an extra argument exit 2. The no-argument text still contains the SP00-W01 skeleton line and names both draft commands.

`structure-draft` goldens and the two closure fixtures still pass. Curated overlays and review files are not command flags. This command does not finish SP07, import rows, grant, certify, or publish.

Attempt 1 installed no Clippy component, so `cargo clippy` exited 1 after the tests had passed. The focused rerun installed Clippy for 1.94.0 and exited 0 with no warning diagnostic. The pre-edit library smoke on the unmodified `63466471` archive was 28 passed and Clippy clean.
