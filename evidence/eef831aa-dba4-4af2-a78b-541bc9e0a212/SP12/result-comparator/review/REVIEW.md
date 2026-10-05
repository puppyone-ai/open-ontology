# Review of the frozen comparator

Reviewer is the same session that wrote the crate. This is not an independent second-author acceptance. Scope is `CONTRACT.md`, the attempt 5 sources, and the attempt 5 exits. No new test was added in this pass. Attempts 1 through 4 stayed untouched.

## Checked against the contract

The command reads stdin only with no arguments. `--help` and `-h` print the frozen help bytes and do not read stdin. Any other argument is `usage` and does not read stdin. Exit 0, 1, and 2 match the contract table. Config lines are `result-comparator: <token>` or `result-comparator: <token> <pointer>`.

`parse_json_value` is the JSON gate. Oversize input is `budget` `/` before that parser runs, so a core byte error is not relabeled `json`. Duplicate keys, depth, nodes, and trailing bytes are `json` with no pointer. Unknown keys use the alphabetically first extra key.

Kind, completion, and warnings are compared before rows. Either side `not_found` stops the walk. Column mismatch stops the walk. Ordered rows emit one `length` difference and then the shared prefix. Unordered rows emit `multiplicity` sorted by the canonical row string. `partial` differs from `complete` in both directions. `failed` differs from `complete`.

Decimal comparison calls `rescale_exact` except for the frozen zero-coefficient rule. The gold file `mismatch-decimal.json` is 120 versus 121 at scale 2, so a float tolerance would be visible and is not used. Int64 values are strings. The extremes are in `match-exact-decimal.json`. `-0`, leading zeros, `+`, fraction, and exponent are config.

## Proof read

`linux/impl-5` docker exit 0, test exit 0, fmt exit 0, clippy exit 0, cli exit 0, lock-drift empty, rustc `1.94.0`. Test stdout records 35 passed and 0 failed. CLI exits are 0 for the five match fixtures and 1 for the eight mismatch fixtures. Host config is 6 GiB, one CPU, capabilities dropped, not privileged.

## Result

No change to the crate. The empty-length token, the folded scale `-0`, and the untested `io` writer are recorded in `FINDINGS.md`. They do not contradict a tested clause and they are not a reason to add cases in this pass.

This review does not accept B-D01.
