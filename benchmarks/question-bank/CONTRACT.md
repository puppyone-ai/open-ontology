# Public question bank v1

Protocol id: `oo.benchmark.question-bank/v1`.

Report id: `oo.benchmark.question-bank-report/v1`.

Manifest id: `oo.benchmark.question-bank-manifest/v1`.

Agent-view id: `oo.benchmark.question-bank-agent-view/v1`.

Scorer-view id: `oo.benchmark.question-bank-scorer-view/v1`.

This document is the frozen input and output of one public synthetic question bank. The program validates the bank and writes three documents: a manifest, an agent view, and a scorer view. Each document has a SHA-256 content hash. The manifest binds the dataset and suite identity to those hashes.

This is not a 300-question bank, a generator for xs/s/m/l rows, a scorer, a judge, a pass^k calculator, a bootstrap, or a customer holdout. Review labels are declared strings. They are not an authenticated signoff and they are not evidence that two humans annotated anything. The 300-question public bank, the two-human annotation, and the 20% independent dual-annotation gate stay outside this protocol.

The crate that implements this document is a standalone workspace. It links `oo-core` from the fixed source archive and uses serde 1.0.229 and serde_json 1.0.151. It does not edit the root manifest or the root lock. It does not call a model, open a socket, or open PostgreSQL.

## Command

```text
question-bank
question-bank --help
```

No other arguments. `--help` and `-h` write the help text below to stdout, write nothing to stderr, do not read stdin, and exit 0. Any other argument list writes `question-bank: usage` to stderr, writes nothing to stdout, does not read stdin, and exits 2.

Help text, including the final newline:

```text
question-bank reads oo.benchmark.question-bank/v1 from stdin.
It writes oo.benchmark.question-bank-report/v1 to stdout.
exit 0 fully approved labels
exit 1 needs reannotation
exit 2 config
```

With no arguments the process reads stdin until EOF, capped at 1_048_577 bytes. A longer stream is refused before parse. The bytes must be UTF-8. The process writes one compact JSON report and a final newline to stdout when the document is a bank. Stderr is empty for exit 0 and for exit 1.

| Exit | Stdout | Stderr |
| --- | --- | --- |
| 0 | report, every question label is `fully_approved` | empty |
| 1 | report, at least one question label is `needs_reannotation` | empty |
| 2 | empty | `question-bank: <token>` or `question-bank: <token> <pointer>` |

Tokens are `usage`, `stdin`, `json`, `schema`, `budget`, `decimal`, `int64`, `version`, `as_of`, `review`, `tolerance`, `role`, and `io`. A pointer is a JSON pointer into the input and does not contain a value from the document. A read failure is `stdin`. Invalid UTF-8, broken JSON, a duplicate key, trailing bytes, and the core depth or node budget are `json`. A stdout or stderr write failure is `io`. Those three tokens have no pointer.

Exit 1 still writes the full question denominator. A question marked `needs_reannotation` stays in the manifest, the agent view, and the scorer view. Exit 1 is the bank's own label gate. It is not a customer acceptance result and it does not delete the question.

An ill-formed document, a private or holdout or customer identity, or a `fully_approved` label that the rules below reject is config, exit 2, and produces no report. Those documents are not accepted and are not rewritten into a public output.

## Document

One JSON value. `oo_core::parse_json_value` rejects a document larger than `DOCUMENT_MAX_BYTES` (1_048_576), depth past 32, more than 50_000 nodes, a duplicate key, and trailing bytes. JSON null is rejected in every field. Unknown keys are rejected. A JSON number is accepted only for a typed-gold decimal `scale`.

Object key order in the input is not significant. Array order is significant and is preserved. The writer emits compact JSON with object keys sorted by UTF-8 bytes. Two inputs that differ only by object key order produce the same report bytes.

Top-level keys, all required:

| Key | Value |
| --- | --- |
| `version` | the string `oo.benchmark.question-bank/v1` |
| `bank_version` | 1 to 64 bytes, `[a-z][a-z0-9._:-]*` |
| `dataset` | dataset identity |
| `suite` | suite identity |
| `questions` | 1 to 64 questions |

A bank version that fails the spelling is `schema` at `/bank_version`. A question array longer than 64 is `budget` at `/questions`. An empty array is `schema` at `/questions`.

## Dataset and suite

Dataset keys, all required: `id`, `distribution`, `scale`.

