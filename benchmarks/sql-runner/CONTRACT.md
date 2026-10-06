# SQL runner v1

Protocol id: `oo.benchmark.sql-runner/v1`.

Report id: `oo.benchmark.sql-runner-report/v1`.

This document is the frozen input and output of one trusted SQL trial runner. A suite lists explicit cases. Each trial starts a new `query-pg` process, then the query-result adapter, then the result comparator. The report is one row per planned trial.

The runner does not call a model, an agent, or a judge. It does not score prose. It does not issue a session certificate. It does not hold customer data and it does not accept a 300-question bank. `frozen_eval` is a suite mode word for a batch the operator already froze. It is not a check that a catalog of frozen questions exists.

`metadata` is the word `declared`. `tuple.snapshot` and `tuple.as_of` are declared labels copied into the report. This program does not inspect a database snapshot and does not certify one.

The adapter, the comparator, and trial-statistics are external programs consumed through this contract. They are not re-approved here. In particular, an adapter completion of `complete` together with `truncated: true` is not a full answer. A comparison that matches that truncated prefix is still a failed trial.

The crate is a standalone workspace. It links `oo-core` from the fixed source archive and uses serde 1.0.229 and serde_json 1.0.151. It does not edit the root manifest, the root lock, or those other crates.

## Command

```text
sql-runner --suite <path> --run-dir <path>
sql-runner --help
sql-runner -h
```

The suite form is those four arguments in that order. `--help` and `-h` are accepted only as the single argument. They write the help text below to stdout, write nothing to stderr, do not read the suite, and exit 0.

Any other argument list writes `sql-runner: usage` to stderr, writes nothing to stdout, and exits 2.

Help text, including the final newline:

```text
sql-runner --suite <path> --run-dir <path>
sql-runner --help

sql-runner reads one oo.benchmark.sql-runner/v1 suite and writes one trial report.
Each trial is a new query-pg process, then the result adapter, then the result comparator.
exit 0 gate passed
exit 1 gate failed
exit 2 config
```

| Exit | Stdout | Stderr | Run directory |
| --- | --- | --- | --- |
| 0 | one report | empty | `report.json` has the same bytes |
| 1 | one report | empty | `report.json` has the same bytes |
| 2 | empty | `sql-runner: <token>` or `sql-runner: <token> <pointer>` | not created when config fails before the run |

Exit 0 is a valid run whose gate passed. Exit 1 is a valid run whose gate failed. The report is kept either way. Exit 2 is config, or a failure to write the report. A config rejection before `create_dir` leaves the run path absent.

Tokens are `usage`, `path`, `run_dir`, `json`, `schema`, `budget`, `mode`, `tuple`, `role`, `hash`, `password`, `gold`, and `io`. A pointer is a JSON pointer into the suite, or `/suite`, or `/run_dir`. It does not contain a path value, a SQL text, a cell, a gold value, or a password. `usage` and a suite-level `json` or `io` have no pointer.

## Suite

The suite path is a safe absolute path: it begins with `/`, it is not `/`, it has no empty, `.`, or `..` component, no component equal to `unknown`, and no byte outside ASCII letters, digits, and `/._:-`. The file is a regular non-symlink of at most 1_048_576 bytes. `oo_core::parse_json_value` rejects duplicate keys, depth past 32, more than 50_000 nodes, and trailing bytes. JSON null is rejected in the suite. Unknown keys are rejected. A JSON number is accepted only for `k`, `trial_wall_ms`, and the numeric request fields this document checks.

Top-level keys, all required:

| Key | Value |
| --- | --- |
| `version` | `oo.benchmark.sql-runner/v1` |
| `mode` | `synthetic` or `frozen_eval` |
| `metadata` | the string `declared` |
| `tuple` | the ten fields below |
| `executables` | `query_pg`, `adapter`, and `comparator` |
| `cases` | 1 to 32 cases |

`production` and `unknown` are refused modes (`mode`). Any other mode word is refused the same way. `metadata` other than `declared` is `schema`.

### Tuple

One object for the whole suite. Keys are the trial-statistics ten, all required, and no others: `project`, `snapshot`, `as_of`, `dataset`, `suite`, `model`, `harness`, `role`, `budget`, `scorer_version`.

| Field | Rule |
| --- | --- |
| `model` | the string `none` |
| `harness` | the string `sql-runner` |
| `scorer_version` | `oo.benchmark.sql-runner/v1` |
| `role` | 1 to 63 bytes, `^[A-Za-z][A-Za-z0-9._:-]{0,62}$` |
| every field | not the string `unknown` or `production` |
| the other fields | 1 to 128 bytes, no ASCII control and no DEL |

