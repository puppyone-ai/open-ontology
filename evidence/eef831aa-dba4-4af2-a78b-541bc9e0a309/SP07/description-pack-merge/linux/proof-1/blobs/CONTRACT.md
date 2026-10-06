# description-pack contract

`oo.builder.description-pack-command/v1` binds one already produced description review to a new sealed captured draft and to role packs. The bridge calls `apply_description_review`, `compile_captured_pg_catalog`, `compile_reviewed_effective`, and `compile_role_pack`. It does not open a database, grant, authenticate, call a model, or publish.

Compiler id: `oo-builder/description-pack/v1`.

Outcome protocol: `oo.builder.description-pack/v1`.

Stdout is canonical JSON with no added newline. `pack_sha256` is SHA-256 of that document with `pack_sha256` omitted.

## Command

One object. Unknown fields are rejected.

| Field | Rule |
| --- | --- |
| `protocol` | `oo.builder.description-pack-command/v1` |
| `review` | The exact original `oo.builder.description-review-command/v1` text |
| `expected_review_sha256` | SHA-256 of the bytes `apply_description_review` returns for `review` |
| `artifact` | The sealed `oo.builder.captured-pg-draft-command/v1` object embedded in that review |
| `settings` | One `oo.builder.reviewed-effective-oos-command/v1` settings object |
| `decisions` | The classification decision array for that reviewed-effective command |
| `roles` | One to eight `oo.builder.role-visibility/v1` objects, each with a distinct `role` |
| `replace_existing_dataset_review` | Optional boolean. Absent or false keeps the historical occupied-review refusal |
| `expected_existing_review_sha256` | Optional object. Present only when replacement is authorized. Keys are `dataset/{qualified}`. Each value is the SHA-256 of the canonical JSON of that one existing review object |

`settings` and every role snapshot must already satisfy the upstream compilers. This bridge does not invent classification, PII, or visibility. A replacement hash without `replace_existing_dataset_review: true` is `schema.invalid`. The boolean alone does not select a review.

## Outcomes

`packed` means at least one approved `label`, `description`, or `grain` was placed by a new captured-draft compile. `resealed_artifact` is that compiler's command object. `resealed_draft_hash`, `resealed_wrapper_hash`, and `resealed_evidence_envelope_hash` are the compiler's own hashes. `original_draft_hash` and `original_wrapper_hash` are the recompile of the supplied artifact. The two wrapper hashes differ when a new seal was required.

`reviewed_unchanged` means the review re-executed and no filled or replaced field remains. Reject, defer, kept, and not-selected decisions do not create a seal. Packs are compiled from the revalidated original artifact. Reseal fields are null. `statements.reseal` is `not_required`.

`not_candidates` means the description compiler did not return candidates. The document carries `compiler_outcome`, `compiler_refusal`, `compiler_budget_reason`, `review_sha256`, and a queue of `dataset`, `kind`, and `reason` only. It has no datasets, decisions, artifact, effective document, or packs.

`unsupported` means the captured-draft compiler cannot represent the approved overlay without a source edit. The document carries `seam` and `review_sha256`. It has no resealed artifact, no pack bytes, and no curated documents. `statements.reseal` is `unsupported`. A seam is one of:

| Seam | When |
| --- | --- |
| `curated.unknown_item` | The compiler rejected a curated dataset or column that is not in the catalog |
| `curated.overlay_rejected` | The compiler rejected the overlay for another static reason |
| `schema.invalid` | The compiler rejected the overlay shape |
| `review.dataset_decision_occupied` | `dataset/{qualified}` is already present and the caller did not authorize replacement. This was the only occupied-review outcome through the a302 review, and it remains the default |
| `review.dataset_action_unsupported` | Replacement was authorized and the CAS matched, but the existing action is not `override`. The bridge does not rewrite it |
| `review.entry_not_preserved` | A compiler result dropped or rewrote a review object the bridge did not intend to add |
| `review.dataset_fingerprint_absent` | The compiler's unbound queue did not publish one dataset fact fingerprint |
| `effective.field_not_applied` | A second compile did not place the approved text on the effective dataset |
| `curated.document_absent` | An applied decision has no reviewed dataset document |
| `curated.field_absent` | A filled or replaced field is not a string on that document |

## What is copied

Only a decision with `applied: true` and disposition `filled` or `replaced` contributes text. The text is the reviewed dataset document's `label`, `description`, or `grain`. The bridge writes those fields onto the captured curated overlay and asks `compile_captured_pg_catalog` for a dataset override. The override fingerprint and schema hash come from that compiler. This crate does not compute a dataset fingerprint. It does not mutate the supplied artifact. A new seal exists only as the bytes of a later `compile_captured_pg_catalog` call.

## Occupied dataset review