| Key | Rule |
| --- | --- |
| `id` | 1 to 64 bytes, `[a-z][a-z0-9-]*`, and not `dev`, `holdout`, `customer`, `private`, or `live` |
| `distribution` | the string `public_synthetic` |
| `scale` | `xs`, `s`, `m`, or `l` |

Suite keys, all required: `id`, `track`. `id` uses the dataset id rule. `track` is one of `A`, `B`, `C`, `D`, `E`, `F`.

Any other distribution, including `customer`, `private`, `holdout`, and `dev`, is `schema` at `/dataset/distribution`. The scale is an identity label. This program does not generate rows for that scale.

## Question

Question keys, all required:

| Key | Rule |
| --- | --- |
| `id` | 1 to 64 bytes, `[a-z][a-z0-9-]*`, unique in the bank, and not a reserved identity word |
| `question` | 1 to 500 Unicode scalars, no NUL, no ASCII control, no `0x7F` |
| `asker` | role and explicit claims |
| `category` | 1 to 4 distinct category words, order preserved |
| `difficulty` | `easy`, `medium`, or `hard` |
| `expected` | the expected object below |
| `required_metrics` | 0 to 8 metric ids, unique, order preserved |
| `as_of` | the anchor below |
| `accepted_interpretations` | the array below |
| `provenance` | the three fixed labels below |
| `split` | the string `public` |
| `version` | the same bytes as `bank_version` |
| `role_group` | 1 to 64 bytes, `[a-z][a-z0-9-]*` |
| `review` | the declared labels below |
| `control_totals` | 0 to 8 scorer-only totals |
| `annotations` | 0 to 8 scorer-only notes |

Reserved identity words are `dev`, `holdout`, `customer`, `private`, and `live`. A second question with the same id is `schema` at `/questions/<n>/id`.

A question `version` that is not the bank version is `version` at `/questions/<n>/version`. A bad `as_of` is `as_of` at `/questions/<n>/as_of`. A `split` other than `public` is `schema` at `/questions/<n>/split`. `dev`, `holdout`, `customer`, `private`, and `live` are not accepted.

`as_of` is 25 characters: `YYYY-MM-DDTHH:MM:SS` plus an explicit `+HH:MM` or `-HH:MM` offset. It must already equal the spelling returned by `oo_core::rfc3339::Rfc3339Timestamp`. No fraction and no `Z`. `+00:00` is not that spelling, because the core normalizer rewrites a zero offset to `Z`. The anchor is the per-question date, not a system clock.

Category words are `single_table`, `multi_table`, `metric`, `entity_join`, `code`, `time_relative`, `top_k`, `permission`, `not_found`, `ambiguity`, `multi_turn`, `entity_ambiguity`, `time_anchor`, `data_quality`, `undetermined_code`, `injection`, and `large_result`.

A metric id is `[a-z][a-z0-9_]{0,63}`.

## Asker

Keys, all required: `role`, `claims`.

`role` is `[a-z][a-z0-9_]{0,63}`.

`claims` is an object with 0 to 8 keys. Each key uses the role spelling. Each value is an array of 1 to 4 strings. Each string is 1 to 64 bytes, with no NUL, no ASCII control, no `0x7F`, and no `/`. An empty object is an explicit empty claim set. There is no hidden claim.

Inside one `role_group`, roles are unique. A repeated role is `schema` at the later question's `/asker/role`.

## Expected

`kind` is `table`, `scalar`, `top_k`, `not_found`, `refuse_permission`, or `clarify`.

`gold_sql`, when present, is 1 to 2000 bytes, with no NUL, no ASCII control, and no `0x7F`. This program does not parse or execute that SQL.

`ordered`, when present, is a boolean. `top_k` and `scalar` require `true`. `table` may be `true` or `false`. `true` means the result-comparison `row_comparator` value `ordered`. `false` means `unordered`. This program does not compare rows.

`key_columns` is an array of column names. `value_columns` is an array of objects with keys `name` and `tolerance` only. `tolerance` is the string `exact` or the string `relative`. A name is 1 to 256 bytes, is not empty, and contains no NUL, no ASCII control, no `0x7F`, and no `/`. Names are unique across the key list and the value list together.

