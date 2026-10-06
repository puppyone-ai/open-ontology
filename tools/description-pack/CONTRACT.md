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

`settings` and every role snapshot must already satisfy the upstream compilers. This bridge does not invent classification, PII, or visibility.

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
| `review.dataset_decision_occupied` | `dataset/{qualified}` is already present in the sealed reviews |
| `review.dataset_fingerprint_absent` | The compiler's unbound queue did not publish one dataset fact fingerprint |
| `effective.field_not_applied` | A second compile did not place the approved text on the effective dataset |
| `curated.document_absent` | An applied decision has no reviewed dataset document |
| `curated.field_absent` | A filled or replaced field is not a string on that document |

## What is copied

Only a decision with `applied: true` and disposition `filled` or `replaced` contributes text. The text is the reviewed dataset document's `label`, `description`, or `grain`. The bridge writes those fields onto the captured curated overlay and asks `compile_captured_pg_catalog` for a dataset override. The override fingerprint and schema hash come from that compiler. This crate does not compute a dataset fingerprint and does not rewrite an existing sealed draft.

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
| `seal.artifact` | `artifact` is not the object inside the review command |
| `seal.rejected` | Recompiling the envelope catalog, curated overlay, and reviews does not reproduce `artifact` |
| upstream | `apply_description_review`, `compile_reviewed_effective`, or `compile_role_pack` returned an error |

Messages are static. They do not echo names, hashes, candidate prose, or sealed bodies.

## Statements

`authentication` and `authorization` are `not_claimed`. `database` is `not_used`. `grants` is `not_authorized`. `llm` is `not_used`. `publication` is `unavailable`. `semantic_publication` is `not_claimed`. `source_artifact` is `not_mutated`.

## CLI

`description-pack apply` reads one command from stdin and writes the outcome to stdout with no added newline. Exit 0 is a document, including `unsupported` and `not_candidates`. Exit 1 is a refusal. Exit 2 is usage. Stderr is `description-pack: <code> <message>`.
