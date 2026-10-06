# Question-bank suite compiler v1

Protocol id: `oo.benchmark.question-bank-suite/v1`.

Report id: `oo.benchmark.question-bank-suite-report/v1`.

Manifest id: `oo.benchmark.question-bank-suite-manifest/v1`.

Agent-view id: `oo.benchmark.question-bank-suite-agent-view/v1`.

This document is the frozen input and output of one compiler. It turns one admitted public question bank, plus an operator tuple and request templates, into files an existing `oo.benchmark.sql-runner/v1` runner can read. The bank gate is `oo_question_bank::evaluate` of the fixed question-bank crate. This program does not reimplement admission, does not score a model, and does not spawn `query-pg`, the adapter, or the comparator.

The bank's `needs_reannotation` label stays `needs_reannotation`. Exit 0 means the suite directory was created. It is not a claim that every bank label is `fully_approved`, and it is not a claim that a later trial matched.

This is not a 300-question bank, a human-annotator roster, a customer holdout, or a quality score. `tuple.model` is `none`. `tuple.harness` is `sql-runner`. The manifest says `quality_claim` `none` and `authentication` `not_claimed`. A request template is an operator document. It is not an authentication claim.

The crate is a standalone workspace. It links `oo-core` and `oo-question-bank` from the fixed source archive and uses serde 1.0.229 and serde_json 1.0.151. It does not edit the root manifest or the root lock.

## Command

```text
question-bank-suite --input <path> --out-dir <path>
question-bank-suite --help
question-bank-suite -h
```

The compile form is those four arguments in that order. `--help` and `-h` are accepted only as the single argument. They write the help text below to stdout, write nothing to stderr, do not read the input, and exit 0.

Any other argument list writes `question-bank-suite: usage` to stderr, writes nothing to stdout, does not read the input, and exits 2.

Help text, including the final newline:

```text
question-bank-suite --input <path> --out-dir <path>
question-bank-suite --help

question-bank-suite reads one oo.benchmark.question-bank-suite/v1 document.
It writes one sql-runner suite and scorer-owned files.
Gold stays out of the agent view and out of this report.
exit 0 suite written
exit 2 config
```

| Exit | Stdout | Stderr | Suite directory |
| --- | --- | --- | --- |
| 0 | one report and a final newline | empty | created |
| 2 | empty | `question-bank-suite: <token>` or `question-bank-suite: <token> <pointer>` | not created when refusal happens before `create_dir` |

Tokens are `usage`, `path`, `out_dir`, `json`, `schema`, `budget`, `bank`, `hash`, `identity`, `group`, `unsupported`, `gold`, `tuple`, `role`, `mode`, `template`, `password`, and `io`. A pointer is a JSON pointer into the compile document or into `/bank` plus the question-bank pointer. It does not contain SQL, a gold cell, a password, or a path value. `usage` and a bare `json`, `bank`, or `io` have no pointer.

A bank error from `oo_question_bank::evaluate`, including `decimal`, is reported here as `bank`. The pointer is `/bank` joined with the bank pointer. This compiler does not invent a second decimal spelling check.

## Document

The input path is a safe absolute path: it begins with `/`, it is not `/`, it has no empty, `.`, or `..` component, no component equal to `unknown`, and no byte outside ASCII letters, digits, and `/._:-`. The file is a regular non-symlink of at most 1_048_576 bytes. The bytes must be UTF-8. `oo_core::parse_json_value` rejects duplicate keys, depth past 32, more than 50_000 nodes, and trailing bytes. JSON null is rejected. Unknown keys are rejected.

Object key order in the input is not significant. The bank value is re-serialized as compact JSON with sorted keys and passed to `oo_question_bank::evaluate`. The operator does not pass a separate bank file. `manifest_sha256` is the question-bank content hash of the manifest view from that evaluation, 64 lowercase hex digits. It is not a hash of the compile document.

Top-level keys, all required:

| Key | Value |
| --- | --- |
| `version` | `oo.benchmark.question-bank-suite/v1` |
| `bank` | one `oo.benchmark.question-bank/v1` object |
| `manifest_sha256` | 64 lowercase hex digits |
| `selection` | `role` and `as_of` |
| `k` | JSON integer 1 through 8 |
| `trial_wall_ms` | JSON integer 1 through 120000 |
| `mode` | `synthetic` or `frozen_eval` |
| `tuple` | the ten sql-runner fields |
| `executables` | `query_pg`, `adapter`, and `comparator` |
| `templates` | one request template per named role |

`production` and `unknown` are refused modes (`mode`). A `k` or `trial_wall_ms` outside the range is `budget`. A non-integer is `schema`.

### Selection

Keys, all required: `role`, `as_of`. No others.

