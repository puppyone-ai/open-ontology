# Findings

No product defect remains in the impl-2 tree that the proof ran. The notes below are the boundaries the live files actually show.

## Carried into the proof

The first compile is preserved under `linux/impl`. Docker exit 101. `tests/cli.rs` bound the name `request` to a `PathBuf`, and two later calls to the helper failed with E0618. rustfmt had already exited 0. The helper calls were renamed and the proof tree is impl-2.

`tools/occupy.rs` line 7 imports `self` and does not use it. The proof compiler printed that warning and still exited the tool phase 0. Occupy itself exited 0 and created the five-byte audit file. The runner sources were not changed to silence an evidence helper, and the proof was not rebuilt for that warning.

A matched truncated prefix fails the trial. In `live-fail`, case `trunc` has query exit 0, adapter exit 0, comparator exit 0, verdict `match`, `truncated` true, `end_audit_ok` true, status `fail`, reason `truncated`. The adapter completion word is not the runner's success condition.

Changed gold mismatches: comparator exit 1, verdict `mismatch`, status `fail`. The comparator name `set` is accepted by the suite parser and refused by the comparator: exit 2, verdict `absent`, reason `comparator_refused`.

Seal and hold both stop at query exit 6, reason `refused`, with adapter and comparator exits null. The observe listing shows one mapped table, `dispatch.ticket`, and one unmapped relation, `dispatch.hold`. The roles the frozen fixture installs include `oo_r_desk` and `oo_r_peek`.

Missing completion, as asserted, is query exit 7 with `end_audit_ok` null and reason `no_end`. The audit file already held `keep` plus a newline, so `query-pg` could not create it. The two tight tmpfs attempts exited 2 with stderr `sql-runner: io` and wrote no trial report. `end_audit_ok` false was not observed in this proof.

The timeout trial has query exit null, status `timeout`, and `backend_cleanup` `unknown`. The during-lock snapshot shows the sleep backend and the observation query. It was taken before `timeout-ready`, so it does not show the user query waiting. After the admin `release.sql` and `sweep.sql`, the backend count is 1. That cleanup is the admin session, not a runner session release.

Every planned trial in the four reports is present. `not_run_count` is 0 on each. Failures stay in `trial_count`. Gate failure is runner exit 1 with empty stderr. The exact suite is runner exit 0.

`metadata` is the string `declared`. `snapshot` `fixture-a231` and `as_of` `2026-10-06` are report labels.

## Out of scope

The runner does not score prose, call a model, or certify a database snapshot. B-D01, B-D02, and B-D03 stay on their own reviews. This proof builds their binaries and runs one filtered live fixture so the runner has real executables. `phases/norun.exit` 0 is `cargo test --no-run` for `query_pg_cli`, not a replay of that suite.

Query requests name password and CA files. Admin `psql` used `-w` and local trust. No password file, private key, or certificate was copied into this directory. `PGPASSWORD` appears in `pg/psql-admin.sh` only as an `unset`.

The failed impl container and the proof container were removed. `linux/impl` stays as the failed attempt. Acceptance of B-D04, a parent rate, and a release stay open.
