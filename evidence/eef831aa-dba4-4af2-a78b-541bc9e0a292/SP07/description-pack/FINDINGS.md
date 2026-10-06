# description-pack findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a292`. Implementation `5ce8b1c4186eb80f9b876a306ae94309f10933ae`. These findings are from the Linux 1.94 runs in `linux/proof-1`, `linux/proof-2`, and `linux/proof-3`. They do not accept the parent unit.

## The captured compiler can reseal approved text

`compile_captured_pg_catalog` does not accept a `DatasetFile` as curated. Approved `label`, `description`, and `grain` enter the effective layer only when an override review exists for `dataset/{qualified}`. The dataset fact fingerprint is not on the public queue for an emitted table, and `canonical_sha256` is not exported. `apply_description_pack` probe-compiles the overlay. The compiler publishes `fact_fingerprint` on the `curated_not_applied` queue item. A second compile adds only that override, using the compiler's fingerprint, schema hash, and the compiler's own wrapper, draft, and envelope strings. `linux/proof-3` `approved_text_is_resealed_and_packed_per_role` passed on that path. The original artifact's wrapper hash stayed the same. The new artifact has its own wrapper, draft, and evidence-envelope hashes.

## Occupied dataset reviews are unsupported

When `dataset/public.orders` is already in the sealed reviews, the second compile is not attempted. The outcome is `unsupported` with seam `review.dataset_decision_occupied`. There is no resealed artifact and no pack. `occupied_dataset_review_is_unsupported` passed. The planted review's wrapper hash was unchanged.

## What stays outside the new seal

`curated_documents` is the description-review `datasets` array. The aside dataset, ledger document, orders entity, row count, tier, caveat, column pii `personal`, status `deprecated`, and classification `confirmed` stayed there. The captured overlay column stayed pii `sensitive`, semantic type `free_text`, status `draft`. Draft effective pii stayed null. Reviewed-effective pii `restricted` came from the submitted classification decision. Three different pii values were visible on the three layers.

## Role-pack policy is unchanged

Settings include `dataset_descriptions`, so the pay pack rendered the description inside the existing `UNREVIEWED` fence. The desk role does not allowlist `vault_token`. The description that names `vault_token` was omitted for that role, and the label and grain remained. `secret_note` stayed out of both packs. Allowlisting `secret_note` returned `oo.builder.role-pack/v1/classification.unknown_pii` and no outcome document.

## Refusals

A stale schema queue item returns `review.schema`. A label CAS mismatch returns `review.cas`. A wrong review hash returns `review.hash`. An artifact that is not the object inside the review command returns `seal.artifact`. A provider-failure body returns `not_candidates` and does not copy that body. Reject and defer leave `reviewed_unchanged` and compile packs from the revalidated original artifact.

## Contract order

`CONTRACT.md` was written after the first source files existed and before a successful compile. The assignment asked for the contract before implementation. That order was not kept. The contract now names the unsupported seams and the four outcomes that `linux/proof-3` exercised.

## Build notes

`linux/proof-1` failed to compile the test canonical writer because `format!("\\u{ch:04x}")` requires `LowerHex` for `char`. The writer now uses `format!("\\u{:04x}", u32::from(ch))`, the same form as `src/canonical.rs`.

`linux/proof-2` compiled and Clippy was clean. Four tests failed because `desk.user` and `pay.user` contain `.`. `check_slug` in `role_pack.rs` accepts lowercase letters, digits, `_`, and `-`. The principals in the passing run are `desk_user` and `pay_user`.

The image has Python (`python-present.flag` 1). The launch and inside scripts do not call it. Hash checks in the container use `sha256sum` and the Rust `lock_subset` tool. The inventory uses the a199 `sha256_manifest.rs`.

The child lock adds no registry package. `a207` remains the only writer of the root lock and connector manifests. This unit did not edit `Cargo.toml`, `Cargo.lock`, `crates/`, or `tools/description-review`.

No database grant, auth store, model call, or semantic publication was performed.
