# Handwritten fixtures

These files were written from `CONTRACT.md` before the implementation. The reports are not program output.

`k3-passed.json` is the worked seed-0 example. Left bits are 0, 1, 0. Right bits are 1, 0, 1. Counts are 1 and 2 of 3. The point difference is 1/3. Four resamples sort to `-1/3`, `-1/3`, `-1/3`, `1/1`. Ranks 0 and 3 are `-1/3` and `1/1`. The gate is passed. `q0` on the left and `q1` on the right and `q2` on the left use `fail` as the retained score for a miss. Those questions stay in the denominator.

`k3-reannotation.json` changes left `q1` trial 0 to `needs_reannotation`. The question count stays 3. The left pass^k count becomes 0. The point difference becomes 2/3. `reannotation_count` is 1. The gate is failed. Exit 1 still writes the report.

`k3-not-pass-at.json` is one question, k = 3. Left passes all three trials. Right fails the last. pass^k differs by `-1/1`. A pass@1 average of the six trial bits would differ by `-1/3`. The report says `-1/1`.

`mixed-tuple.json` changes the right model. That is config, token `tuple`, and no report.

`missing-trial.json` drops one required index. That is config, token `trials`, and no report.

`duplicate-key.json` repeats `version`. That is config, token `json`, and no report.
