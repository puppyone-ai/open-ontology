# Findings

Independent review of frozen commit `6ac2216505723b51ae116b1938aca47846ade35f` for `analyze-pg`. No product, library, manifest, or lock edit. The author a236 live test was not re-run. This review used a new database pair, new role names, and compiled binaries from the fixed archive.

## Result

APPROVE_LIMITED. No product defect showed up in the frozen command, the committed unit tests, the independent init/import/analyze chain, the five required refusals, or the fixture catalog query.

The three harness failures are preserved under `linux/overlay/attempt-1/`, `attempt-2/`, and `attempt-3/`. They were an unread `login_file` under Clippy `-D warnings`, a second `init-pg` against cluster-wide bootstrap roles, and `GrantMismatch` from a leftover `oo_r_clerk`. The green review Clippy log has no warning. The green pipeline Clippy log's only warnings are two unfulfilled `dead_code` expectations in vendor `sqlx-postgres`, and that command exited 0.

## What the green run shows

- Committed `analyze_pg_cli` compiled on Linux arm64 Rust 1.94.0 before the review file existed: 4 passed, 1 ignored, 0 failed. The ignored test did not open a connection.
- `init-pg` on empty `oo_a247_dst` exited 0 with `install=bootstrapped` and `full_import=installed`.
- `oo-pipeline import-pg` exited 0 with `state=awaiting_publish` and 2 rows. Source `app.party` `reltuples` bits were the `-1` sentinel. Target `public.sentinel` was the same sentinel. Source `app.write_audit` was 2 from the setup inserts.
- Wrong CA, operator `oo_a247_admin`: exit 4, stderr `analyze-pg: target connect`, empty stdout.
- Operator `oo_a247_runtime`, member of `oo_pipeline_runtime`: exit 5, `analyze-pg: runtime caller`.
- Operator `oo_a247_plain`, `CONNECT` plus `USAGE` on `_oo` and no `MAINTAIN`: exit 5, `analyze-pg: target role`.
- Generation 12 against stored generation 11: exit 6, `analyze-pg: stale generation`.
- Table `absent`: exit 6, `analyze-pg: mapping absent`.
- After those five refusals, target `reltuples` was still `-1`.
- `analyze-pg` for `app.party` exited 0. `published` false. `estimate_is_exact` false. `reltuples_bits` `1073741824`. `estimated_rows` 2. `disposition` `reusable`. A second call returned the same bits, still unpublished and inexact. Physical `count(*)` was 2. Sentinel bits stayed `-1`.
- `pg_stat_activity` rows in `idle in transaction` for the admin, runtime, plain, and reader roles were 0 after the commands exited. The analyze request JSON has no `transaction` key.
- Source audit count stayed 2. Rows attributed to `oo_a247_reader` stayed 0. Source `reltuples` stayed `-1`.
- Fixture `install_column_acl` returned `Installed` for role `oo_r_clerk`. `load_policy_catalog` with `UnknownRows::Refuse` loaded one relation whose `estimated_rows` was 2, equal to the receipt, and below `u64::MAX`. Login `oo_a247_login` through `DedicatedSession::execute_trusted_select` of `SELECT count(*) FROM app.party` returned that same count. The oracle records `statistics_published=false` from the analyze receipt.

## Boundary

Column ACL install and the typed query are a fixture so `UnknownRows::Refuse` can see the estimate the command just wrote. They are not behavior of `analyze-pg`. The command did not publish statistics. The receipt's timeout fields were not exercised as a campaign. An open caller transaction was not passed in. Forged-relation refusal was not repeated. Authentication, publication, and an audit WAL were not measured.

The author unit tests ran only as the frozen smoke. The live evidence for this review is `analyze_pg_a247_review`.

## Open

M1-D06B stays open. A publish command, an open-transaction injection, and the library timeout and forged-relation campaigns stay out of this verdict.
