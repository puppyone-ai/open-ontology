# G-S5c E309 rerun: 58 correct; two upstream timeouts

STATUS: partial. S5-G-01 remains **not done**. PostgreSQL and Oracle answer
20/20 correctly; MySQL answers 18/20, with two upstream timeouts rather than
wrong answers. Oracle's command additionally fails cleanup. No all-P0 or
full-fixture T5 acceptance is claimed.

The requested rebase reports already up to date with integration
`5eb79b842913fe10520284f1fcad3d403250d479`.
Tested lane commit: `b5d06d033b4baf749f9cbfd365960fcd025d711a`.
The only harness behavior change is the Agent egress timeout, 120 to 300
seconds, recorded in preflight and covered by a unit test. Builder timeouts,
call limits, one-hour Agent deadline, scoring, scale and model references
are unchanged. The integration question wording is used; gold SQL statements
are unchanged (only their question comments changed).

## Commands and results

```sh
tools/oo-test/oo-test t6 --engine postgres --run-id g-s5c-e309-pg-20261105 --execute
tools/oo-test/oo-test t6 --engine mysql --run-id g-s5c-e309-mysql-20261105 --execute
tools/oo-test/oo-test t6 --engine oracle --run-id g-s5c-e309-oracle-20261105 --execute
tools/oo-test/oo-test t6 --engine mysql --run-id g-s5c-e309-mysql-retry-20261105 --execute
```

| Measured engine/version | Run suffix | Correct | Failed | Not run | Command exit | Builder calls / limit | Agent calls / limit |
|---|---|---:|---:|---:|---:|---:|---:|
| PostgreSQL 17.11 | pg | 20 | 0 | 0 | 0 | 11 / 21 | 141 / 160 |
| MySQL 8.4.7 | mysql | 0 | 0 | 20 | 1 | 3 / 21 | 0 / 160 |
| Oracle 23.5.0.24.07 | oracle | 20 | 0 | 0 | 1, cleanup | 13 / 39 | 141 / 160 |
| MySQL 8.4.7 | mysql-retry | 18 | 2 | 0 | 1 | 11 / 21 | 133 / 160 |

All use the default 2,000,000 events and 200 small fixture tables, fresh
owned sources/targets, and the reviewed publisher. Only the 12 gold tables
undergo retained Self-build. Source gold answers agree byte-for-byte across
all four runs. No fixture-fence shortcut is used.

The first MySQL attempt fails during Builder call 3 with
`UPSTREAM_TIMEOUT`, surfaced as `s5_provider_http_504`; no Agent call occurs.
One infrastructure-only setup retry is recorded separately, preserving the
failed run. Neither dialogue failure is retried or rescored. Total dispatched
calls, including the failed setup: 453 (38 Builder, 415 Agent).
Currency cost and timed-out call token usage are unmeasured.

All three dialogue runs reach owner self-review, reviewed publication
`activated/ready`, 12 opened fences, native share-prompt enrollment,
`oo login`, and verified `oo whoami`.

## Per-question results

The MySQL column is the separately identified infrastructure retry. In its
original setup-failure run, Q01 through Q20 are all `not_run`, with
NULL/truncated/refused flags unknown.

| Question | PostgreSQL | MySQL retry | Oracle |
|---|---|---|---|
| Q01 | correct | correct | correct |
| Q02 | correct | correct | correct |
| Q03 | correct | correct | correct |
| Q04 | correct | correct | correct |
| Q05 | correct | correct | correct |
| Q06 | correct | correct | correct |
| Q07 | correct | correct | correct |
| Q08 | correct | correct | correct |
| Q09 | correct | correct | correct |
| Q10 | correct | correct | correct |
| Q11 | correct | correct | correct |
| Q12 | correct | failed: timeout | correct |
| Q13 | correct | correct | correct |
| Q14 | correct | correct | correct |
| Q15 | correct | correct | correct |
| Q16 | correct | correct | correct |
| Q17 | correct | failed: timeout | correct |
| Q18 | correct | correct | correct |
| Q19 | correct | correct | correct |
| Q20 | correct | correct | correct |

Wrong, refused and truncated answers: zero. Every attempted question records
`null=false`; for the two timeouts this means no returned answer contains
NULL, not that a usable answer exists. Q09/Q10's portable NULL/empty-note
counts pass on every engine. Raw `Q01.json` through `Q20.json`, model
responses and native CLI outputs are retained for all three dialogue runs.

