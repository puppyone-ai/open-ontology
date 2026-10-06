# Findings

No product defect turned up in the sealed tree at `8f533140a2823568c5f375190f8708934cd413fa`. The proof compiled that archive, and the live suites matched the hand expects. The notes below are what the raw files show.

## Harness failure, preserved

`linux/proof-1/docker-run.exit` is 1. Clippy exit 0. sql-runner tests exit 0. The console stops in `tools/fill.rs` with E0618: a local binding named `exes` shadowed the function, and a later call used the binding. Libclang was not unpacked and no suite ran. `WHY.txt` records that. The rename to `exe_json` and `bad_exe_json` is in the review harness. `benchmarks/sql-runner` was not edited. The rerun is `linux/proof/`, docker exit 0.

## Matched truncated prefix fails

Fail suite, case `trunc`. Query exit 0, adapter exit 0, comparator exit 0, comparator verdict `match`, `end_audit_ok` true, `partial` false, `truncated` true, status `fail`, reason `truncated`. The trial stays in `trial_count` 4 and `fail_count` 4. The comparator output is 90 bytes and contains no cell (`attempts/facts.txt`). The measured truncate bound for this database is 40 (`truncate-bytes.txt`, `oracle-facts.txt`). A match of the prefix is a failed gate. Runner exit for the suite is 1. Stderr is empty.

## Refusals stay in the denominator

Seal (`SELECT seal`) and hold (`SELECT id FROM dispatch.hold`) stop at query exit 6, reason `refused`, adapter and comparator null. The peek suite's hidden case (`SELECT lane` as the peek role) is the same shape: query exit 6, reason `refused`. The peek notes case passed with comparator verdict `match`. Peek is its own suite because the suite role and the identity role have to agree. Question count 2, trial count 2, pass count 1, fail count 1, `not_run_count` 0, runner exit 1.

Changed gold is a real mismatch: query exit 0, comparator exit 1, verdict `mismatch`, status `fail`, reason `mismatch`.

## Timeout and the cleanup word

Timeout suite: question count 1, trial count 1, timeout count 1, fail count 0, `not_run_count` 0, `passed` false, runner exit 1. The trial has `query_exit` null, adapter null, comparator null, reason `timeout`, status `timeout`, `backend_cleanup` `unknown`.

`unknown` is written once `query-pg` has been spawned. On this run it is present for passes, mismatches, refusals, the truncation failure, the missing completion, and the timeout. JSON null would mean the query was not spawned. The word records that the runner did not observe a backend release.

The after-runner snapshot, taken before admin release, still shows pid 162 in `pg_sleep(120)` and pid 180 active with wait event Lock. The physical query text on pid 180 was removed from `backends-after-runner.stdout` and replaced with `[withheld physical sql]`. Pid, state, wait event, and backend type remain. Admin `release.sql` terminated pid 162 only. The sweep returned 0 rows. The following snapshot shows the observation connection alone. That sequence is an admin action on the sleep, followed by the waiting backend disappearing. It is not the runner observing or performing a release.

## Missing completion

Noend suite: query exit 7, reason `no_end`, `end_audit_ok` null, adapter and comparator null, trial count 1, fail count 1, `not_run_count` 0, runner exit 1. The audit file the runner pointed at already contained `keep` plus a newline (`attempts/noend-audit.jsonl`). Occupy exited 0 on the first try. The retry branch in `inside.sh` did not run.

## Bounds

A flipped `query_pg` digest exits 2 before `create_dir`. Stdout is empty. Stderr is `sql-runner: hash /executables/query_pg/sha256`. `attempts/bad-hash-rundir.txt` is `absent`.

A request that contains the gold path exits 2 the same way. Stderr is `sql-runner: gold /cases/0/request_file`. The run directory is absent. The poisoned request is kept at `linux/proof/built/request-poison.json` as the rejected input.

The ten rewritten requests that the successful suites actually ran contain no secret and no gold path (`requests_scanned=10`). The two exact-trial request files differ.

## Children

`/proc` was scanned after live, pass, fail, peek, timeout, noend, bad-hash, and gold-poison, before any host kill of a runner child. Each `*-children.txt` is empty. The runner had already waited and reaped, or had not left a live child. A later admin terminate is a separate fact and is described above.

## Report contents

Reports are compact JSON, one trailing newline, same bytes on stdout and in `report.json`. Root fields include `executable_sha256`, the counts, `lane`, `metadata`, `mode`, `passed`, `question_count`, `trial_count`, `trials`, `tuple`, and `version` `oo.benchmark.sql-runner-report/v1`. Trial objects carry exits, `backend_cleanup`, `case_id`, `comparator_verdict`, `end_audit_ok`, `index`, `partial`, `reason`, `status`, and `truncated`. They do not carry SQL, gold, cells, a password, a CA, or a path. Suite stderr files are empty.

`live/desk-explicit.stdout` does not contain the cell bytes. It is `withheld` and `bytes=937`. The oracle file keeps counts and digests. Audits keep hashes, role, and sizes.

## Out of scope

This review did not edit product source, the root manifest, or the root lock. a207 remains the sole writer of those connector files. a242 benchmark-tools evidence was not re-reviewed.

The live fixture is the frozen installer in `crates/oo-gateway/tests/query_pg_cli.rs`. Building `query-pg`, the adapter, and the comparator from the archive, and running that one filtered test, supplies the executables and the rows. It does not reopen B-D01, B-D02, or B-D03.

`cmpset` and the tight-tmpfs attempts were not repeated. The sealed tests already include a comparator refusal, and nothing in this run failed in a way that required those extra attempts.

Tuple labels `fixture-a244` and `2026-10-06` name this review's report. They do not certify a snapshot. The alias `oo-a198-pg` is this review's Postgres, `oo-a244-pg`.
