# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a242`. Archive `4b15249b70ee38d5a4d21c546b1329e6532ccfed`.

## B-D01 result comparator — APPROVE_LIMITED

Fresh stdin documents, not the author fixtures, produced the frozen report bytes.

- Null versus missing exits 1. The single difference is reason `presence`, path `rows/0/0`, actual presence missing, expected presence null.
- Null matches null. Missing matches missing. Both exit 0 with the empty difference list.
- Decimal coefficient 120 scale 2 matches coefficient 12 scale 1. Coefficient 121 scale 2 mismatches 120 scale 2 with reason `decimal`. Coefficient 12 scale -2 matches 1200 scale 0. Two non-negative zero coefficients at scales 0 and 2 match.
- Ordered `1` then `2` versus `2` then `1` exits 1 with two value differences. The same pair unordered exits 0.
- Unordered multiplicity of two identical `7` rows matches itself and mismatches a single `7` row. The mismatch reason is `multiplicity`, actual `1`, expected `2`, with the canonical row JSON.
- Text `beta` versus `gamma` is reason `value`.
- Partial versus complete and complete versus partial each exit 1 with one completion difference.
- `--help` exits 0 with the frozen help text. `--bogus` exits 2 and stderr ends with `: usage`.

Limit: the tool compares one typed expected result with one typed actual result. It does not score a bank.

## B-D02 query-result adapter — APPROVE_LIMITED

- Header type decimal and gateway text `1.20` emit coefficient `120`, negative false, scale 2. The comparator accepts that actual against expected coefficient `12` scale 1.
- Text `-0.00` exits 2. Stderr is `query-result-adapter: decimal /1/cells/0`.
- A row cell `{"kind":"null"}` beside a column whose ordinal is JSON null emits presence null and presence missing.
- A result-comparison document on adapter stdin exits 2. Stderr starts with `query-result-adapter: `. The canary `GOLD_CANARY_a242` is absent from stderr.
- Compiled `query-pg` against private TLS PostgreSQL 17.10 returned three rows and a summary: succeeded, truncated false, end_audit_ok true, returned_rows 3. Stdout contained no `sealed-` column. Adapter bytes matched the handwritten desk gold, including i64 min and max, SQL null, the note U+96EA U+306E U+7968 U+0301, and JSON booleans. The comparator then matched those bytes.
- Unordered reversal of the three rows matched. Ordered reversal exited 1.
- Replacing the beta cell with gamma produced a value mismatch that contains `gamma`.
- Rewriting the summary status from succeeded to partial kept `"completion":"partial"` and mismatched the complete gold on path `completion`.
- `end_audit_ok` false, with status still succeeded, exits 2 at `query-result-adapter: completion /summary/end_audit_ok`. The summary is not rewritten to failed.
- Dropping the summary exits 2 at `query-result-adapter: protocol /`.

Truncation boundary, measured on the live first row with a replacement summary `succeeded`, `truncated` true, `returned_rows` 1, `end_audit_ok` true:

- Adapter completion is `complete`.
- Adapter stdout does not contain `truncated`.
- Comparator versus the full three-row gold exits 1: actual `1`, expected `3`, path `rows`, reason `length`.
- Comparator versus a one-row prefix gold exits 0.

`linux/proof/harness/truncation-boundary.txt` records `runner_must_refuse_truncation=independent` and `runner_approved=no`. The frozen adapter contract treats that summary as a complete actual. The mismatch against full gold is necessary and not sufficient. A runner has to refuse the truncated stream itself. This archive does not contain the SQL runner, and this review does not approve one.

Limit: the adapter emits one typed actual. It does not decide that a truncated complete result is a full answer.

## B-D03 trial statistics — APPROVE_LIMITED

The harness reimplements SplitMix64, rejection sampling, reduced rationals, and stable cross-multiply ordering. It compares CLI stdout bytes. It does not call `evaluate`.

- Seed 0 matches the four published draws. The n=2 tail rejects `u64::MAX` and `u64::MAX-1`. The n=1 rule accepts 0 and rejects `u64::MAX`.
- One question, k=3, left all pass, right pass/pass/fail: exit 0, gate passed, point difference `{"denominator":1,"numerator":-1}`. No denominator of 3 appears. The repeat run is byte-identical.
- Two questions, seed `7`, 4 resamples: exit 0 and the CLI bytes match the oracle. Ranks are `floor(4*25/1000)=0` and `floor(4*975/1000)=3`.
- `needs_reannotation` on question 0: `question_count` stays 2, `reannotation_count` is 1, gate `failed`, exit 1.
- One missing trial on the right copy of question 1 exits 2 at `trial-statistics: trials /variants/1/questions/1/trials`.
- Right project `other` exits 2 at `trial-statistics: tuple /variants/1/tuple/project`.

Limit: the checked oracle is this small enumerable set. It is not a customer benchmark. pass^k is the fraction of questions whose every trial is pass. A question that needs reannotation stays in the denominator and fails the gate.

## Harness failures that are not product defects

Attempt 1 stopped because `rustfmt` is not installed for toolchain 1.94.0 in the pinned image. The proof no longer runs rustfmt. Clippy is installed with `rustup component add clippy --toolchain 1.94.0`, the same step as the a200 smoke script.

Attempt 2 was harness clippy. The product crates were not the failing package.

Attempt 3 reached a green fixture test and green product clippy, then `a242-review` was absent because clippy had only written metadata. `cargo build --locked --offline` now produces the binary before the live chain.

Attempt 4 ran the live chain, wrote the truncation boundary, and then the secret scan found the detector spelling inside a preserved copy of the harness source under `attempts/`. The walk skips `attempts`. The successful run's scan case passed. No password bytes and no private-key text were reported in the successful run's outputs.

## Checks that stayed green on the successful run

Product clippy `-D warnings` exited 0 for each crate. Product bytes were unchanged. The archive shape check exited 0 with `benchmarks/sql-runner` absent. Offline workspace fetch on the successful run exited 0 because attempt 3 had already filled the private cargo volume. Attempt 3 records the cold offline failure: `fetch-workspace-offline.exit` 101, then online fetch 0. Libclang extract exited 0. `libclang-load.exit` is 0.