A bad fixed field, a banned word, or a bad role is `tuple`. A label that is too long is `budget`. A label with a control byte is `schema`. Cases do not carry their own snapshot or `as_of`. A second tuple cannot appear.

### Executables

Each of `query_pg`, `adapter`, and `comparator` is an object with `path` and `sha256` only. `path` is a safe absolute path to a regular non-symlink of at most 268_435_456 bytes. `sha256` is 64 lowercase hex digits. Before any trial, the runner reads the file and compares `oo_core::canonical::hash_bytes` with that hex. A mismatch is `hash`. A symlink, a directory, a relative path, `/`, or a component `unknown` is `path`. The check happens once, before the run directory is created.

The runner spawns those paths with `Command`. It does not use a shell. The child environment is cleared. The only variable set is `PATH=/usr/bin:/bin`. The parent environment is not copied. `PGPASSWORD` is not set. The password is not an argument.

### Cases

Each case is an object with `id`, `role`, `k`, `annotation`, `trial_wall_ms`, `request_file`, `binding_file`, and `gold_file`, and no other key.

| Field | Rule |
| --- | --- |
| `id` | 1 to 64 bytes, `^[A-Za-z][A-Za-z0-9._:-]{0,63}$`, unique in the suite |
| `role` | the same string as `tuple.role` |
| `k` | JSON integer 1 through 8 |
| `annotation` | `score` or `needs_reannotation` |
| `trial_wall_ms` | JSON integer 1 through 120000 |
| `request_file`, `binding_file`, `gold_file` | safe absolute paths, distinct from each other |

A duplicate id is `schema`. A role that is not the tuple role is `role`. `k` or `trial_wall_ms` outside the range, or more than 32 cases, is `budget`. A non-integer is `schema`. `k` may differ by case. The planned trial count is the sum of `k`, and every planned trial stays in the report.

Integers are lexical: digits only, no sign, no leading zero, no fraction, no exponent. `0` is an integer and then fails the range.

## Request, binding, and gold

The request file is a regular non-symlink of at most 1_048_576 bytes and must parse as one JSON value. `version` must be `oo.gateway.query-pg/v1`.

The runner checks these fields and carries the other request keys through:

| Check | Rule |
| --- | --- |
| `password_file`, `ca_file` | safe absolute paths to regular non-symlinks |
| `audit_file` | present as a safe absolute path string; the value is replaced per trial |
| `statement.source` | `request` |
| `statement.sql` | a string of 1 to 65536 bytes, no NUL |
| `statement.max_rows` | JSON integer 0 through 2000 |
| `statement.max_result_bytes` | JSON integer 0 through 262144 |
| `statement.wall_clock_ms` | JSON integer 1 through 30000 |
| `identity.role` | `oo_r_` concatenated with the case role |
| `policies` | 1 to 32 objects, each `role` equal to the case role |

`statement.source` of `stdin` is refused. The runner does not read statement SQL from the process stdin.

`password_file` is at most 1026 bytes. One trailing LF, or one trailing CRLF, is removed. The remaining password is 1 to 1024 UTF-8 bytes and contains no NUL. If those bytes occur in the request file or in a rewritten request, the suite is refused with `password`. The password is not an argument, not an environment value, and not written to stdout, stderr, or the report.

`ca_file` is at most 1_048_576 bytes, non-empty, and must not contain the byte string `PRIVATE KEY`. An empty CA or a private-key marker is `schema`. An oversize password or CA file is `budget`. A missing file or a symlink is `path`. A password that is empty, not UTF-8, or contains NUL is `password`. Those pointers are the case `request_file`. The runner does not parse the certificate and does not write it to the report.

The binding file is one JSON object, the adapter header. Required keys are `record` (`header`), `version` (`oo.benchmark.query-result-adapter/v1`), `turn` (integer 0), `max_rows`, `max_result_bytes`, and `columns` (an array). Optional key `empty_result` is the string `rows` or `not_found_if_complete_empty`. JSON null, any other type, and any other string are `schema` at the binding file's `empty_result` pointer. Unknown keys are `schema`. `max_rows` and `max_result_bytes` must equal the request. The runner rewrites the header as one compact JSON line and keeps `empty_result` when the binding has it, with the same string. An omitted key stays omitted. The runner does not read the gold `expected`, the comparator, or an actual result to set or replace that field. This program runs one statement and only turn 0.

