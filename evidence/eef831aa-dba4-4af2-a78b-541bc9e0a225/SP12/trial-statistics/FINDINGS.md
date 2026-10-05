# Findings

No product defect remains in the attempt 4 tree. The notes below are boundaries of the frozen contract, not follow-up features.

## Carried into the proof

pass^k is the count of questions whose k trials are all `pass`. The not-pass-at fixture is the witness that a pass, pass, fail question is a miss. The report stores `-1/1`. A pass@1 trial average would be a different rational, and the gold file does not contain it.

`needs_reannotation` leaves the question in the denominator and fails the gate. A question with that status on both variants increments `reannotation_count` once. The gate is measurement validity. It is not a claim that one variant won.

The caller writes `fail` or `needs_reannotation` for a missing run, a cancellation, or an error. The words `missing` and `error` are tested and refused as schema. `cancelled` and `canceled` fall through the same status arm. They have no separate fixture.

Empty questions, empty trials, and an id or tuple value outside its length bound are `budget`. A pattern failure or a control byte is `schema`. Overlong values are `budget` at that field, before the payload sum.

Question arrays pair by position. The program does not sort ids. The point difference is `variants[1]` minus `variants[0]` in input order. Zero reduces to `0/1`. Percentile ranks are `floor(resamples * 25 / 1000)` and `floor(resamples * 975 / 1000)` on the sorted rationals. The sort is stable.

A stdout write error returns token `io` and exit 2. If stderr itself cannot accept the config line, the process returns 2 without a further write. No test installs a failing writer. `Stderr` from the standard library is unbuffered, and the config writer does not call `flush`.

## Out of scope

The tool does not read model prose, call a model, estimate pass@k, or compute a mean, a majority rate, a McNemar test, or a p-value. It does not drop a reannotation question and it does not compare unequal frozen tuples. It does not make B-D03 accepted.

Attempts 1 through 3 failed in rustfmt or clippy and are kept under `linux/impl`, `linux/impl-2`, and `linux/impl-3`. Their containers were removed. They are not the proof.