`role` is 1 to 63 bytes, `^[A-Za-z][A-Za-z0-9._:-]{0,62}$`. A longer role is `budget` at `/selection/role`. A bad spelling is `schema`. `as_of` is 1 to 128 bytes with no ASCII control and no DEL. The compiler compares it as text with each selected question's bank `as_of`. It does not read a clock.

### Tuple

Keys, all required, no others: `project`, `snapshot`, `as_of`, `dataset`, `suite`, `model`, `harness`, `role`, `budget`, `scorer_version`.

| Field | Rule |
| --- | --- |
| `model` | the string `none` |
| `harness` | the string `sql-runner` |
| `scorer_version` | `oo.benchmark.sql-runner/v1` |
| `role` | the same string as `selection.role` |
| every field | not the string `unknown` or `production` |
| the other fields | 1 to 128 bytes, no ASCII control and no DEL |

A bad fixed field or a mismatched role is `tuple`. `tuple.dataset` must equal the bank dataset id and `tuple.suite` must equal the bank suite id. A mismatch is `identity`.

### Executables

Each of `query_pg`, `adapter`, and `comparator` is an object with `path` and `sha256` only. `path` is a safe absolute path to a regular non-symlink of at most 268_435_456 bytes. `sha256` is 64 lowercase hex digits. The compiler reads the file and compares `oo_core::canonical::hash_bytes` with that hex. A mismatch is `hash`. A symlink, a directory, or a missing file is `path`. The compiler does not spawn the path and does not use a shell. Caller code in that file is not executed.

### Templates

`templates` is one object. Each key is a role spelling of 1 to 63 bytes. The key must be a role that appears on some question in the admitted bank. The selected role must have an entry. A missing or extra role is `template`. Every entry is checked, including roles that are not selected. An invalid unused template refuses the whole compile.

Each value has `request` and `binding_header` only.

The request is an `oo.gateway.query-pg/v1` object. Keys, all required, no others: `version`, `endpoint`, `password_file`, `ca_file`, `audit_file`, `identity`, `policies`, `catalog`, `statement`. `version` is `oo.gateway.query-pg/v1`. There is no `row_filter` key. This compiler does not open a connection. A row filter is not accepted because the fixed query-pg column ACL refuses one before a connection.

`statement` keys, all required: `source`, `sql`, `max_rows`, `max_result_bytes`, `wall_clock_ms`. `source` is `request`. The template `sql` is a non-empty placeholder of at most 65536 bytes and no NUL. The compiler replaces it with the question `gold_sql` only when it renders the selected role. `max_rows` is 0 through 2000. `max_result_bytes` is 0 through 262144. `wall_clock_ms` is 1 through 30000.

`identity.role` is `oo_r_` plus the template role. `identity.policy_id` is 64 lowercase hex digits. `identity.project` is 1 to 128 bytes with no ASCII control. A role mismatch is `role`.

`endpoint` keys: `host`, `port`, `database`, `user`. `port` is 1 through 65535. `host` is 1 to 253 bytes. `database` and `user` are 1 to 63 bytes. No ASCII control.

`catalog` keys: `max_tables` (1 through 32), `max_columns` (1 through 512), `max_functions` (1 through 8), `max_operators` (54 through 128), `large_table_row_threshold` (0 through the maximum u64), `unknown_rows` (`refuse`, `zero`, or `saturate`).

`policies` is 1 to 32 objects. Each policy's keys are `project`, `config_digest`, `generation`, `role`, `login`, and `table`. `role` equals the template role. `config_digest` is 64 lowercase hex digits. `generation` is an integer greater than or equal to 1. `table` keys are `namespace`, `name`, and `columns`. `columns` is 0 to 512 objects, each with `name`, `pii`, `semantic_type`, and `status` only.

`password_file`, `ca_file`, and `audit_file` are safe absolute paths. The password file and the CA file must already exist as regular non-symlinks. The password file is at most 1026 bytes. One trailing LF or CRLF is removed. The remaining password is 1 to 1024 UTF-8 bytes and contains no NUL. If those bytes occur in the compact request, the compile refuses with `password`. The CA file is 1 to 1_048_576 bytes and must not contain the byte string `PRIVATE KEY`. The audit path is checked as a path string. The compiler does not create it. A later runner rewrites `audit_file` per trial.

`binding_header` keys, all required, no others: `record`, `version`, `turn`, `max_rows`, `max_result_bytes`. `record` is `header`. `version` is `oo.benchmark.query-result-adapter/v1`. `turn` is 0. `max_rows` and `max_result_bytes` equal the statement budgets. A `columns` key on the header is `schema`. The compiler derives columns from typed gold.

## Bank identity

After `evaluate` returns, the compiler parses the question-bank report and recomputes SHA-256 over the compact JSON of `manifest`, `agent_view`, and `scorer_view`, with no trailing newline. Each digest must equal `content_hashes`. `manifest_sha256` must equal `content_hashes.manifest`. A mismatch is `hash`.

