# Paired trial statistics v1

Protocol id: `oo.benchmark.trial-statistics/v1`.

Report id: `oo.benchmark.trial-statistics-report/v1`.

Algorithm id: `oo.benchmark.paired-question-bootstrap/v1`.

This document is the frozen input and output of one paired trial statistic. It reads explicit typed score records and writes exact pass^k counts plus one reproducible paired question bootstrap. It is not a question bank, a scorer, a judge, a pass@k estimator, an HTML report, a cost report, or an acceptance of a quality benchmark.

The crate that implements this document is a standalone workspace. It links `oo-core` from the fixed source archive and uses serde 1.0.229 and serde_json 1.0.151. It does not edit the root manifest or the root lock.

## Command

```text
trial-statistics
trial-statistics --help
```

No other arguments. `--help` and `-h` write the help text below to stdout, write nothing to stderr, do not read stdin, and exit 0. Any other argument list writes `trial-statistics: usage` to stderr, writes nothing to stdout, does not read stdin, and exits 2.

Help text, including the final newline:

```text
trial-statistics reads oo.benchmark.trial-statistics/v1 from stdin.
It writes oo.benchmark.trial-statistics-report/v1 to stdout.
exit 0 gate passed
exit 1 gate failed
exit 2 config
```

With no arguments the process reads stdin until EOF, capped at 1_048_577 bytes. A longer stream is refused before parse. The bytes must be UTF-8. The process writes one compact JSON report and a final newline to stdout when the document is a paired statistic. Stderr is empty for a passed gate and for a failed gate.

| Exit | Stdout | Stderr |
| --- | --- | --- |
| 0 | report, gate `passed` | empty |
| 1 | report, gate `failed` | empty |
| 2 | empty | `trial-statistics: <token>` or `trial-statistics: <token> <pointer>` |

Tokens are `usage`, `stdin`, `json`, `schema`, `budget`, `tuple`, `trials`, and `io`. A pointer is a JSON pointer into the input and does not contain a value from the document. A read failure is `stdin`. Invalid UTF-8, broken JSON, a duplicate key, trailing bytes, and the core depth or node budget are `json`. A stdout or stderr write failure is `io`. Those three tokens have no pointer.

A failed gate is a well-formed document in which at least one trial is `needs_reannotation`. An ill-formed document is config, exit 2, and produces no report. Gate failure is not a claim that one variant beat the other.

## Document

One JSON value. `oo_core::parse_json_value` rejects a document larger than `DOCUMENT_MAX_BYTES` (1_048_576), depth past 32, more than 50_000 nodes, a duplicate key, and trailing bytes. JSON null is rejected in every field. Unknown keys are rejected. A JSON number is accepted only for `resamples` and trial `index`.

Top-level keys, all required:

| Key | Value |
| --- | --- |
| `version` | the string `oo.benchmark.trial-statistics/v1` |
| `algorithm` | the string `oo.benchmark.paired-question-bootstrap/v1` |
| `seed` | canonical base-10 `u64`, as a JSON string |
| `resamples` | integer in `1..=10000` |
| `variants` | an array of exactly two variants |

`seed` is `0`, or digits without a leading zero, and the value fits in `u64`. `00`, `+1`, `-1`, a fraction, and an exponent are `schema` at `/seed`. The report echoes this canonical spelling.

`resamples` is a JSON integer with no fraction and no exponent. A negative number, `0`, or a value above 10000 is `budget` at `/resamples`. A fraction, an exponent, or a non-number is `schema` at `/resamples`. `1e3` is `schema` even though the magnitude is in range. There is no float.

## Variant

| Key | Value |
| --- | --- |
| `id` | 1 to 64 bytes |
| `tuple` | the frozen tuple |
| `questions` | 1 to 256 questions |

An id matches `^[A-Za-z][A-Za-z0-9._:-]*$`. A length outside 1..=64 is `budget`. Any other spelling is `schema`. The two ids differ. The same id on both variants is `schema` at `/variants/1/id`.

The two question arrays have the same length and the same ids in the same order. A different length or a different id is `schema`. Order is the pairing. The program does not sort ids and does not match them by name.

## Frozen tuple

Both variants carry the same ten strings. Exact UTF-8 equality. The strings are not trimmed, case-folded, or parsed as dates or numbers.

