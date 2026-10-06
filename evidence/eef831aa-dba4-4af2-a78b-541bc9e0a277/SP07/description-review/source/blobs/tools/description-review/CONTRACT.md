# Description review

Contract `oo.builder.description-review-command/v1`. Outcome `oo.builder.description-review/v1`. Compiler id `oo-builder/description-review/v1`.

This unit reads one explicit human review and applies it to curated dataset descriptions in memory. It re-executes `oo_builder::complete_description_candidates` on the original command string. It does not trust a candidate object supplied beside that command. An approval writes only `label`, `description`, and `grain` onto an existing core `DatasetFile`. Caveats and open questions stay on the review record. This unit does not write a file, open a database, call a model, grant, certify, authenticate, or publish. A declared reviewer label is not an authenticated owner. This is not full S5.

`docs/06` keeps generated text and curated text apart. Curated wins a field only after a person accepts it. When the upstream schema or evidence hash moves, a curated value is not silently rewritten and is not silently kept as if it still matched. This unit records that case on `queue` and leaves the dataset bytes unchanged. It does not call `merge_effective` and it does not emit effective OOS.

## Command

`apply_description_review` reads one JSON object. The scanner rejects a document above 1048576 bytes, a duplicate key, a depth or node overrun, and trailing data. `serde` `deny_unknown_fields` rejects an unknown key. Messages are static and do not echo names, hashes, catalog text, or response text.

| Field | Rule |
|---|---|
| `protocol` | `oo.builder.description-review-command/v1` |
| `command` | string. The exact `complete_description_candidates` document, including `captured_response` |
| `expected_outcome_sha256` | 64 lowercase hex characters |
| `current` | object. `schema_hash` and `evidence_hash`, each 64 lowercase hex characters |
| `curated` | object. Keys are dataset ids. Values are core `DatasetFile` objects |
| `decisions` | array. One object per dataset the reviewer decided |

`command` is passed to `complete_description_candidates` unchanged. The outcome hash is SHA-256 of those exact UTF-8 result bytes. A mismatch is `outcome.hash`. No dataset is returned. A compiler `Err` is returned as that error. Its message is already static.

A key the schema does not name, including `candidates`, `classification`, `pii`, `semantic_type`, `status`, `entity`, `metric`, or `confidence`, is `schema.invalid`. Confidence on a generated candidate is never an approval.

### Decision

| Field | Rule |
|---|---|
| `dataset` | core qualified table `{schema}.{table}` |
| `decision` | `approve`, `reject`, or `defer` |
| `reviewer_label` | core `Label`: 1 to 256 Unicode scalars, no forbidden controls |
| `reviewer_at` | core civil date `YYYY-MM-DD` |
| `request_sha256`, `card_sha256`, `schema_hash`, `evidence_hash`, `candidate_sha256`, `curated_sha256` | 64 lowercase hex characters |
| `fields` | required on `approve`. Forbidden on `reject` and `defer` |

`reviewer_label` outside 1..=256 scalars is `reviewer.length`. A label inside that count that still fails the core control rule is `reviewer.controls`. Neither case truncates the label. `reviewer_at` that is not a civil date is `reviewer.date`. The label and date are recorded as declared. `authenticated` on every decision is false.

A repeated decision dataset is `decision.duplicate`. A curated key that is not a qualified table, or that differs from that value's `dataset`, is `curated.identity`.

### Approval fields

`fields` may contain only `label`, `description`, and `grain`. Each present value is `{ "overwrite": false }` or `{ "overwrite": true, "current_sha256": "<64 lowercase hex>" }`. `overwrite: true` without `current_sha256`, or `overwrite: false` with `current_sha256`, is `field.cas`. An omitted field is not selected.