`typed_gold`, when present, is one expected side of `oo.benchmark.result-comparison/v1`. It is not a comparison document: no `version`, no `row_comparator`, and no `actual`. Its warning array is empty. Its `completion` is `complete`. `partial` and `failed` are `schema`. Cell rules match that protocol, including `oo_core::ExactDecimal` for decimal cells and canonical `i64` text for int64 cells. A leading-zero coefficient, a negative zero coefficient, and a non-integer scale are `decimal`. A non-canonical int64 spelling is `int64`. Row and column budgets for this bank are 32 rows and 16 columns. A text cell is 0 to 500 Unicode scalars. This program does not apply a relative tolerance and does not call the comparator.

`unresolved`, when present, is `missing_gold`, `ambiguity`, `tolerance_not_exact`, `dual_disagree`, or `declared_unresolved`.

Kind shapes:

| Kind | Fully approved gold | Notes |
| --- | --- | --- |
| `table` | `gold_sql`, `ordered`, `key_columns`, `value_columns`, `typed_gold` of kind `rows` | every typed column is listed once as a key or a value |
| `scalar` | same, `ordered` true, one value column, no key columns | typed gold has one column and one row |
| `top_k` | same, `ordered` true, at least one key column, at least one row | ties are not inferred |
| `not_found` | `gold_sql` and `typed_gold` of kind `not_found` | no ordered, key, or value fields |
| `refuse_permission` | `gold_sql` only | no typed gold and no ordered, key, or value fields |
| `clarify` | `gold_sql` plus a row or `not_found` typed gold, and the matching fields for that typed kind | the one selected interpretation has the same `gold_sql` |

`not_found` typed gold has keys `completion`, `kind`, and `warnings` only. Row typed gold has keys `columns`, `completion`, `kind`, `rows`, and `warnings` only.

Column order in typed gold is the row order. A typed column name and type must match the declared key or value name. A value column's typed type may be `bool`, `int64`, `decimal`, or `text`. A key column's typed type may be those same four. This program checks the name binding, not a business type.

`accepted_interpretations` is empty for every kind except `clarify`. A clarify question has 1 to 4 interpretations. Each object has `id`, `statement`, and `selected`. `id` uses the question id spelling and is unique in the array. `statement` is 1 to 200 Unicode scalars with no NUL and no ASCII control. `selected` is a boolean. `gold_sql` is required when `selected` is true and forbidden when it is false. It uses the gold_sql rule.

## Provenance, review, totals, annotations

Provenance keys, all required, and no other values are accepted:

| Key | Value |
| --- | --- |
| `origin` | `public_synthetic_fixture` |
| `annotator` | `unassigned` |
| `reviewed_by` | `unassigned` |

A personal name, a customer name, or any other owner string is `schema`. This protocol does not record a fictional human owner.

Review keys, all required:

| Key | Value |
| --- | --- |
| `attestation` | the string `label_only` |
| `status` | `fully_approved` or `needs_reannotation` |
| `dual_annotation` | `not_declared`, `declared_single`, `declared_dual_agree`, or `declared_dual_disagree` |

`label_only` means the status is a declared label. It is not a signature, a credential, or a claim that a human reviewed the row. `declared_dual_agree` does not prove the 20% dual-annotation gate.

A control total has `name` and `value` only. `name` is a metric id. `value` is 1 to 200 Unicode scalars with no NUL and no ASCII control. The value is opaque text. It is not a comparator cell.

An annotation has `label` and `text` only. `label` is `[a-z][a-z0-9_]{0,31}`. `text` uses the control-total value rule.

## Approval rules

The program computes one defect, in this order, and does not look further:

1. `ambiguity`, when the question is `clarify` and the number of selected interpretations is not 1.
2. `missing_gold`, when a fully shaped gold for that kind is absent. An empty gold_sql is not a value; the key is absent.
3. `tolerance_not_exact`, when any value column has tolerance `relative`.
4. `dual_disagree`, when `dual_annotation` is `declared_dual_disagree`.
5. Otherwise there is no gold defect.

`fully_approved` is emitted only when the declared status is `fully_approved`, `unresolved` is absent, and there is no gold defect. Any gold defect on a declared `fully_approved` question is config. Defects 1, 2, and 4 are `review` at `/questions/<n>/review/status`. Defect 3 is `tolerance` at the first relative `/questions/<n>/expected/value_columns/<i>/tolerance`.

`needs_reannotation` is emitted when the declared status is `needs_reannotation` and `unresolved` equals the computed defect. When there is no gold defect, `unresolved` must be `declared_unresolved`. A mismatch is `review` at `/questions/<n>/expected/unresolved`. The program does not upgrade `needs_reannotation` to `fully_approved`.

