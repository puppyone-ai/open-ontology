# Trial report adapter v1

Protocol id: `oo.benchmark.trial-report-adapter/v1`.

Output id: `oo.benchmark.trial-statistics/v1`.

This document is the frozen pairing of two trusted SQL runner reports into one statistics input. The output is the document `trial-statistics` reads. This program does not compute pass^k, a bootstrap, a percentile, or a gate.

The comparison class is `trusted_sql`. A SQL runner report carries `tuple.model` `none` and `lane` `trusted_sql`. `none` means no model was scored. This adapter refuses any other model. It writes `model` as `trusted_sql`, copied from the lane, so the statistics input is an explicit trusted SQL comparison and not a model benchmark.

`snapshot` and `as_of` are declared labels copied from the reports. This program does not open a database and does not certify a snapshot. `backend_cleanup` `unknown` means the runner did not observe a session release. It is not copied, and it is not a cleanup certificate.

The crate is a standalone workspace. It links `oo-core` from the fixed source archive and uses serde 1.0.229 and serde_json 1.0.151. It does not edit the root manifest, the root lock, or the runner.

## Command

```text
trial-report-adapter
trial-report-adapter --help
```

No other arguments. `--help` and `-h` write the help text below to stdout, write nothing to stderr, do not read stdin, and exit 0. Any other argument list writes `trial-report-adapter: usage` to stderr, writes nothing to stdout, does not read stdin, and exits 2.

Help text, including the final newline:

```text
trial-report-adapter reads oo.benchmark.trial-report-adapter/v1 from stdin.
It writes oo.benchmark.trial-statistics/v1 to stdout.
The process does not compute pass^k or a bootstrap.
exit 0 statistics input written
exit 2 config
```

With no arguments the process reads stdin until EOF, capped at 1_048_577 bytes. A longer stream is refused before parse. The bytes must be UTF-8. A valid pair writes one compact JSON document and a final newline to stdout. Stderr is empty on exit 0. There is no exit 1. A refusal writes no document.

| Exit | Stdout | Stderr |
| --- | --- | --- |
| 0 | one `oo.benchmark.trial-statistics/v1` document | empty |
| 2 | empty | `trial-report-adapter: <token>` or `trial-report-adapter: <token> <pointer>` |

Tokens are `usage`, `stdin`, `json`, `schema`, `budget`, `tuple`, `trials`, `lane`, and `io`. A pointer is a JSON pointer into the input and does not contain a value from the document. A read failure is `stdin`. Invalid UTF-8, broken JSON, a duplicate key, trailing bytes, and the core depth or node budget are `json`. A stdout or stderr write failure is `io`. Those three tokens, and `usage`, have no pointer.

## Document

One JSON value. `oo_core::parse_json_value` rejects a document larger than 1_048_576 bytes, depth past 32, more than 50_000 nodes, a duplicate key, and trailing bytes. JSON null is rejected except on the trial fields named below. Unknown keys are rejected. A JSON number is accepted only for `resamples`, the report counters, trial `index`, and the three exit fields.

Top-level keys, all required:

| Key | Value |
| --- | --- |
| `version` | `oo.benchmark.trial-report-adapter/v1` |
| `algorithm` | `oo.benchmark.paired-question-bootstrap/v1` |
| `seed` | canonical base-10 `u64`, as a JSON string |
| `resamples` | integer in `1..=10000` |
| `labels` | the explicit labels below |
| `reports` | an array of exactly two SQL runner reports |

`seed` and `resamples` use the trial-statistics spelling rules. They are copied into the output. This program does not choose a default seed or a default resample count. `00`, a fraction, and an exponent are `schema`. `0` and a value above 10000 are `budget` at `/resamples`.

## Labels

`labels` has `budget`, `scorer_version`, and `variants`, and no other key.

`variants` is an array of exactly two objects. Each object has `id` and no other key. An id matches `^[A-Za-z][A-Za-z0-9._:-]*$` and is 1 to 64 bytes. The two ids differ. These ids are the variant identity. They are not read from the SQL role.

`budget` and `scorer_version` are the frozen study labels. Each is 1 to 128 bytes, with no ASCII control and no `0x7F`. They must equal both reports' tuple fields of the same name. A disagreement is `tuple` at `/labels/budget` or `/labels/scorer_version`. The adapter does not rewrite a report budget or scorer to force a pair.

The report `scorer_version` must be `oo.benchmark.sql-runner/v1`. Any other scorer is `tuple` at that report's `/tuple/scorer_version`. This adapter does not attach a model scorer.

## Reports

