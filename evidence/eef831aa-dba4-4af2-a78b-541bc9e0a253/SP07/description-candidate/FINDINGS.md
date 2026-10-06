# Findings

## What the compiler accepts

`prepare_description_request` reads `oo.builder.description-request-command/v1` with `admitted=true` and no `captured_response`. The artifact must be `oo.builder.captured-pg-draft-command/v1` and recompile to its sealed hashes. The selection names visible generated datasets with the sealed schema ident, name ident, dataset, schema hash, and evidence hash.

The request is canonical JSON. `boundary.instructions` is a fixed sentence and is not interpolated with catalog text. `boundary.data.role` is `data`. Table cards contain dataset identity, primary key, column types, comment hashes, and foreign keys whose other end is also selected. `request_sha256` is the canonical hash of the request with that field omitted, then inserted.

`complete_description_candidates` requires `captured_response`. Status `complete` parses one `oo.builder.description-response/v1` object. The candidate set must equal the selected datasets. Text bounds, controls, and confidence shape are checked against the caller budget. The outcome copies label, description, grain, caveats, open questions, and confidence, and adds `effect=review_only`.

## Refusals

Seal, admission, selection, binding, and command-shape failures return `Err` with a static message. The message does not include catalog names, owners, or response text.

After a valid seal, truncated, incomplete, provider, caller budget, response-byte budget, and format failures return an outcome document. `candidates` is empty. `structural_draft` keeps the sealed protocol and hashes. A body that would have parsed is not turned into candidates when the status is not `complete`, when the body exceeds `max_response_bytes`, or when the request itself already failed its table, column, or request-byte budget.

Duplicate JSON keys, unknown keys (`pii`, `publication`, and any other extra key), wrong JSON types, and echoed hash or card mismatches reject the whole response. A missing or extra dataset is `incomplete`, not a partial candidate list.

## Hidden table

A schema of 31 ASCII `A` characters has stable ident `q_` plus the hex of those bytes, length 64. `QualifiedTable` does not accept that ident, so the structure draft keeps the physical table and emits no generated dataset. The description compiler returns `selection.unknown` when the name is absent and `selection.hidden` when the physical table is present but not generated, including when the caller sets `visibility=hidden` on a generated table. Unknown is checked first.

The locked schema hash is the `pg-catalog-hash/v1` preimage with `primary_key_nulls_not_distinct` null. The value false does not load through `compile_captured_pg_catalog` for an empty primary key. The unit test and the integration compile both lock `49c560bd3d46e627813a8352a165fd042a1fbce1acbf7b51979ace0b97f330f2`.

## Compiler notes from the Linux proof

rustc 1.94.0 rejected `usize::from(u32)` with `E0277` in `linux/proof-2/test-lib.log`. The table-budget check uses `usize::try_from`.

Clippy `-D warnings` rejected `Stage::Ready(Ready)` as `large_enum_variant`. The ready variant is `Box<Ready>`.

`Cargo.lock` was not edited. a207 remains the connector manifest and root lock writer.

## Outside this unit

No sample channel, no comment body channel, no column-level candidates, no temperature, cache, retry, provider transport, residency approval, or token cost. Comments and labels are data. They do not change statements. Confidence does not set PII, curated, effective, grants, or publication.