The captured compiler's review object is exactly `item_id`, `action`, `schema_hash`, and `fact_fingerprint`. `action` is `accept`, `reject`, or `override`. Unknown fields are rejected. The object has no reviewer, date, or provenance field. A dataset `reject` is `review.not_in_scope` and cannot occur in a seal this compiler emits. `defer` is not a compiler action. `accept` on a dataset item does not copy `label`, `description`, or `grain`. Only `override` does, and it also copies `source_name` when that curated field is present. Column override copies that column's `label`, `description`, `unit`, and `status`. Primary-key override copies `primary_key`. Relationship override copies `status`. The dataset fact fingerprint is the compiler's structural fingerprint. It does not include label, description, or grain text.

Through the a302 review, any existing `dataset/{qualified}` review was `unsupported` with seam `review.dataset_decision_occupied`. That default remains when `replace_existing_dataset_review` is absent or false. The bridge does not return a probe compile that already applied new text over an occupied review.

Replacement requires both `replace_existing_dataset_review: true` and `expected_existing_review_sha256`. The map's keys must be exactly the occupied dataset items whose approvals are `filled` or `replaced`. A missing key, an extra key, or a different hash is refusal `review.occupied_cas`. There is no outcome document and no new seal. The hash is SHA-256 of the canonical JSON of that review object, with object keys sorted.

When the hash matches and `action` is not `override`, the outcome is `unsupported` with seam `review.dataset_action_unsupported`. The bridge does not change `accept` or any other action into `override`.

When the hash matches and `action` is `override`, the bridge compiles the updated overlay with the original review list. If that compile's queue has `stale_decision` for the item, the binding's schema hash or fact fingerprint does not match the compiler. The refusal is `review.occupied_stale`. The compile is discarded. Reject and defer decisions do not create an approval, so they do not replace a review. A non-empty replacement map with no occupied approval is `review.occupied_cas`. An empty or absent map on a reject or defer leaves the historical `reviewed_unchanged` path.

A successful replacement writes only the approved `label`, `description`, and `grain` onto that curated dataset entry. `source_name`, columns, primary key, entity, status, relationships, and every other review object stay. The dataset review's four compiler fields stay. The bridge does not insert the description reviewer into the review object and does not invent an approved author for the structural review. The new seal is only `compile_captured_pg_catalog` of that overlay and that review list. Other review entries are preserved. If the compiler result does not keep them, the seam is `review.entry_not_preserved` and no seal is returned.

The packed document then carries `occupied_replacements`, sorted by `item_id`. Each row has `attribution` `bridge_provenance`, `item_id`, `prior_review` (the four-field object), `prior_review_sha256`, `compiler_review_sha256` of that same object in the new seal, and `descriptive_provenance`. The descriptive provenance copies `reviewer_label`, `reviewer_at`, and `authenticated` from the description decision, plus the sorted names of the filled or replaced fields. `compiler_review_sha256` equals `prior_review_sha256` because the compiler cannot store a second reviewer. That equality is the field separation this compiler allows. The description reviewer is not a structural author. `authenticated` remains the decision's own value. This bridge does not set it to true. Documents that did not replace an occupied review omit `occupied_replacements`. Statement keys are unchanged.

Classification, status, PII, entity, semantic type, column rows, and extra curated dataset files stay on `curated_documents`, which is the description-review `datasets` array. Those DatasetFile-only fields are not inserted into the captured overlay. A curated dataset that is not a catalog table stays in `curated_documents` and is not forced into the seal.

Reviewed-effective classification comes only from `decisions`. Curated PII and semantic fields are not copied into the effective layer. Role packs are the bytes `compile_role_pack` returns for each visibility snapshot. Dataset descriptions render only when settings include `dataset_descriptions`, and the role-pack compiler still prefixes its UNREVIEWED fence. A description that names a hidden column is omitted for that role by the existing role-pack filter. An allowlisted column with unknown PII fails the role-pack compiler; this bridge returns that error and no pack document.

## Refusals

A refusal is an error. There is no outcome document.

| Code suffix | When |
| --- | --- |
| `review.hash` | `expected_review_sha256` is not the re-executed review |
| `review.schema` | A review queue item is `stale` with reason `schema` |
| `review.evidence` | Stale reason `evidence` |
| `review.cas` | Stale reason `cas` |
| `review.anchor` | Any other stale reason |
| `review.occupied_cas` | Replacement was requested and the existing-review hash, key set, or unused map does not match |
| `review.occupied_stale` | An authorized override compiled to `stale_decision` for that dataset item |
| `seal.artifact` | `artifact` is not the object inside the review command |
| `seal.rejected` | Recompiling the envelope catalog, curated overlay, and reviews does not reproduce `artifact` |
| upstream | `apply_description_review`, `compile_reviewed_effective`, or `compile_role_pack` returned an error |

Messages are static. They do not echo names, hashes, candidate prose, or sealed bodies.

## Statements

`authentication` and `authorization` are `not_claimed`. `database` is `not_used`. `grants` is `not_authorized`. `llm` is `not_used`. `publication` is `unavailable`. `semantic_publication` is `not_claimed`. `source_artifact` is `not_mutated`.

## CLI

`description-pack apply` reads one command from stdin and writes the outcome to stdout with no added newline. Exit 0 is a document, including `unsupported` and `not_candidates`. Exit 1 is a refusal. Exit 2 is usage. Stderr is `description-pack: <code> <message>`.