Role variants: when a `role_group` has two or more questions, their canonical expected objects must not be equal. The later equal expected object is `role` at `/questions/<n>/expected`. Canonical expected bytes are the compact key-sorted JSON of that object. Question text may match across the group. The expected answer must differ.

The internal payload cap is 16_384 bytes. Payload bytes are the UTF-8 lengths of question text, gold SQL, statements, annotation text, control-total values, claim strings, and typed-gold text, int64 spellings, decimal coefficients, column names, and warning fields, plus 1 byte for every typed cell. Crossing the cap is `budget` at `/`.

## Report

Compact JSON, one trailing newline. Keys are sorted.

```json
{"agent_view":{},"content_hashes":{},"manifest":{},"scorer_view":{},"version":"oo.benchmark.question-bank-report/v1"}
```

`content_hashes` has `agent_view`, `manifest`, and `scorer_view`. Each value is 64 lowercase hex characters. The digest is `oo_core::canonical::hash_bytes` over the compact key-sorted JSON of that document, with no trailing newline. The manifest document contains `agent_view_sha256` and `scorer_view_sha256`, and those two strings equal the hashes of the two views. The manifest hash covers the manifest document, including those two strings. A change to gold SQL, a typed cell, a control total, or an annotation changes the scorer-view hash and the manifest hash. It does not change the agent-view hash. A change to question text, asker, category, difficulty, required metrics, `as_of`, split, or question version changes the agent-view hash. A change to dataset scale changes the manifest hash and does not change the agent-view hash or the scorer-view hash. Reordering the question array changes all three hashes. Repeating the same input produces the same report bytes.

## Manifest

| Key | Value |
| --- | --- |
| `version` | `oo.benchmark.question-bank-manifest/v1` |
| `bank_version` | the bank version |
| `dataset_id` | dataset id |
| `distribution` | `public_synthetic` |
| `scale` | the scale label |
| `suite_id` | suite id |
| `track` | the track letter |
| `question_count` | the number of questions, including `needs_reannotation` |
| `fully_approved_count` | the number of `fully_approved` labels |
| `needs_reannotation_count` | the number of `needs_reannotation` labels |
| `agent_view_sha256` | the agent-view hash |
| `scorer_view_sha256` | the scorer-view hash |
| `questions` | one object per input question, in input order |

Each manifest question has `id`, `role_group`, `review_status`, and `dual_annotation` only. The two counts sum to `question_count`.

## Agent view

Keys: `version`, `dataset_id`, `suite_id`, `bank_version`, `questions`.

`version` is `oo.benchmark.question-bank-agent-view/v1`.

Each question has `id`, `question`, `asker`, `category`, `difficulty`, `required_metrics`, `as_of`, `split`, and `version` only. The asker is the role and the explicit claims.

The agent view has no `expected`, `gold_sql`, `typed_gold`, `ordered`, `key_columns`, `value_columns`, `accepted_interpretations`, `control_totals`, `annotations`, `provenance`, `review`, `role_group`, `unresolved`, or `tolerance`. A planted gold canary that appears in gold SQL, a typed cell, a control total, or an annotation must be absent from the agent-view bytes.

## Scorer view

Keys: `version`, `dataset_id`, `suite_id`, `bank_version`, `question_count`, `questions`.

`version` is `oo.benchmark.question-bank-scorer-view/v1`. `question_count` is the full denominator.

Each question has the input question fields, with the expected object and the review labels that the approval rules emitted. Typed gold stays on the expected object. Control totals and annotations stay here. Nothing in this view is removed to raise an approval count.

## Hash

The bank hash is the content hash above. The crate also exposes `sha256_hex`, which calls `oo_core::canonical::hash_bytes`. Tests check that function against the FIPS 180-4 vectors for the empty string, `abc`, and the 448-bit alphabet string.

## What this protocol does not do

It does not generate 300 questions. It does not invent a human annotator or a human reviewer. It does not accept a private, holdout, customer, dev, or live bank into a public document. It does not put gold SQL, expected answers, control totals, or annotations in the agent view. It does not treat a declared review label as a signature. It does not execute gold SQL. It does not apply a relative decimal tolerance. It does not score an agent answer. It does not claim that SP12-W01, AT-EVAL-01, AT-EVAL-02, or a release has passed.