`current_sha256` is the SHA-256 of the canonical JSON literal of the current curated value: the four bytes `null` when the field is absent, otherwise the canonical JSON string of the text. `curated_sha256` is the SHA-256 of the canonical JSON of `serde_json::to_value` of the core `DatasetFile` before any write. Canonical JSON sorts object keys, keeps array order, and escapes controls the way `oo-builder` canonical JSON does.

## Re-execution

The compiler outcome must be `oo.builder.description-candidate/v1` from `oo-builder/description-candidate/v1`. Unknown outcome keys, unknown candidate keys, or a non-empty `candidates` array when `outcome` is not `candidates` is `compiler.schema`.

When `outcome` is `candidates`, every candidate has `effect` `review_only`. Any other effect is `candidate.effect`. The candidate set must equal `selected_datasets`. A decision dataset outside that set is `decision.unknown`.

`candidate_sha256` is the SHA-256 of the canonical JSON of that candidate object as the compiler returned it, including `effect`.

When `outcome` is `budget_failure`, `truncated`, `incomplete`, `provider_failure`, or `format_rejected`, the review outcome is `not_candidates`. Curated documents are unchanged. Candidate text inside `captured_response` is not copied into the review document. `queue` has one item, `kind` `compiler`, `reason` the compiler outcome.

## Stale and unchanged

A stale dataset is not modified. The first matching reason is the queue reason.

| Order | Reason | Condition |
|---|---|---|
| 1 | `schema` | `current.schema_hash` differs from the outcome draft `schema_hash` |
| 2 | `evidence` | `current.evidence_hash` differs from the outcome draft `evidence_hash` |
| 3 | `schema` | the decision `schema_hash` differs from `current.schema_hash` |
| 4 | `evidence` | the decision `evidence_hash` differs from `current.evidence_hash` |
| 5 | `request` | the decision `request_sha256` differs from the outcome `request_sha256` |
| 6 | `card` | the decision `card_sha256` differs from the candidate `card_sha256` |
| 7 | `candidate` | the decision `candidate_sha256` differs from the recomputed candidate hash |
| 8 | `curated` | the decision `curated_sha256` differs from the curated document hash |
| 9 | `cas` | an overwrite hash differs from the current field literal |

`reject`, `defer`, and a missing decision leave the dataset unchanged. `reject` records the decision and does not write. `defer` does the same and adds queue reason `deferred`. A candidate with no decision adds queue reason `decision_missing`. A candidate with no curated `DatasetFile` adds `curated_missing` and does not invent a dataset. Columns, classification, PII, semantic type, status, entity, metric, tier, keys, and every other curated field stay as parsed.

## Core gate

A selected write is checked before it is stored. The check uses the core limits, which are tighter than an unconstrained string. Nothing is truncated.

| Field | Core rule |
|---|---|
| `label`, `grain` | `Label`: 1 to 256 Unicode scalars. Controls in `U+0000`–`U+0008`, `U+000B`, `U+000C`, `U+000E`–`U+001F`, and `U+007F` are rejected |
| `description` | `BoundedText<4000>`: 1 to 4000 Unicode scalars and no NUL |

The description-candidate compiler ceilings are the same 256 / 4000 / 256 counts, so a `candidates` outcome cannot by itself exceed those counts. The review still deserializes the merged `DatasetFile`. A failure refuses that dataset.

