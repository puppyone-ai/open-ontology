# Freeze — combined project pack

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a362`. Amendment of the consumer at `a13b1cc80ed66c07199a41d47b31fd42530f2be9` and of description-pack at the same tree. Written before the product edit. This file does not accept E47, SP07, public authentication, semantic publication, or a release. It adds no countable unit.

a357 WITHHOLD stands. The measured gap is that each per-table pack reseals only that call's approvals, so `SHELF_ID_BODY_a357` and `BIN_ID_BODY_a357` never share one role file, and the sealed party sentence is absent from the account role file. This amendment does not concatenate those role files and does not edit the a357 oracle.

## Product

One combined project candidate is built from explicit per-table description-review commands that already share one sealed captured catalog.

Pins, all required and compared before any reseal:

| Pin | Rule |
| --- | --- |
| schema hash | artifact wrapper, every review `current.schema_hash`, and `expected_schema_hash` are the same |
| evidence hash | artifact wrapper, every review `current.evidence_hash`, and `expected_evidence_hash` are the same |
| curated hash | canonical JSON of every review command's `curated` object, and `expected_curated_sha256` |
| artifact | canonical JSON of every embedded captured-draft artifact, and `expected_artifact_sha256` |
| prior review CAS | canonical JSON of the artifact's compiler review list (`[]` when absent), and `expected_prior_review_sha256` |

Approved dataset and column overlays are the `applied` rows whose disposition is `filled` or `replaced`. They are merged in dataset order, then column ident order. Defer and reject add no text. The merged overlay is passed to `compile_captured_pg_catalog` through the existing description-pack `reseal` once. That call is the only returned seal. `compile_reviewed_effective` and `compile_role_pack` then run on that one seal. The command requires exactly two role snapshots.

The original per-table `description-pack apply` and `pg-builder pack` / `pack_pair` stay. They still reseal one generation. `description-pack apply-combined` and `pg-builder pack-combined` are the aggregate command and the library functions those binaries call. The aggregate document contains the combined product and one provenance row per table. A provenance row copies that table's review decisions, column records (confidence, caveats, open questions), rebind record, and the prior compiler review objects for that dataset. It is not a second role pack.

## Refusals before a seal

A refusal is an error. There is no combined document and no new seal. Inputs are not modified.

| Code | When |
| --- | --- |
| `review.stale` | The reviews agree with each other and disagree with a caller pin, or a review queue is stale |
| `review.double` | The same dataset is selected twice, or the same classification column is repeated with the same values |
| `review.conflict` | The same dataset field or column would receive two different strings, or the same column is classified twice with different `pii` or `semantic_type` |
| `review.mixed_scope` | Artifacts, schema, evidence, curated bytes, or prior review lists differ across the reviews; a decision names a dataset outside its selection; a role names a dataset outside the shared catalog |

`review.blocked` remains the refusal for a generation that is not `validated`. An unbound rebind does not call the pack compiler. Timeout and unavailable generations do not produce a document.

## What stays unforged

`review.state` is not written. `authenticated` stays false. `parent_auth` stays `required`. Grants, publication, and model publication stay unauthorized. `include_unreviewed` is the caller's settings value. When it lists `column_descriptions`, the role compiler's existing `UNREVIEWED:` prefix is the visible form of approved column prose. When it does not, that prose stays off the role file. Caveats and questions stay on the review record.

`national_id` has no classification and is not on either role allowlist, so the column and its prose stay out of both role files. `shop.entry` decisions stay `defer`, so entry prose is not applied. The restricted role's allowlist is the caller's snapshot: it omits `display_name` and `phone`.

Stored generation bytes, cache files, and the curated input are read and not rewritten. No new model call is part of this command. The stored request, cache, and cost fields stay on the generation documents.

## Out of scope

Full SP07 management RBAC, public authentication, semantic publication, release, parent acceptance, and a fresh 100k database load. The core compiler, root manifests, and pins are not edited.