Each report is one `oo.benchmark.sql-runner-report/v1` object. The keys are the runner's keys and no others. `lane` must be `trusted_sql` or the token is `lane`. `metadata` must be `declared`. `mode` must be `synthetic` or `frozen_eval`. The two modes are not required to be equal. Mode is not part of the version tuple and is not copied.

`executable_sha256` has `adapter`, `comparator`, and `query_pg`, each 64 lowercase hex digits. The digests are checked for shape and are not copied. They are not required to be equal across the two reports. This program does not re-hash a binary.

`version` must be `oo.benchmark.sql-runner-report/v1`.

### Version tuple

Both reports carry the runner's ten strings. Exact UTF-8 equality, compared in this order: `project`, `snapshot`, `as_of`, `dataset`, `suite`, `model`, `harness`, `role`, `budget`, `scorer_version`. The first field that differs is `tuple` at `/reports/1/tuple/<field>`. The program then writes no document. It does not fill, drop, or impute a question to hide the difference.

`model` must be `none`. `harness` must be `sql-runner`. A value `unknown` or `production` on any field is `tuple`. Each value is 1 to 128 bytes and has no ASCII control and no `0x7F`. A bad length is `budget`.

The output tuple copies those fields and replaces `model` with `trusted_sql`. `snapshot` and `as_of` stay the declared labels from the reports.

### Questions and trials

Trials stay in report order. A case is one contiguous run of the same `case_id`. A repeated id after another case is `schema`. Case ids use the variant id pattern with a maximum of 64 bytes.

`k` is the number of trials on the first case. Every case in that report has the same `k`. `k` is an integer in `1..=8`. A report has 1 to 32 cases. Outside those ranges is `budget`. A different `k` inside one report is `trials`.

On a case, `index` at position `i` is the integer `i`, starting at 0. A missing index, a repeated index, or an out-of-order index is `trials`. The program does not insert a `fail` for a hole.

The two reports have the same case ids in the same order and the same `k`. A different case count, or a different id, is `schema`. A different `k` across the reports is `trials` at `/reports/1/trials`. The program does not drop the longer side and does not pad the shorter side.

### Status mapping

The runner status and reason must be one of the rows below. Any other pair is `schema`. The mapped word is the only status written. Exit codes are not copied. `truncated`, `end_audit_ok`, and `partial` must be a boolean or null. Exit fields must be an integer or null. `comparator_verdict` must match the row.

| Runner `status` | Runner `reason` | Verdict | Output status |
| --- | --- | --- | --- |
| `pass` | `match` | `match` | `pass` |
| `fail` | `mismatch` | `mismatch` | `fail` |
| `fail` | `truncated` | `match` or `mismatch` | `fail` |
| `fail` | `refused`, `no_end`, `partial`, `query`, `adapter_refused`, `comparator_refused`, `budget`, or `protocol` | `absent` | `fail` |
| `timeout` | `timeout` | `absent` | `fail` |
| `not_run` | `not_run` | `absent` | `fail` |
| `needs_reannotation` | `needs_reannotation` | `match`, `mismatch`, or `absent` | `needs_reannotation` |

`needs_reannotation` stays that word. It is not rewritten to `fail` and the question is not removed. The statistics program keeps it in the denominator. `not_run` has `backend_cleanup` null. Every other status has `backend_cleanup` `unknown`.

The report counters must match the trials. `question_count` is the number of cases. `trial_count` is the number of trial objects. `pass_count`, `fail_count`, `timeout_count`, `not_run_count`, and `reannotation_count` count those statuses. `passed` is true only when every status is `pass`. A counter that disagrees is `schema`. The adapter does not repair a counter.

## Output

Compact JSON, one trailing newline, object keys alphabetical. Variant order is the label order, which is the report order. Question order is the report order. Trial indexes are `0..k-1`.

```json
{"algorithm":"oo.benchmark.paired-question-bootstrap/v1","resamples":4,"seed":"0","variants":[],"version":"oo.benchmark.trial-statistics/v1"}
```

Each variant has `id`, `questions`, and `tuple`. Each question has `id` and `trials`. Each trial has `index` and `status` only. The tuple `model` is `trusted_sql`.

The same input bytes produce the same output bytes.

## What this protocol does not do

It does not compute pass^k or a bootstrap. It does not call a model, a judge, or a database. It does not certify a snapshot or a backend session release. It does not treat `tuple.model` `none` as a model name. It does not compare unequal version tuples. It does not drop, fill, or impute a question or a trial. It does not claim a private TLS session, a customer holdout, a 300-question bank, or a release gate.
