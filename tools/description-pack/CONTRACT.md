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
| `replace_existing_column_review` | Optional boolean. Absent or false keeps an occupied column review refused |
| `expected_existing_column_review_sha256` | Optional object. Present only when column replacement is authorized. Keys are `column/{qualified}/{ident}`. Each value is the SHA-256 of that four-field compiler review object |

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
| `effective.field_not_applied` | A second compile did not place the approved dataset or column text |
| `review.column_decision_occupied` | `column/{qualified}/{ident}` is already present and the caller did not authorize replacement |
| `review.column_action_unsupported` | Column replacement was authorized and the CAS matched, but the existing action is not `override` |
| `review.column_fingerprint_absent` | The compiler's unbound queue did not publish one column fact fingerprint |
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

## Column text

A v2 description review can carry `column_decisions`. The bridge reads `applied: true` rows whose `label_disposition` or `description_disposition` is `filled` or `replaced`. It copies that column's `label` and `description` from the reviewed `DatasetFile` onto the captured curated overlay, using the compiler `ident` as the column `name`. It does not copy confidence, caveats, open questions, PII, semantic type, classification, status, or unit from the decision. Caveats stay on the review record. An empty column-approval list leaves the dataset reseal path unchanged.

Occupied column replacement uses `replace_existing_column_review` and `expected_existing_column_review_sha256`. The hash is the canonical four-field compiler review object. A hash without the flag is `schema.invalid`. The wrong set or hash is `review.occupied_cas`. A missing flag on an occupied column is `unsupported` seam `review.column_decision_occupied`. An action other than `override` is `review.column_action_unsupported`. A stale override is `review.occupied_stale`. Existing review objects stay. Dataset occupied CAS is unchanged and does not use column keys.

`compile_reviewed_effective` still receives only the caller `decisions` array. This bridge does not invent a PII level or a semantic type and does not copy confidence into those decisions. `compile_role_pack` hides columns that are absent from that role's allowlist, including their labels and descriptions. Unknown PII on an allowlisted column fails closed with no pack document.

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

## Combined command

`oo.builder.description-pack-combined-command/v1` reads one to sixteen per-table `oo.builder.description-review-command/v1` texts that already share one sealed captured catalog. Unknown fields are rejected. `description-pack apply` is unchanged.

Pins, all required: `expected_schema_hash`, `expected_evidence_hash`, `expected_curated_sha256`, `expected_artifact_sha256`, and `expected_prior_review_sha256`. The schema and evidence pins are the artifact wrapper and every review `current` object. The curated pin is the canonical JSON of each review command's `curated` object. The artifact pin is the canonical JSON of each embedded captured-draft artifact. The prior-review pin is the canonical JSON of that artifact's compiler review list, or `[]` when the list is absent. `settings` is the caller reviewed-effective settings object. `decisions` is the caller classification array. `roles` is exactly two distinct `oo.builder.role-visibility/v1` objects.

Each review is re-executed. `expected_review_sha256` must match. The outcome must be `reviewed`. Approved dataset and column text is the `applied` rows whose disposition is `filled` or `replaced`. Defer and reject add no text. The rows are merged in dataset order, then column ident order, and passed to the existing `reseal` once. `compile_reviewed_effective` and `compile_role_pack` then run on that one seal. `reseal_count` is 1. Zero approvals skip `reseal`, set `reseal_count` to 0, and compile packs from the original artifact. This command does not set `replace_existing_dataset_review` or `replace_existing_column_review`. An occupied compiler review stays unsealed.

The outcome protocol is `oo.builder.description-pack-combined/v1`. Compiler id stays `oo-builder/description-pack/v1`. The document carries the pins, `reseal_count`, `product`, and `tables`. `product` is one pack: packs, effective document, reseal hashes, statements, curated documents, and `occupied_replacements` when a prior compiler review was kept. `tables` has one provenance row per selected dataset: `review_sha256`, `authenticated` false, that table's decisions and column records, the prior compiler review objects for that dataset, and the queue. `curated_documents` starts from the first dataset's review map and overlays later `changed: true` rows. `pack_sha256` is SHA-256 of the document with that field omitted. There is no trailing newline.

A refusal is an error. There is no document and no new seal. `review.hash` means a re-executed review hash does not match. `review.stale` means the reviews agree and a pin does not, or a review queue is stale. `review.double` means the same dataset is selected twice, or the same classification column is repeated with the same values. `review.conflict` means the same field would receive two strings, or the same column is classified twice with different values. `review.mixed_scope` means schema, evidence, curated bytes, artifacts, or prior review lists differ, a decision names a dataset outside its selection, or a decision or role names a dataset outside the shared catalog. `review.blocked` means a member outcome is not `reviewed`. `review.state` is not written. `include_unreviewed` is the caller's settings value.

`description-pack apply-combined` reads that command from stdin. Exit codes match `apply`.
