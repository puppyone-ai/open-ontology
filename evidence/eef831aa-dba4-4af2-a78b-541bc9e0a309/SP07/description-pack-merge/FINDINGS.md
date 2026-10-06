# description-pack occupied-review findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a309`. Implementation `ceb4be5ebe12cf581f511cc8a676050d02465c55`. Contract `c548c8b18c4de5d75e6f8527c71569a2c311eb11`. Fixed archive `cab5e8e219d1924070df7336f5ab5d4ce7cf40dc`. Proof `linux/proof-1` exit 0.

## Compiler review object

`ReviewIn` in `crates/oo-builder/src/draft.rs` accepts `item_id`, `action`, `schema_hash`, and `fact_fingerprint`. Unknown fields fail parse. Dataset fact fingerprint is the canonical hash of kind, table name, and schema. It does not include label, description, or grain. `accept` on a dataset item does not copy those fields. `override` on a dataset item copies label, description, grain, and source name from the curated overlay. It does not copy entity, PII, or classification. Column override copies label, description, unit, and status. Primary-key override copies the key list. A stale schema hash or fact fingerprint becomes queue kind `stale_decision` and does not apply. Dataset `reject` is `review.not_in_scope` and cannot sit in a compiled seal. `defer` is not a compiler action.

The evidence envelope stores the reviews value that was passed in, then canonicalizes the envelope. The draft re-emits the same four fields. This crate does not insert `reviewer_label` or `reviewer_at` into that object.

## Replacement rule

`replace_existing_dataset_review` defaults false. `expected_existing_review_sha256` defaults absent. A hash map without the flag is `schema.invalid` with message `a replacement hash requires authorization`, checked before `apply_description_review`.

When an approval's dataset item is already in the sealed reviews and the flag is false, the outcome is `unsupported` / `review.dataset_decision_occupied` before any reseal compile. The previous probe-before-occupied path is gone, so a fresh existing override is not overwritten by a probe that already applied the new text.

When the flag is true, the map's keys must be exactly the occupied dataset item ids under approval. A missing, extra, or unequal hash is refusal `review.occupied_cas`. The compared hash is SHA-256 of the canonical JSON of that one stored review object. An action other than `override` is `unsupported` / `review.dataset_action_unsupported`. The crate does not rewrite `accept` to `override`. An `override` whose compile shows `stale_decision` for that item is refusal `review.occupied_stale`, and that compile is discarded.

Reject and defer decisions do not produce a filled or replaced approval, so they do not replace a review. The outcome is `reviewed_unchanged` and the packs come from the original seal. A non-empty hash map with no occupied approval is `review.occupied_cas`.

## What the reseal writes

`write_approvals` inserts only label, description, and grain onto the existing curated dataset entry. Source name, columns, primary key, entity, relationships, status, and PII stay on that entry. The original review array is passed back to `compile_captured_pg_catalog` when every approval is occupied. The new seal is that compiler result. If the envelope reviews are not canonically identical to the supplied reviews, the seam is `review.entry_not_preserved` and no seal is returned.

A dataset with no existing review still appends one `override` whose fingerprint comes from the compiler queue detail `curated_not_applied`. That is the path exercised by the original eight tests. A command that mixes an occupied approval with a fresh approval is implemented: the probe must not be stale, then only the fresh items are appended and a second compile must keep every original review object. The handproof does not include that mixed command.

## Attribution

The compiler object cannot carry a second reviewer. On a successful replacement the pack document adds `occupied_replacements`. Each row has `attribution` `bridge_provenance`, the exact `prior_review`, `prior_review_sha256`, and `compiler_review_sha256`. Those two hashes are equal because the compiler review bytes did not change. `descriptive_provenance` copies `authenticated`, `reviewer_at`, and `reviewer_label` from the description decision, plus the filled or replaced field names. `authenticated` is copied. This crate does not set it true. The array is omitted when empty, so the eight original documents keep their previous shape. Statement keys are unchanged.

The structural reviewer is not recorded on the compiler object, and this crate does not invent one. The description decision's reviewer is only in `descriptive_provenance`.

## Proof bounds

The author's eight tests passed on the archived tree and again after the overlay. The six extension tests passed in the same post-overlay run. The a302 seven-test file passed from a read-only mount. It is not vendored into `tools/description-pack`. Commands in that file do not opt in, and its occupied case stays the old seam.

`description-review` and `oo-builder` bytes were unchanged inside the container. The root lock and the child lock match the fixed archive. `cargo update` was not run. Python was present in the image and was not called. This task started no PostgreSQL. Host `cargo`, `rustc`, `rustdoc`, and `rustup` process count at launch was 0. This task did not invoke the host toolchain.

Clippy before the overlay was not a separate phase. The pre-overlay phase is the eight archived tests on Rust 1.94.0. Clippy `-D warnings` ran after the overlay and exited 0.

## Acceptance

This extension is the same S5-D05 unit. It does not add a denominator child. Parent acceptance is false. An amended review of this unit can still be required after delivery. SP07 and SP08 stay open.
