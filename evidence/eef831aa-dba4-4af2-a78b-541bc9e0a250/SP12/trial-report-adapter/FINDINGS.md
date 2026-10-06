# Findings

No product defect remains in the attempt 3 tree. The notes below are boundaries of the frozen contract, not follow-up features.

## Carried into the proof

`pass` stays `pass`. `timeout`, `refusal`, `fail`, and `not_run` become `fail`. `needs_reannotation` stays that status and stays in the question denominator. The adapter exit is 0 when the statistics input is written, including an all-fail vector and a reannotation vector. Exit 2 is config. There is no adapter exit 1.

The statistics CLI then applies its own gate. On the live k=2 pair that gate is `failed` because one question needs reannotation. On the live all-fail pair the gate is `passed` because every trial is `fail` and `reannotation_count` is 0. Those gate words come from `trial-statistics`. This crate does not calculate them.

Question arrays pair by position. A swapped case id is `schema` at `/reports/1/trials/{index}/case_id`. A short vector, a hole, or an extra case is `trials` or `schema` at `/reports/1/trials`. Nothing is padded, dropped, or imputed. A lying count is `schema` at that count field even when the other counts still sum.

`tuple.model` must be the string `none`. The output model is `trusted_sql`, the runner lane. A model name, the token `unknown`, and the token `production` are refused. `snapshot` and `as_of` are copied declared labels. They are not a snapshot certificate. Executable hashes are checked for three lowercase 64-hex digests and are not copied. The two reports may name different executable hashes. `mode` is declared and is not required to be equal.

Budget and scorer labels must equal both reports. A disagreement uses token `tuple`, not a separate label token. The ten tuple fields must match, in the order project, snapshot, as_of, dataset, suite, model, harness, role, budget, scorer_version. The first mismatch stops the comparison.

The handwritten right note row mixes `needs_reannotation` with `timeout`. The runner's annotation rule rewrites every non-`not_run` trial of an annotated case, so one live case does not emit that mix. The fixture stays because the adapter accepts a legal report row the runner did not produce in this proof.

## Out of scope

The tool does not read model prose, call a model, estimate pass@k, or compute a mean, a majority rate, a McNemar test, or a p-value. It does not drop a reannotation question and it does not compare unequal frozen tuples. It does not open PostgreSQL. It does not make B-D05 accepted.

Attempts 1 and 2 failed in the CLI byte search and in rustfmt. They are kept under `linux/impl` and `linux/impl-2`. Their containers were removed. They are not the proof.
