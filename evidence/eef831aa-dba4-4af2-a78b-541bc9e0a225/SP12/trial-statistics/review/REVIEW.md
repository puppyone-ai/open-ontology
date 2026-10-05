# Review of the frozen trial statistics

Reviewer is the same session that wrote the crate. This is not an independent second-author acceptance. Scope is `CONTRACT.md`, the attempt 4 sources, and the attempt 4 exits. No new test was added in this pass. Attempts 1 through 3 stayed untouched.

## Checked against the contract

The command reads stdin only with no arguments. `--help` and `-h` print the frozen help bytes and do not read stdin. Any other argument is `usage` and does not read stdin. Exit 0, 1, and 2 match the contract table. Config lines are `trial-statistics: <token>` or `trial-statistics: <token> <pointer>`.

`parse_json_value` is the JSON gate. Oversize input is `budget` `/` before that parser runs, so a core byte error is not relabeled `json`. Duplicate keys, depth, nodes, and trailing bytes are `json` with no pointer. Unknown keys use the alphabetically first extra key. `1e3` and `1.5` stay non-integers and are schema at `/resamples`.

pass^k is 1 only when every trial is `pass`. `fail` and `needs_reannotation` contribute 0 and stay in the denominator. `reannotation_count` counts a question once when either variant carries that status. The gate is `failed` exactly when that count is not zero. The k=3 gold reports are byte-compared, including the reannotation denominator and the not-pass-at point `-1/1`.

The ten tuple fields compare in contract order. The first mismatch is `tuple` at `/variants/1/tuple/<field>` and produces no report. Question ids pair by index. A length inside 1..=8 that differs from the first question is `trials`. A length outside 1..=8 is `budget`.

SplitMix64 uses the contract constants. The known seed-0 vector and the seed-1 first output are asserted against both the library and an independent copy in the test file. Unbiased indexes reject the tail: n=2 rejects `u64::MAX` and `u64::MAX-1`; n=3 rejects `u64::MAX`. One resample draws n indexes and applies that list to both variants. Ranks are integer division of `resamples * 25 / 1000` and `resamples * 975 / 1000`. The 80-resample test builds the expected report with its own PRNG, its own reduction, and its own sort, then compares those bytes to `evaluate`.

## Proof read

`linux/impl-4` docker exit 0, test exit 0, fmt exit 0, clippy exit 0, cli exit 0, lock-drift empty, rustc `1.94.0`. Test stdout records 17 passed and 0 failed. CLI exits are 0, 1, and 0 for the three gold fixtures, and 2 for mixed tuple, missing trial, and duplicate key. Host config is 6 GiB, one CPU, capabilities dropped, not privileged.

## Result

No change to the crate. The untested `io` writer, the shared schema arm for `cancelled` and `canceled`, and the unflushed config stderr are recorded in `FINDINGS.md`. They do not contradict a tested clause and they are not a reason to add cases in this pass.

This review does not accept B-D03.