The gold file is one JSON object with `row_comparator` and `expected` only. `row_comparator` is 1 to 32 bytes of ASCII lowercase letters or `_`, and is not `unknown` or `production`. `ordered` and `unordered` are the comparator's names. `set` is accepted here so the comparator can refuse it. `expected` is a JSON object. An `actual` key is `schema`. The runner does not compare gold itself.

The absolute gold path must not occur in the request file bytes, the reserialized request, the rewritten request, or the binding bytes. The request path must not be the gold path. Gold is written only as the comparator's `expected` value.

These checks all finish before the run directory is created. A failure is exit 2 and writes no report.

## Run directory

The run path uses the same safe absolute path rule. Its parent is an existing directory and not a symlink. The final component must not exist, including as a symlink. After the suite checks succeed, the runner creates that one directory with `create_dir` (not `create_dir_all`). If the directory exists, the token is `run_dir` and nothing inside it is changed.

Trial paths, created only for a trial that starts:

```text
<run-dir>/case-<id>/trial-<index>/request.json
<run-dir>/case-<id>/trial-<index>/query.stdin
<run-dir>/case-<id>/trial-<index>/query.stdout
<run-dir>/case-<id>/trial-<index>/query.stderr
<run-dir>/case-<id>/trial-<index>/adapter.stdin
<run-dir>/case-<id>/trial-<index>/adapter.stdout
<run-dir>/case-<id>/trial-<index>/adapter.stderr
<run-dir>/case-<id>/trial-<index>/comparator.stdin
<run-dir>/case-<id>/trial-<index>/comparator.stdout
<run-dir>/case-<id>/trial-<index>/comparator.stderr
<run-dir>/case-<id>/trial-<index>/audit.jsonl
<run-dir>/report.json
```

`<index>` is `0` through `k-1` with no leading zero. `request.json` is the request object rewritten with `audit_file` set to that trial's `audit.jsonl`. The runner creates `audit.jsonl` only by leaving it absent for `query-pg`. Adapter files are created only when the adapter is started. Comparator files are created only when the comparator is started.

Cases and trials run sequentially. Each process is a new spawn. Stdin, stdout, and stderr are regular files created with `create_new`. The runner does not use pipes.

`query.stdin` is an empty file. The adapter stdin is the header line plus the query stdout bytes. The comparator stdin is one comparison document: `version` `oo.benchmark.result-comparison/v1`, the gold `row_comparator`, the gold `expected`, and `actual` parsed from the adapter stdout. The runner does not put gold in the query request.

## Supervision

`trial_wall_ms` is one deadline for all three processes in that trial, measured from an `Instant` taken before `query-pg` is spawned. The runner waits with `Child::try_wait` and sleeps at most 20 milliseconds between waits. A child that is still running at the deadline is killed and then waited. A child that exits during the sleep is reaped normally. On drop, a still-held child is killed and waited.

Each stdio file is capped at 1_048_576 bytes. A larger file kills the child, waits, and fails that trial as `budget`. Query stdout over the cap is not passed to the adapter.

`backend_cleanup` is the string `unknown` when `query-pg` was spawned, including after a kill and including a guard refusal. It is JSON null when the query process was not spawned. `unknown` means this runner did not observe a backend session release. It is not a claim that the session was closed.

## Classification

The runner reads query stdout as JSONL and uses at most one summary object. From that object it reads `status`, `truncated`, `end_audit_ok`, and `stop`. It does not copy cells into the report.

The decision order after `query-pg` stops is:

1. The runner deadline fired: `timeout`. Do not start a later process. Processes already running are killed and waited.
2. A stdio file crossed the byte cap: `budget`. Do not compare.
3. Query exit 3, 4, 5, or 6: `refused`. Do not compare.
4. Query stdout is not usable JSONL when a summary is required: `protocol`.
5. Summary `status` `failed` and `stop` `timeout`: `timeout`. Do not compare.
6. Summary `status` `partial`: `partial`. Do not compare.
7. No summary, and the exit is not 0: `no_end`. Do not compare.
8. No summary and exit 0: `protocol`.
9. `end_audit_ok` is false: `no_end`. Do not compare.
10. Summary `status` `failed`: `query`. Do not compare.
11. Exit 0, `status` `succeeded`, `end_audit_ok` true, `truncated` false: run the adapter and the comparator.
12. Exit 0, `status` `succeeded`, `end_audit_ok` true, `truncated` true: run the adapter and the comparator, then fail the trial as `truncated` even when the verdict is `match`.
13. Any other combination: `protocol`.

