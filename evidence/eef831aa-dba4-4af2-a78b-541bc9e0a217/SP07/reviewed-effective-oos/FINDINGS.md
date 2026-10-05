# Findings

Author check of commit `9daeaa05e34c154391a757fc78fb47f09c954c6a` against `contracts/builder/reviewed-effective-oos-v1.md` blob `0a219892be7bb9643a29bd2c8022aa99071e33b5`. This is the same session that wrote the code. It is not an independent review. M1-D18A stays unaccepted. No ledger line was edited.

## Missing bridge

a203 and a211 left no compiler from a sealed `oo.builder.captured-pg-draft-command/v1` artifact to `oo.builder.effective-oos/v1`. A structural accept does not set `pii` or `semantic_type`. Wrapping review stdout as `effective_oos` is `schema.rejected`. `compile_role_pack` returns `classification.unknown_pii` when an allowlisted column has null `pii`.

`compile_reviewed_effective` recompiles the evidence envelope through `compile_structure_draft` and checks the wrapper, draft, and manifest hashes. A mismatch is `seal.rejected`. Generated and effective columns that already carry `pii`, `semantic_type`, `classification`, or `review` are `source.classified`. Curated may echo those fields. They are hashed as `curated_sha256` and are not copied. The green test `curated_pii_is_not_copied_and_money_needs_an_existing_unit` passed.

## Decisions

A column is classified only by an explicit decision with a lowercase schema hash, the structural-column fingerprint, a `PiiLevel`, a `SemanticType`, and reviewer `by` and `at`. `ontology.classification.auto_accept` is stored because `OntologyFile` requires it. The library test keeps `id` unclassified while `auto_accept` is `rule_high_confidence`.

Duplicate dataset and column is `decision.duplicate`. A name that is not on the effective draft, including a rename, is `decision.unknown`. A bad PII token is `decision.invalid`. A hash that does not match is `decision.stale_schema` or `decision.stale_fact`, and the column stays null. Schema is checked before fact. The hidden list is sorted by dataset then column. Unclassified columns that remain on the dataset stay there so role-pack closure still sees the name.

## Rejected columns

`oo_core::oos::merge` removes an item whose `status` is `rejected` from the effective list. `review-pg-draft` action `reject` on `column/public.customer/email` therefore drops `email`. Attempt 2 expected the column to remain and failed. The contract now says a decision for that removed name is `decision.unknown`, and a role pack that names it is `visibility.unknown_column`. The compiler does not put the column back.

If an effective column is still present with `status` `rejected`, the decision is not applied and `hidden` uses `column.rejected`. Stale schema and stale fact are recorded first. The library test `a_rejected_column_that_is_still_effective_is_not_classified` covers that shape. The current structure-draft merge does not emit it.

## Role pack

The effective document is `validate_dataset` and `validate_relationships`, then scanned as `oo.builder.effective-oos/v1`. Money without an existing unit is `oos.invalid`. The compiler does not invent a unit. A structural override that already set unit `CNY` is kept, and the decision's `pii` replaces nothing that was copied from curated.

`oo_r_clerk` includes `purchase_customer_fk` with the text `declared_fk (source-declared, not a grant)`. `oo_r_peek` does not. `oo_r_ledger` allowlists both tables and omits `customer_id`, so the relationship is absent. Pack text for those three roles does not contain `email`. Authentication, authorization, grants, publication, and semantic publication stay `not_claimed`.

## Bounds

- `crates/oo-builder/Cargo.toml` still says the crate does not classify. That file is not owned. It was not edited. `Cargo.lock` was not edited. a207 remains the only connector-manifest and root-lock writer. This batch added no dependency.
- `lib.rs` versus the fixed commit is the crate-doc paragraph, `mod reviewed_effective`, and the `pub use`. Blob `c3a09422c79727ac3413ab8082c0c050f673eea4`. No other `oo-builder` library module was edited.
- The fixed commit's `rust-toolchain.toml` names channel 1.98.1. The extracted file is moved aside and `RUSTUP_TOOLCHAIN=1.94.0` is set. The git object is unchanged.
- Owner string `owner_role_do_not_leak` and comment body `a156-comment-body-do-not-carry` are in the catalog passed to `review-pg-draft`. They are absent from `reviewed-effective-oos` stdout and from the role-pack stdout in the green tests.
- Column ACL for a second table, `reltuples` / `ANALYZE`, authentication, row security, and semantic publication stay outside this slice. M1-D18 publication stays open.
- No PostgreSQL container was started. The catalog document is the captured `oo.source.pgsql/v1` fixture.
