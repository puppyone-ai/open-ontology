# Findings

Verdict in `REPORT.md`: APPROVE_LIMITED for the paired trial-report adapter at `8e7925d7f4576cb6a9f4027eca66ab1671e3fd1a`. The frozen contract is `source/blobs/benchmarks/trial-report-adapter/CONTRACT.md`. The adapter reads two `oo.benchmark.sql-runner-report/v1` documents and writes one `oo.benchmark.trial-statistics/v1` document. It does not compute pass^k, a bootstrap, a percentile, or a gate.

## Status map on real runner reports

k=2 left, raw then mapped: `pass/match` stayed `pass`; `fail/mismatch` became `fail`; `fail/refused` became `fail`; `timeout/timeout` became `fail`; both `needs_reannotation` rows stayed; both slow timeouts became `fail`; both drop passes stayed `pass`. k=2 right: two desk passes stayed; two gate mismatches became `fail`; both note rows stayed `needs_reannotation`; two slow passes stayed `pass`; drop `pass` stayed and drop `not_run` became `fail`.

The full raw rows, including `backend_cleanup`, are in `REPORT.md` and in `linux/proof/independent/k2-*-runner.stdout`. The mapped lines are `vectors-adapter.txt`. Adapter exit on this pair is 0. Stderr is empty.

`needs_reannotation` remained on four trials. The consumed statistics report has `question_count` 5, `reannotation_count` 1, `trial_count` 2, and pass^k denominators 5. The question stayed in the denominator. Statistics exit is 1 and `gate` is `failed` because the reannotation count is not zero. This review does not re-review that calculator.

## All-fail gate is structural

All-fail left raw order is `timeout`, `refused`, `mismatch`, `not_run`. Right raw order is four `mismatch` rows. The adapter mapped every trial to `fail`. Its stdout has no `"status":"pass"` and no `needs_reannotation`.

Statistics exit is 0. `gate` is `passed`. `question_count` is 1, `reannotation_count` is 0, `trial_count` is 4, and both `pass_k_count` values are 0. `linux/proof/independent/structural-gate.txt` says:

```
allfail_statistics_exit=0
allfail_gate=passed
allfail_question_count=1
allfail_reannotation_count=0
allfail_trial_count=4
allfail_pass_k_count=0,0
meaning=structural
quality_winner=no
reason=The consumed trial-statistics gate is passed when reannotation_count is 0. Both pass_k_count values are 0. The word passed does not say either side won.
```

That file was written only after the driver had checked those fields. The word passed is the structural gate. It is not a quality winner.

## Ordered tuple, budget, scorer, case order

Tuple comparison order in the frozen contract is `project`, `snapshot`, `as_of`, `dataset`, `suite`, `model`, `harness`, `role`, `budget`, `scorer_version`. The live tuple pair differs in `project` (`dispatch` against `other`) and also in `budget` (`k1` against `k1b`). The adapter stopped at the first field. Stderr is `trial-report-adapter: tuple /reports/1/tuple/project`. Stdout is empty. Exit is 2.

A separate pair differs only in `budget`. Stderr is `trial-report-adapter: tuple /reports/1/tuple/budget`.

A label budget `other` on two copies of one real report, whose tuple budget is `k1`, yields `tuple /labels/budget`. A label scorer `oo.benchmark.other-scorer/v1` against report scorer `oo.benchmark.sql-runner/v1` yields `tuple /labels/scorer_version`. Changing that one scorer string inside the second real report yields `tuple /reports/1/tuple/scorer_version`.

Two real passes ordered `desk` then `gate`, against `gate` then `desk`, yield `schema /reports/1/trials/0/case_id`. The adapter did not realign the cases.

## Missing trial

The base runner report is a k=2 pass, runner exit 0, `trial_count` 2, `pass_count` 2. The driver deleted the second trial object from the second copy and set that copy's `trial_count` and `pass_count` to 1, so the counters match the remaining object. The adapter saw trial lengths 2 and 1. Stderr is `trial-report-adapter: trials /reports/1/trials`. Stdout is empty. Exit is 2. No trial was imputed and no shorter document was written.

The sealed `tests/bounds.rs` run covers a hole, a lying pass count, a short vector, an extra case, and a swapped order on synthetic documents. Those 10 tests passed in smoke and again in the proof. The live missing-trial row above is the independent driver, not that synthetic file.

## What the output keeps

On both successful live documents the output tuple copies the runner tuple and replaces `model` `none` with `trusted_sql`. `snapshot` stays `declared-a255`. `executable_sha256` is absent. `mode` is absent. `backend_cleanup` is absent. The k=2 runner still has `backend_cleanup` `unknown` on the nine spawned right-side trials and `null` on the final `not_run`. The all-fail left runner has `unknown`, `unknown`, `unknown`, `null`. Those words stay in the runner reports. The adapter does not copy them and does not turn `unknown` into a cleanup certificate.

`linux/proof/independent/scope.txt` records `postgres=not_used`, `private_tls=not_claimed`, `pgpassword=unset`, `snapshot=declared_label`, and `model_lane=trusted_sql`.

## Sealed checks

`cargo fmt --check` exited 0 with empty output. Clippy `--locked --offline --all-targets -- -D warnings` exited 0. The author's `tests/proof.rs` re-ran inside the sealed cargo test, exit 0, 8.09s, with its own `A250` markers under `linux/proof/sealed-proof/`. That sealed live proof is a focused check. The independent driver adds the live budget pair, the label scorer, and the swapped case order, and it uses `A255` markers. The sealed statistics files show the same structural shape as the independent run: k=2 gate `failed` with counts 5, 1, 2 and pass_k_count 1 then 2; all-fail gate `passed` with counts 1, 0, 4 and pass_k_count 0, 0. Those sealed files are the author's harness, re-executed. They are not a second review of trial-statistics.

## Out of scope

sql-runner at this commit was compiled and driven. Its classification behavior is input to the adapter. This review does not accept the runner again. trial-statistics was compiled and given the adapter's stdout. This review checks the gate word and the counts listed above. It does not accept the bootstrap, the percentile arithmetic, or a quality ranking.

No question bank, model name, customer snapshot, or release gate was exercised. The stub answers `query-pg`, the query-result adapter, and the result comparator. A real database was not part of this proof. The plan's aggregate figures are unchanged.
