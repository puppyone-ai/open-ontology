# Findings

No product defect remains in the attempt 5 tree. The notes below are boundaries of the frozen contract, not follow-up features.

## Corrected before the delivered proof

Attempt 3 added bank version, dataset id, suite id, question id, role, role group, and `as_of` to the payload. The contract does not list those fields. Attempt 5 starts the sum at zero. The boundary test accepts 16_384 contract bytes and refuses one more byte at `/`.

Column names are charged on the typed-gold column. The boundary fixture's value-column name is that same name and is not charged a second time. A non-empty warning array is `schema`, so warning-field bytes are never added.

## Carried boundaries

A `fully_approved` question that also has an `unresolved` key is `schema` at that key before the gold-defect token is chosen. The missing-gold fixture omits the key and is `review` at `/questions/0/review/status`.

Category count above 4, interpretation count above 4, and metric count above 8 follow the contract tokens in the parser. This unit has no separate fixture for those three caps. Question count 0 and 65 are tested.

`declared_dual_agree` is echoed. It is not evidence that two people annotated the question. Provenance rejects every owner other than `unassigned`.

A stdout write error on `--help` returns token `io` and exit 2. If stderr itself cannot accept the config line, the process returns 2 without a further write. The config writer does not call `flush`.

The launcher's docker exit is the last non-zero phase, not the numeric maximum. Attempt 1's docker exit 2 is the missing binary after the compile error. The phase files are the record.

## Out of scope

The tool does not generate 300 questions, invent a human annotator, score an answer, execute gold SQL, apply a relative tolerance, or call a model. It does not accept a private, holdout, or customer bank into a public document. It does not make B-D06, SP12-W01, or a release accepted.

Attempts 1, 2, and 4 failed and are kept. Attempt 3 passed on the earlier payload accounting and is kept. Their containers were removed. Attempt 5 is the proof.