## Diagnosis and ownership

1. **Model upstream / egress availability:** MySQL retry Q12 and Q17 fail on
   their first model call, before any CLI tool call. `usage.jsonl` records
   `UPSTREAM_TIMEOUT` at 300339 and 301376 milliseconds. These are neither
   SQL failures nor wrong/zero answers. The initial MySQL setup failure is
   the separate Builder egress timeout at 45625 milliseconds; E309 changes
   only the Agent timeout. No further timeout or budget expansion is applied.

2. **Oracle typed SourceEngine fix is exercised:** a new baseline is loaded.
   Q02 executes native `SUM(amount)` and returns 7275; Q05's numeric grouped
   aggregate and Q12's numeric customer ordering pass. Q07/Q08 use native
   timestamp filters; Q19's MIN/MAX returns 1.125 and 120.125. These are not
   the earlier text-column workaround or Agent-side aggregation.

3. **Gateway offset fix is exercised:** MySQL retry Q07 accepts
   `TIMESTAMPTZ '2026-01-01 00:00:00+00'` and its February upper bound,
   returning 60. The clarified Q05/Q13/Q14 pass on all engines. Q12 passes
   on PostgreSQL/Oracle; MySQL's timeout provides no interpretation evidence.

4. **Gateway's intentionally closed SQL shapes:** Q14 initially uses EXISTS;
   Q20 initially uses subqueries/set operations (also a CTE on MySQL).
   These receive `STATEMENT_NOT_ALLOWED`, then recover through supported
   queries. There are 3, 4 and 3 recorded tool failures on PostgreSQL,
   MySQL retry and Oracle respectively, not hidden acceptance failures.

5. **Test infrastructure cleanup:** Oracle `0044.json`/`0044.stderr` record
   Docker daemon unavailability during target-label inspection.
   `tools/oo-test/t1.py` `AgentDB.close` raises; inherited
   `tools/oo-load/test-gateway-cycles.py` `Live.close` then does not reach
   source cleanup. The original result remains `status=failed`,
   `containers_left=unmeasured`. Separate recovery evidence verifies exact
   lane/run ownership before removing the two Oracle-run containers and
   its named volume. No product source fix is inferred from this transient
   Docker error.

**Resource-limit deviation:** the MySQL retry was launched before the Oracle
cleanup failure was inspected. Four owned database containers briefly
coexisted, violating the two-container limit. Oracle resources were removed
immediately upon discovery. No unrelated container, volume, image or service
was stopped or removed. This deviation is not erased by final cleanup.

## Validation and evidence

`g-s5c-e309-validation-20261105` contains build/unit/document-check outputs,
revision bindings, the infrastructure retry record, cleanup diagnosis and
recovery, final Docker scans, and verification of every run-manifest file:

- `cargo check --workspace --locked`: passed.
- Locked CLI, Builder, Gateway, oo-load and release-coordinator builds: passed.
- `python3 -m unittest discover -s tools/oo-test -v`: 69 passed.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: passed.
- `retained_gateway_install` on each owned target: passed.
- All 2178 files listed in the four run manifests verified.
- Final lane-G container and volume scans: empty.

SHA-256 of each run's `SHA256SUMS`:

```text
baa33abf6f590df6b4c89a38cc4a935f1bded6256e112ae6db31be3529a3055f  g-s5c-e309-pg-20261105
e93621070e06559f7d5297d28828f433869e90faa2f22dcc26181311d3a007d1  g-s5c-e309-mysql-20261105
cc977976abede1964eaeec46b973ff6132525936d895f00109518da53afb3683  g-s5c-e309-oracle-20261105
033907bf5eb1bebcb67d854a5720374492dd570df4ac87f23cfa9bea24a703d8  g-s5c-e309-mysql-retry-20261105
```

No shared-file patch, Cargo.lock change or Rust edit. Rust formatting is not
applicable. No unrelated failing test is waived. Earlier evidence is retained.
Full-fixture T5, remaining P0 versions, T7 source audit, public deployment and
business acceptance were not tested. Per-table consistency, CROSS_TABLE_SKEW
and unmeasured source time remain unchanged.
