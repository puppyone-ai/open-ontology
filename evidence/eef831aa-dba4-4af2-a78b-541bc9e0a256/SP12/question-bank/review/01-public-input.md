# Review 1: public input

Reviewer is the same session that wrote the crate. This is not an independent second-author acceptance. The tree under review is the attempt 5 source. No source change came out of this pass.

The command reads stdin only with no arguments. `--help` and `-h` write the frozen help bytes, write nothing to stderr, and do not read stdin. Any other argument is `usage`, exit 2, and does not read stdin. A stream longer than 1_048_576 bytes is `budget` at `/` before `parse_json_value`. Invalid UTF-8, a duplicate key, trailing bytes, and the core depth or node budget are `json` with no pointer. JSON null at the root is `schema` at `/`.

Required question fields are present in `QUESTION_KEYS`: id, question, asker, category, difficulty, expected, required metrics, as_of, accepted interpretations, provenance, review, split, version, plus role group, annotations, and control totals. `as_of` must already be the 25-character canonical spelling from `Rfc3339Timestamp`, with an explicit offset and no fraction. `2026-10-01T00:00:00+00:00` is `as_of`. A question version other than `bank_version` is `version`.

Dataset distribution must be `public_synthetic`. Split must be `public`. The reserved words `dev`, `holdout`, `customer`, `private`, and `live` are rejected as dataset, suite, and question ids. The customer-distribution and holdout-split fixtures exit 2 and write no report. Provenance origin must be `public_synthetic_fixture`. Annotator and reviewer must be `unassigned`. The annotator value `li` is `schema` at `/questions/0/provenance/annotator`.

Empty questions are `schema` at `/questions`. More than 64 questions are `budget` at `/questions` before an element is parsed. Categories over 4 are `schema`. Interpretations over 4 and metrics over 8 are `budget`.

Attempt 5 CLI exits for these fixtures are 2, with empty stdout and the contract stderr line. This review does not accept a public bank, a holdout, or a release.
