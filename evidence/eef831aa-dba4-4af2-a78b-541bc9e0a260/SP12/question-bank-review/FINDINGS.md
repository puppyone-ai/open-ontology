# Findings

Verdict in `REPORT.md`: APPROVE_LIMITED for the public synthetic question-bank validator at `c275aa6326ae945ef09534bd6fb5b7f2a5eb938b`. The frozen contract is `source/blobs/benchmarks/question-bank/CONTRACT.md`. The delivered raw cases are `linux/proof-2/independent/`. `linux/proof/` records the earlier driver stop and is not a product failure.

## Hand bank, views, and hashes

Exit 1, stderr empty, denominator kept. `lane-gap` has `unresolved` `missing_gold` and remains in the manifest, the agent view, and the scorer view. `question_count` is 3 on the manifest and on the scorer view. `fully_approved_count` is 2. `needs_reannotation_count` is 1.

The four `A260_*` canaries sit in gold SQL, a typed text cell, a control total, and an annotation. Each is absent from the agent view and present in the scorer view. Forbidden gold, review, provenance, and role-group keys are absent from the agent view.

`hand.stdout`, `hand-repeat.stdout`, and `hand-keyorder.stdout` are byte-identical. Content hashes from the other saved reports:

| Case | Agent | Manifest | Scorer |
| --- | --- | --- | --- |
| hand | `79b7a8f9c96ae091bc34d0ec87a381d335c134145b8816c52140cb51e2295eea` | `520580c277bb7b039da13f6fb7102062a0d7d7d77ceb353651e2349cf31c58b7` | `fcff5ed56c4c5460ea926a77d34cf95288d06666938f3881221ff97466ad48e9` |
| scale `xs` to `s` | unchanged | `e3c85fc4c54e73f86dacde7f5e901d5dbbf132c147a96b3732acdf19332e74f2` | unchanged |
| gold SQL changed | unchanged | `197ce253db4a3a629c00522028466ae2709e9334cd5e04ef6cb50280ac92b716` | `cb4cb03a095d8cd0c2fdab1fad36beaf43d5051bff071de05f0c2dd5c5b04863` |
| question text changed | `78fc6b18070bdcb19d04986e4c6d9b3aca1bce0e9786d00ffb8a22ac47a6bc59` | `5e4e1d8ddb737381c598b5dc5e563a2363a8e6981a8ebba86643058fc8bb77f6` | `5fce8efe56ea6d535a4ee64d47d75afa4c160828ca84578d56f7e2e411f57a59` |
| questions reordered | `3e69aa3bbdb922c20a60f5fa7a183881d158f0f5686519fa27cfd96cc39b97e5` | `d77fb1ce66d521cb38c40464451010849f1a333adede08a42870b29a97264f31` | `feda6025a9e5e63bcc5d80a54109ca699783682f6b356dfe928f0f7dab79c000` |

Scale moves only the manifest hash. Gold SQL moves the scorer hash and the manifest hash. Question text is stored in both views, so that edit moves the agent hash, the scorer hash, and the manifest hash. Reorder moves all three, and the manifest id order becomes `lane-dispatch`, `lane-finance`, `lane-gap`. The finance typed-gold decimal stays coefficient `"120"` scale 2.

`declared_dual_agree` is echoed. The emitted attestation is `label_only`. That echo is not the 20% dual-annotation gate.

## Refusals

Each config case has empty stdout and one stderr line. Exit is 2.

| Case | Stderr |
| --- | --- |
| question version `a260-other-1` | `question-bank: version /questions/0/version` |
| `as_of` ending in `Z` | `question-bank: as_of /questions/0/as_of` |
| `as_of` offset `+00:00` | `question-bank: as_of /questions/0/as_of` |
| duplicate role in one group | `question-bank: schema /questions/1/asker/role` |
| same expected object in one group | `question-bank: role /questions/1/expected` |
| distribution `customer` | `question-bank: schema /dataset/distribution` |
| distribution `private` | `question-bank: schema /dataset/distribution` |
| split `holdout` | `question-bank: schema /questions/0/split` |
| dataset id `private` | `question-bank: schema /dataset/id` |
| suite id `holdout` | `question-bank: schema /suite/id` |
| question id `customer` | `question-bank: schema /questions/0/id` |
| annotator `li` | `question-bank: schema /questions/0/provenance/annotator` |
| reviewed_by `ada` | `question-bank: schema /questions/0/provenance/reviewed_by` |
| relative tolerance | `question-bank: tolerance /questions/0/expected/value_columns/0/tolerance` |
| `fully_approved` plus `unresolved` | `question-bank: schema /questions/0/expected/unresolved` |
| coefficient `0120` | `question-bank: decimal /questions/0/expected/typed_gold/rows/0/0/coefficient` |
| negative zero | `question-bank: decimal /questions/0/expected/typed_gold/rows/0/0/negative` |
| int64 `01` | `question-bank: int64 /questions/0/expected/typed_gold/rows/0/0/value` |
| `questions` `[]` | `question-bank: schema /questions` |
| 65 null questions | `question-bank: budget /questions` |
| 300 null questions | `question-bank: budget /questions` |
| extra argv | `question-bank: usage` |
| stdin 1048577 bytes | `question-bank: budget /` |
| duplicate key | `question-bank: json` |

The 300-element array is refused at `/questions`. It is not an approved 300-question bank. Human names `li` and `ada` are schema failures. They are not owners of a bank. `--help` and `-h` write the same 189-byte help text, stderr empty, exit 0. The oversize input is recorded as `oversize.stdin.note`: 1048577 bytes, sha256 `0b9e54a53cf91811df02190f4d28f611ab40bc65c00fee1646953123cd81a544`. Duplicate-key stdout is empty and the stderr line has no pointer.

## Payload bound

One fully approved question with 64-byte identity strings for bank version, dataset id, suite id, question id, role, and role group exits 0. `question_count` is 1 and `fully_approved_count` is 1. The agent question id is 64 characters. One extra typed-text byte exits 2 with `question-bank: budget /` and empty stdout. Identity bytes stayed outside that cap on this input.

## Still pending

A public 300-question bank, two-human annotation, the 20% dual-annotation gate, private customer holdout, B-D06 acceptance, and a release stay open. The sealed author fixtures were not replayed as this proof. The hand bank uses `A260` canaries.