A signal exit that this runner did not cause is `protocol`. Exit 7 with `end_audit_ok` false is `no_end`. Exit 8 with no summary is `no_end`. Exit 8 with `stop` `timeout` is `timeout`.

The adapter is started only for steps 11 and 12, and only if the deadline has not passed and the adapter stdin would be at most 1_048_576 bytes. Adapter exit other than 0 is `adapter_refused`, and the comparator is not started. Adapter exit 0 whose stdout is not one JSON value is `protocol`.

The comparator is started only after adapter exit 0. Comparator exit 2, or any exit other than 0 or 1, is `comparator_refused`. Comparator exit 1 with verdict `mismatch` is `mismatch`. Comparator exit 0 with verdict `match` is `match` on step 11 and `truncated` on step 12. A verdict that disagrees with the exit is `protocol`. The runner reads only `verdict`. It does not copy `differences`, cells, or gold text into the report.

`annotation` `needs_reannotation` still runs the trial. After classification, `status` and `reason` become `needs_reannotation`. Exit fields and `comparator_verdict` stay as the run left them. A match does not become `pass`.

A filesystem or spawn failure stops the remaining plan. The current trial is `not_run` if `query-pg` was not spawned. Later trials are `not_run`. Finished trials are kept. The report is still written and the gate fails.

## Report

Compact JSON, one trailing newline, object keys alphabetical. The same bytes go to stdout and to `report.json`. Stderr is empty for exit 0 and exit 1.

```json
{"executable_sha256":{"adapter":"<64>","comparator":"<64>","query_pg":"<64>"},"fail_count":0,"lane":"trusted_sql","metadata":"declared","mode":"synthetic","not_run_count":0,"pass_count":1,"passed":true,"question_count":1,"reannotation_count":0,"timeout_count":0,"trial_count":1,"trials":[],"tuple":{},"version":"oo.benchmark.sql-runner-report/v1"}
```

`lane` is `trusted_sql`. `metadata` is `declared`. `mode` and `tuple` are copied from the suite. `executable_sha256` is the three configured digests that were checked. Paths are not in the report.

`question_count` is the number of cases. `trial_count` is the number of trial objects, which is the sum of `k`. `pass_count`, `fail_count`, `timeout_count`, `reannotation_count`, and `not_run_count` are counts of `status` and add up to `trial_count`. `passed` is true only when `trial_count` is greater than 0 and every status is `pass`.

Each trial object has these keys only:

| Key | Value |
| --- | --- |
| `case_id` | the case id |
| `index` | `0` through `k-1` |
| `status` | `pass`, `fail`, `timeout`, `needs_reannotation`, or `not_run` |
| `reason` | the reason below |
| `query_exit` | an integer, or null if the process did not exit with a code |
| `adapter_exit` | an integer, or null if the adapter was not spawned or did not exit with a code |
| `comparator_exit` | an integer, or null on the same rule |
| `truncated` | a boolean from the summary, or null |
| `end_audit_ok` | a boolean from the summary, or null |
| `partial` | true when the summary status is `partial`, false when a summary was read, null otherwise |
| `backend_cleanup` | `unknown` or null, as above |
| `comparator_verdict` | `match`, `mismatch`, or `absent` |

| `status` | `reason` |
| --- | --- |
| `pass` | `match` |
| `fail` | `mismatch`, `truncated`, `refused`, `no_end`, `partial`, `query`, `adapter_refused`, `comparator_refused`, `budget`, or `protocol` |
| `timeout` | `timeout` |
| `needs_reannotation` | `needs_reannotation` |
| `not_run` | `not_run` |

The report does not contain SQL, gold, cell values, the password, the CA, or the password path.

## Hash

The runner hashes executables with `oo_core::canonical::hash_bytes`. The crate exposes `sha256_hex` for that function. Tests check it against the FIPS 180-4 vectors for the empty string and `abc`. Those digests are published vectors. They are not a benchmark result.

## What this protocol does not do

It does not open a database. It does not install grants, rotate a password, or release a backend session. It does not treat adapter `completion` `complete` as proof that `truncated` was false. It does not treat a matched truncated prefix as a pass. It does not drop a failed, refused, timed out, or reannotation trial from the denominator. It does not re-approve the adapter, the comparator, or trial-statistics. It does not claim a customer holdout, a 300-question bank, or a release gate.