| Key | Meaning on the record |
| --- | --- |
| `project` | project id |
| `snapshot` | snapshot id |
| `as_of` | as-of label |
| `dataset` | dataset id |
| `suite` | suite id |
| `model` | model id |
| `harness` | harness id |
| `role` | role id |
| `budget` | budget label |
| `scorer_version` | scorer version |

Each value is 1 to 128 bytes, contains no NUL, and contains no ASCII control and no `0x7F`. A length outside 1..=128 is `budget`. A control byte is `schema`.

The first field that differs, in the order listed above, is `tuple` at `/variants/1/tuple/<field>`. The program then writes no report. Unequal tuples are not compared.

## Question and trials

A question has `id` and `trials` and no other key. The id uses the variant id pattern with a maximum of 128 bytes. Ids are unique inside a variant. A duplicate id is `schema`.

`k` is the length of the first question's `trials`. Every question on both variants has that same `k`. `k` is an integer in `1..=8`. A length outside that range is `budget`. A length inside that range that differs from the first question is `trials`.

A trial has `index` and `status` and no other key. On a question, `index` at position `i` is the integer `i`, starting at 0. A missing index, a repeated index, an out-of-order index, or any other integer is `trials`. A fraction or exponent is `schema`.

`status` is one of three strings:

| Status | pass^k bit | Denominator | Gate |
| --- | --- | --- | --- |
| `pass` | contributes | stays | unchanged |
| `fail` | does not contribute | stays | unchanged |
| `needs_reannotation` | does not contribute | stays | failed |

`fail` is the retained score for a trial the caller already classified as a miss, including a missing run, a cancellation, or an error. The caller writes that word. Omitting the trial is `trials`, not a silent failure. The words `missing`, `cancelled`, `canceled`, and `error` are `schema`. They are not extra statuses.

A question's pass^k bit is 1 only when every trial is `pass`. One `fail` or one `needs_reannotation` makes the bit 0. The question stays in the denominator either way.

`pass^k` count for a variant is the number of questions whose bit is 1. It is not pass@k. pass@k would average the trial bits. This protocol does not compute that average, a majority rate, or a probability estimator.

`reannotation_count` is the number of questions in which either variant has at least one `needs_reannotation` trial. A question that needs it on both variants counts once. Those questions remain inside `question_count`. The gate is `failed` when `reannotation_count` is not zero, and `passed` otherwise. The gate does not drop them and does not change the counts.

## Payload budget

Payload bytes are the sum of the seed length, both variant id lengths, every tuple value length, every question id length, and 1 byte for every trial. The cap is 65536. Crossing it is `budget` at `/`. A single overlong id or tuple value is `budget` at that field, before the sum is checked.

Question count outside 1..=256 is `budget` at that variant's `/questions`. Trial count outside 1..=8 is `budget` at that question's `/trials`.

## Bootstrap

The algorithm id is `oo.benchmark.paired-question-bootstrap/v1`.

The PRNG is SplitMix64, the mix from Steele, Lea, and Flood used by Java `SplittableRandom`. State is a `u64`. The seed is the state before the first step. One step:

```text
state = state + 0x9E3779B97F4A7C15   (mod 2^64)
z = state
z = (z xor (z >> 30)) * 0xBF58476D1CE4E5B9   (mod 2^64)
z = (z xor (z >> 27)) * 0x94D049BB133111EB   (mod 2^64)
output = z xor (z >> 31)
```

The known vector for seed 0, first four outputs:

```text
0xe220a8397b1dcdaf
0x6e789e6aa1b965f4
0x06c45d188009454f
0xf88bb8a8724c81ec
```

An index in `0..n`, where `n` is the question count, draws outputs until one is accepted:

```text
zone = floor((2^64 - 1) / n) * n
accept output when output < zone
index = output mod n
```

Comparison and multiplication are unsigned. Rejected outputs are not indexes. For `n = 2`, `2^64 - 1` and `2^64 - 2` are rejected. For `n = 1` the only index is 0.

One resample draws `n` indexes in order, with replacement. The same indexes are applied to both variants. A question drawn twice contributes twice. The draw consumes the whole question: all `k` trials stay tied to that question. Trials inside a question are not resampled.

For resample `b`:

```text
count(v) = sum of the pass^k bits of the drawn questions for variant v
difference = (count(second) - count(first)) / n
```

`second` is `variants[1]`. `first` is `variants[0]`. The point difference uses each original question once. It is not a bootstrap draw.

```text
point = (pass_k_count(second) - pass_k_count(first)) / question_count
```

