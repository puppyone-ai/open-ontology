# Handoff

## This unit

Fixed archive `0bcd494457091d4b55c82fc6b9922b5bc5dc44c4`, tree `fafa03de58bee2f37df65f9d3691d1aa99290fc5`.

Contract: `tools/business-calendar/CONTRACT.md`.

Workspace: `tools/business-calendar`. Binary: `business-calendar`.
Entry: one `oo.business-calendar/v1` JSON document on stdin. `--help` prints help and exits 0.

The library calls `parse_calendar_yaml`, derives the configured anchors from the explicit `as_of` civil date in `business_tz`, and evaluates phrase endpoints with the anchor grammar. Quarter boundaries are `business_fiscal_quarter`.

Passing proof: `linux/impl-4`. Fresh-target review: `linux/review-1`. Pre-edit smoke: `linux/precode`. All three are Linux arm64 Rust 1.94, 6 GiB, 1 CPU, no PostgreSQL. Root lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Child lock `7a9470817fc50ce83b44faacd4cd6330b3ee6d76d22a0e0bf9cbb75532f83cf5`.

Inventory: `SHA256SUMS`, written by `tools/sha256_manifest.rs` after this handoff and the cleanup record. That tool matches the a199 blob `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`.

## Next child

A later child owns database materialization, a restricted-owner calendar view, and any authenticated finance close. This helper keeps `finance_approval` and `authenticated` false and stores the caller's declared closure hash.

Do not treat `week_start` as a week anchor. Do not replace a missing anchor with a natural month. Do not shift a gap or an overlap onto a nearby midnight. Phrase order stays the `BTreeMap` order of labels. `inclusive_end_civil` stays the civil day before the exclusive end.

## Do not widen this unit

PostgreSQL, auth, a live close, root lock edits, and new dependencies are outside this child. A new expansion needs its own frozen contract before code.

This child does not claim parent acceptance, SEM-D01 acceptance, or a release.