These manifest fields must equal the input bank: `bank_version`, dataset id, suite id, `distribution`, `scale`, `track`, and the question ids in order. `distribution` must be `public_synthetic`. A mismatch is `identity`. The fixed question bank already refuses any other distribution. This check keeps a substituted manifest from renaming the bank.

`evaluate` returning `gate_passed: false` is still a successful admission. The questions remain in the denominator.

## One role, one as_of

The planned group is every scorer-view question whose `asker.role` equals `selection.role`, in bank order. Questions with other roles are not planned. Their ids are listed, in bank order, as `excluded_question_ids`. That list is the explicit selection. It is not a silent drop inside the selected role.

If the selected role has no question, the compile refuses with `group` at `/selection/role`. If any selected question has a different `as_of`, the compile refuses with `group` at `/selection/as_of`. The matching questions are not kept. If the selected role has more than 32 questions, the compile refuses with `budget` at `/cases`. No question is truncated and no partial suite is written.

Planned ids are the whole selected group, including `needs_reannotation`. `fully_approved` becomes annotation `score`. `needs_reannotation` stays `needs_reannotation`. Any other review status is `schema`.

## Gold

`clarify` and `refuse_permission` are `unsupported` at the question's `expected/kind`. The whole compile refuses. The other questions in the group are not written.

`table`, `scalar`, and `top_k` require a non-empty `gold_sql` of at most 65536 bytes, a `typed_gold` object whose `kind` is `rows`, and an `ordered` boolean. `ordered: true` sets `row_comparator` to `ordered`. `ordered: false` sets it to `unordered`. If `value_columns` is present, every `tolerance` must be `exact`. A `relative` tolerance is `gold`. The compiler does not rescale a decimal. It copies `typed_gold` into the gold file.

`not_found` requires `gold_sql` and a `typed_gold` object whose `kind` is `not_found`. `row_comparator` is `ordered`. The binding `columns` array is empty.

A missing `gold_sql` or `typed_gold` is `gold` for the whole group. The compiler does not skip that question.

The fixed query-result adapter emits `kind` `rows` for an empty gateway result. It does not emit `kind` `not_found`. A later sql-runner trial can therefore record `mismatch` for a compiled `not_found` case. This compiler still emits that case, with the declared `not_found` gold. It does not rewrite the gold to an empty row set and it does not drop the id.

Binding columns for a rows gold are `name`, `ordinal`, and `type`. `ordinal` is 0 through n-1 in typed-gold column order. `type` is `bool`, `int64`, `decimal`, or `text`.

The gold file's absolute path must not occur in the request bytes or the binding bytes. A hit is `gold`.

## Output

The output path is a safe absolute path. Its parent must already exist and be a directory. The final component must not exist. The compiler calls `create_dir`, not `create_dir_all`. An existing path is `out_dir` and is left unchanged. On a later write failure the compiler removes the new directory only when the path has at least four components. Directories are mode `0700`. Files are created with `create_new` and mode `0600`.

Files, all compact JSON with one trailing newline:

| Path | Contents |
| --- | --- |
| `suite.json` | `oo.benchmark.sql-runner/v1` |
| `manifest.json` | planned-question manifest |
| `agent-view.json` | selected questions only, bank agent-view fields, this crate's agent-view version |
| `questions/<id>/request.json` | the selected template with `statement.sql` replaced by `gold_sql` |
| `questions/<id>/binding.json` | adapter header plus derived columns |
| `questions/<id>/gold.json` | `row_comparator` and `expected` set to the typed gold |

`suite.json` keys are `version`, `mode`, `metadata` (`declared`), `tuple`, `executables`, and `cases`. Each case has `id`, `role`, `k`, `annotation`, `trial_wall_ms`, `request_file`, `binding_file`, and `gold_file`. Paths are absolute. `k` and `trial_wall_ms` are the document values. Planned order is bank order. `trial_count` in the manifest is `k` times the planned question count. No trial is dropped.

The agent view is not a second gold channel. It changes `version` to `oo.benchmark.question-bank-suite-agent-view/v1` and keeps only planned ids. Gold SQL, typed gold, control totals, annotations, and review labels are not in it.

The manifest carries `files`, including each question's `request_sha256`, `binding_sha256`, and `gold_sha256`. Those hashes are SHA-256 of the written bytes, including the trailing newline. The stdout report is the manifest object without `files`, plus `manifest_sha256` of `manifest.json` including its newline, and `version` set to the report id. The report also repeats `suite_sha256` and `agent_view_sha256` so a caller can check the files without reading gold. The report has no gold cells, no SQL, and no password.

`authentication` is `not_claimed`. `operator_document` is `declared`. `model` is `none`. `quality_claim` is `none`.

A password byte string in any written file or in the report is `password`. The compiler still writes nothing when that check fails before `create_dir`. If it fails after the files are built in memory, `create_dir` has not been called yet. The check runs before `create_dir`.