A rational is a numerator and a positive denominator, reduced by `gcd(|numerator|, denominator)`. The sign stays on the numerator. Zero is `0/1`, not `0/n`.

The `resamples` rationals are sorted non-decreasing. `a/b` precedes `c/d` when `a*d < c*b`, compared in exact integer arithmetic. Ties keep their original order.

Ranks are 0-based and use integer division. No floating point and no interpolation between neighbors.

```text
low_rank = floor(resamples * 25 / 1000)
high_rank = floor(resamples * 975 / 1000)
```

`low_rank` is the exact floor rank at 2.5%. `high_rank` is the exact floor rank at 97.5%. The reported percentile is the rational stored at that rank after the sort. For `resamples = 1000` the ranks are 25 and 975. For `resamples = 1` both ranks are 0. For `resamples = 4` the ranks are 0 and 3. For `resamples = 80` the ranks are 2 and 78.

## Worked seed 0, three questions, four resamples

Question bits for the passed-gate fixture, in order `q0`, `q1`, `q2`:

| Variant | Bits | Why |
| --- | --- | --- |
| `left` | 0, 1, 0 | `q0` has a `fail`, `q1` is all `pass`, `q2` has a `fail` |
| `right` | 1, 0, 1 | `q1` has a `fail`, `q0` and `q2` are all `pass` |

`question_count` is 3. Counts are 1 and 2. The point difference is `1/3`.

Seed 0, `n = 3`, first twelve accepted indexes, grouped by resample:

| Resample | Indexes | Left sum | Right sum | Difference |
| --- | --- | --- | --- | --- |
| 0 | 1, 0, 1 | 2 | 1 | `-1/3` |
| 1 | 1, 1, 0 | 2 | 1 | `-1/3` |
| 2 | 2, 2, 2 | 0 | 3 | `1/1` |
| 3 | 2, 1, 1 | 2 | 1 | `-1/3` |

Sorted: `-1/3`, `-1/3`, `-1/3`, `1/1`. Rank 0 is `-1/3`. Rank 3 is `1/1`.

The reannotation fixture changes `left` `q1` trial 0 from `pass` to `needs_reannotation`. The left bits become 0, 0, 0. `question_count` stays 3. The left pass^k count becomes 0. The point difference becomes `2/3`. `reannotation_count` is 1. The gate is `failed`. The same indexes then give differences `1/3`, `1/3`, `1/1`, `1/3`. Sorted rank 0 is `1/3`. Rank 3 is `1/1`.

The not-pass-at fixture has one question and `k = 3`. `left` is three passes. `right` is pass, pass, fail. pass^k is `1/1` and `0/1`. The point difference is `-1/1`. The pass@1 trial average would be `-1/3`. This protocol reports `-1/1`. With `resamples = 1` and `n = 1` both percentiles are that same difference.

## Report

Compact JSON, one trailing newline, keys in alphabetical order because the writer declares struct fields in that order. Nested rationals and variant objects do the same. Variant order is the input order.

```json
{"algorithm":"oo.benchmark.paired-question-bootstrap/v1","gate":"passed","high_rank":3,"low_rank":0,"percentile_high":{"denominator":1,"numerator":1},"percentile_low":{"denominator":3,"numerator":-1},"point_difference":{"denominator":3,"numerator":1},"question_count":3,"reannotation_count":0,"resamples":4,"seed":"0","trial_count":3,"variants":[{"id":"left","pass_k":{"denominator":3,"numerator":1},"pass_k_count":1},{"id":"right","pass_k":{"denominator":3,"numerator":2},"pass_k_count":2}],"version":"oo.benchmark.trial-statistics-report/v1"}
```

`pass_k` is the reduced form of `pass_k_count / question_count`. `question_count` is the study denominator, including every reannotation question. `pass_k_count` keeps the unreduced numerator.

The same input bytes and the same seed produce the same report bytes.

## What this protocol does not do

It does not read model prose. It does not call a model or a judge. It does not estimate pass@k. It does not compute a mean, a majority rate, a McNemar test, or a p-value. It does not smooth percentile ranks. It does not drop a `needs_reannotation` question. It does not compare unequal project, snapshot, as-of, dataset, suite, model, harness, role, budget, or scorer version tuples. It does not report cost, latency, a holdout, a customer gate, or a 300-question bank. It does not claim that a benchmark suite or a release gate has passed.