Text that contains `` ` ``, `{{`, `}}`, `${`, `$(`, or `%{` is `core_schema`. The dataset is not modified and the candidate text is kept whole on the decision record. A length failure is `core_length`. A control or NUL failure is `core_type`. Queue `kind` is `core_rejected`.

An absent field with `overwrite: false` is filled from the candidate. A present field with `overwrite: false` is kept. A present or absent field with `overwrite: true` and a matching literal hash is replaced. Dispositions are `filled`, `kept`, `replaced`, and `not_selected`. A stale, rejected, deferred, or core-rejected dataset uses `unchanged`.

`caveats` and `open_questions` are not written onto the dataset. `DatasetFile.caveats` already exists and is left as curated. `open_questions` is not a dataset field and is not added. Non-empty candidate arrays add queue reasons `caveats` and `open_questions`. The decision record keeps the arrays.

## Outcome

Success returns one canonical JSON object, sorted keys, no trailing newline.

| Field | Value |
|---|---|
| `protocol` | `oo.builder.description-review/v1` |
| `compiler` | `oo-builder/description-review/v1` |
| `outcome` | `reviewed` or `not_candidates` |
| `compiler_outcome` | the compiler `outcome` |
| `compiler_refusal` | the compiler `refusal` |
| `compiler_budget_reason` | the compiler `budget_reason` |
| `compiler_statements` | the compiler `statements` object, copied |
| `outcome_sha256` | SHA-256 of the compiler result bytes |
| `request_sha256` | the compiler request hash, or null |
| `datasets` | curated documents after the review, sorted by `dataset` |
| `decisions` | one record per input decision, sorted by `dataset` |
| `queue` | stale, pending, core, and compiler items |
| `statements` | the fixed object below |
| `review_sha256` | SHA-256 of this document with `review_sha256` removed |

Each dataset item is `dataset`, `changed`, `document`, and `document_sha256`. `document` is the core `DatasetFile` JSON. `changed` is true only when `document_sha256` differs from the preimage hash.

Each decision record contains the anchored hashes, the declared reviewer, `authenticated: false`, `confidence_effect: recorded_only`, the candidate `confidence` when a candidate exists, the candidate label, description, grain, caveats, and open questions when a candidate exists, `effect` `review_only` or null, `applied`, and the three dispositions.

Each queue item is `dataset`, `kind`, and `reason`. Compiler-wide items use an empty `dataset`. Items are sorted by those three strings.

`statements` is fixed:

| Field | Value |
|---|---|
| `authentication` | `not_claimed` |
| `auto_accept` | `not_authorized` |
| `column_acl` | `not_changed` |
| `confidence_effect` | `recorded_only` |
| `database` | `not_used` |
| `effective` | `not_written` |
| `entity` | `not_changed` |
| `files` | `not_written` |
| `grants` | `not_authorized` |
| `metric` | `not_changed` |
| `owner_ui` | `not_claimed` |
| `pii_classification` | `not_performed` |
| `publication` | `unavailable` |
| `reviewer` | `declared_only` |
| `semantic_classification` | `not_performed` |
| `source_artifact` | `not_mutated` |
| `status` | `not_changed` |

The captured draft inside `command` is not rewritten. Curated JSON in the input is not a file write.

## CLI

Binary `description-review`. The only phase is `apply`. It reads one command from stdin and writes the outcome document to stdout. The process does not add a newline.

The process reads at most 1048577 bytes. A longer buffer is `json.budget.bytes` with message `document exceeds the byte budget`, before UTF-8 validation. A read error is `io.read`. Non-UTF-8 input is `json.syntax` with message `stdin is not UTF-8`. A stdout write failure is `io.write`.

Exit 0 is the document. Exit 1 writes nothing to stdout. Stderr is one line:

```text
description-review: <code> <message>
```

Exit 2 writes nothing to stdout.

| Condition | Stderr |
|---|---|
| no argument, or any argument other than `apply` | `description-review: usage: description-review apply` |
| `apply` and a further argument | `description-review: the phase takes no arguments` |

Usage lines:

```text
description-review: usage: description-review apply
description-review:   apply    one oo.builder.description-review-command/v1 document on stdin
description-review: stdout is the review document and has no added newline
description-review: this command does not call a model, open a network connection, read credentials, use PostgreSQL, write files, grant, certify, authenticate, or publish
```

## Out of scope

Provider transport, residency, token cost, publication, effective OOS, column descriptions, PII classification, semantic classification, table ACL, entity edits, metric edits, and owner authentication are later gates. A new expansion needs its own frozen contract before code.

This unit does not claim parent acceptance, a customer-owner proof, or full S5.
