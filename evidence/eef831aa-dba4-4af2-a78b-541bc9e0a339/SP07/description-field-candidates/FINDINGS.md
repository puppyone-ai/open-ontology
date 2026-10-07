# Findings

Verdict: **DELIVERED_FOR_A328**. Producer commit `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. Decisive container `linux/proof-7`, id `02949443c871a986285e9a6cb82836d4cc53a05a2d3c31aa7d431f263a02e744`, `docker-run.exit` 0.

The owned overlay that container compiled matches `source/blobs.txt` and `linux/proof-7/overlay.sha256`. Failed attempts `linux/smoke` through `linux/proof-6` are preserved.

## Smoke-3

Smoke-3 is a harness failure on the fixed archive, before product edits. `tests/provider.rs:804` panics when `OO_A263_DESCRIPTION_CANDIDATE` is unset (`cli path: NotPresent`). 22 provider tests passed. The candidate API (10), CLI (5), review (16), and pack (14) had already passed in that container. Smoke-4 exports the variable to `/target/smoke/debug/description-candidate` and the provider count is 23, including `valid_response_is_review_only_and_matches_the_cli`. `tests/provider.rs` was not edited. Smoke-3 is not the delivery.

## Proof-1 through proof-6

Proof-1 did not compile: the card digest was moved into `card_hashes` and then borrowed. The insert now stores `digest.clone()`, and the column index receives that clone.

Proof-2 compiled candidate, lib, and review. Pack tests passed an owned `String` to `serde_json::from_str`. The request text is bound, and the parser receives `&request_text`.

Proof-3 compiled pack and failed two assertions. A column-only approval reseals column label and description and leaves the effective dataset label at the generated draft value `orders`. The curated document still holds `Orders register`. Role packs emit a column description when `column.review.state` is approved or when `include_unreviewed` contains `column_descriptions`. The shared `settings()` list is `dataset_descriptions` only, so the vault body was absent until that one command opted in. The column review object was not written by this seam.

Proof-4 passed every focused test and failed oo-builder Clippy: `outcome_value` had eight arguments, and the `columns` insert was a nested `if`. Candidates and columns now travel in one `Listed` value. The insert is one let chain. v1 still omits the key because `failure_columns` is `None` for a v1 binding.

Proof-5 passed tests and oo-builder Clippy and failed description-review Clippy: `column_stale` had eight arguments. The function no longer takes a separate request string. It compares `decision.request_sha256` with `outcome.request_sha256`, using an empty string when the outcome has none. That matches the previous `unwrap_or` behavior.

Proof-6 passed tests, oo-builder Clippy, and description-review Clippy, and failed description-pack Clippy at `queue_projection`. The optional queue `column` key is now one let chain. Absent `column` still omits the key, so a v1 queue item stays three fields. Proof-7 ran that source. `clippy-pack.exit` is 0, and the pack tests are 21 passed.

## What proof-7 holds

Approve writes `label` and `description` on the matching `DatasetColumn` and round-trips `DatasetFile`. Confidence, caveats, and open questions remain on the column decision (`confidence_effect` `recorded_only`, `authenticated` false) and on queue items `pending/caveats` and `pending/open_questions`. They are not curated keys. Dataset `caveats` are not replaced by column caveats. Interpolation in approved column text is `core_schema` at review time and writes nothing.

Reject writes nothing. Defer queues `deferred` and writes nothing. A missing ident queues `column_missing` before the stale checks and does not invent a column. Stale anchors refuse the decision. A v2 failure outcome has empty `candidates` and empty `columns`, and pack does not reseal it.

An occupied column review stays in place unless `replace_existing_column_review` is true and `expected_existing_column_review_sha256` is the canonical hash of that four-field compiler review. Prior review objects stay on the resealed artifact. A dataset-only approval does not require column decisions. A column-only approval still reseals. Reject and defer with no filled field leave the artifact unchanged.

Unknown PII on an allowlisted column fails `classification.unknown_pii` with no pack document. The pack does not invent `pii` or `semantic_type`, and it does not copy confidence into those decisions. Hidden schema hash `49c560bd3d46e627813a8352a165fd042a1fbce1acbf7b51979ace0b97f330f2` passed again.

## Outside this verdict

This file does not close E47 or SP07. It does not add a unit. It does not accept publication, a live grant, or an authenticated principal. No model was called from this actor. The root lock was not edited. `tools/pg-builder` was not edited.
