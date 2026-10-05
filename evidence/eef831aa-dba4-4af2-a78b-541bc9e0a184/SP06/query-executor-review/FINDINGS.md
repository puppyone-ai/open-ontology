# Findings — a184 SELECT executor review

Verdict: **APPROVE LIMITED** `oo_gateway::query_executor::DedicatedSession::execute_trusted_select` on `8a289f0c699abb8c707ca45ad45cb036abc3fc05`, tree `fbbfe506668cf4263d670a0909c1d5b664f7c81d` (`linux/smoke/archive-tree.txt`). Author evidence commit `ce040593f6af6a61a5c3e29abd494b1a863f32c5`.

The measure is contract `contracts/gateway/query-executor-v1.md` for one trusted, permission-bound `SELECT`. This review does not close SP06-W04, SP06-W05, or SP06-W06, does not publish a gateway, and does not claim a release.

No product, dependency, or gate line needs to change for this limited acceptance. The deciding fresh run is `linux/probe/oracle-test.log` (1 passed) and `live/oracle.txt`. `linux/probe/attempt-1/` through `attempt-7/` are preserved harness misses. `linux/smoke/` is the pre-overlay archive. `linux/smoke/attempt-1/` failed before cargo while the allow-site matcher missed a generic `async fn`; the rerun is the smoke that exited 0.

## F1. What the entry runs

`execute_trusted_select` (`crates/oo-gateway/src/query_executor.rs` lines 590–686) is the only entry. `UNTRUSTED_ADMISSION` is `Blocked` (line 87). There is no public untrusted statement function. The function calls `guard_select` on `original_sql` (lines 619–620) and the producer runs `GuardedPlan.executed_sql`. The request type has no executed-SQL field.

The fresh session read `pg_backend_pid()`, a 20-character `backend_start`, and required `pg_stat_ssl.ssl`. Column ACL on the shadow table: east hidden `42501`, east `n4` ok, west `n2` `42501`, west `n4` ok. East has no `INSERT` on `depot.a184_audit`. A lying catalog that names the ungranted hidden column returned `42501`, status failed. `INSERT` returned `write statement is not allowed` with audit delta 0. Admit failure returned `audit admission failed`, call count 0, audit delta 0.

The live audit hook is a second connection's autocommit insert into `depot.a184_audit`. It is not a production audit WAL.

## F2. Fresh oracle

`linux/probe/oracle-test.log`: 1 passed, 0 failed, Rust `1.94.0`, `--locked --offline`, `--test-threads=1`. The oracle scan rejected a missing file, a fixture password, `SELECT `, and `INSERT `. Clippy `-D warnings` on `--lib --test query_executor_a184_review` exited 0.

`live/oracle.txt`, PostgreSQL 17.10, TLS SAN `DNS:oo-a184-pg`, `track_functions=pl`:

- `untrusted_admission=Blocked`. `function_whitelist=not_claimed`.
- `typed_rows=4`, `typed_compare=exact`. The projection was int2, int4, int8, text, bool, and null, including both integer minima and maxima, empty text, `托盘`, and a newline. The harness compared those strings with the administrator read of the same physical columns.
- `star_columns=6`, `star_hidden_absent=yes`.
- `audit_matches_plan=yes`, `audit_delta_typed=2` (admit and complete). `rollback_xact_cleared=yes`. `who=role_and_login` (`oo_r_a184_east/oo_a184_login`). `role_reapplied_after_rollback=yes`.
- `row_trunc_rows=2`, `row_trunc_calls=3`, `row_trunc_status=succeeded`. The extra fetch ran the volatile function and was not appended. The call count was read after that backend disconnected. `byte_trunc_rows=0`, `byte_trunc_status=succeeded`, `byte_trunc_sqlstate=none`.
- `backpressure_open_xact=yes` with the one-row channel still holding the transaction open.
- `type_numeric_oid=1700`, `type_varchar_oid=1043`, both displays `result type oid … is refused`, both call counts 0.
- `literal_guard_class=text`, `literal_executed=yes`, `literal_calls=0`. That 0 is the pending `pg_stat_user_functions` counter on a backend that was still connected. The row-truncation case is the one that published a count.
- `physical_query_seen=yes`, `logical_query_absent=yes`, `activity_secret_absent=yes`.
- Displays recorded for cancel, deadline, types, admit, insert, roles, the omitted function, and the lying column do not contain `SELECT`, `INSERT`, the hidden cell, or `pt_`.

## F3. Deadline, cancel, and drop

The client `Instant` is taken at `execute_trusted_select` entry. `read_pages` (lines 1146–1225) wraps each `query_portal` page in `tokio::time::timeout(remain)`. Page size is 1. The server `statement_timeout` is set once to the remaining budget minus about 250 ms.

The deadline fixture held `ACCESS EXCLUSIVE` for an extra 1500 ms after the query was visible, and `depot.a184_pause()` sleeps 2 seconds per execution. Wall was 4500 ms. Result: `client_deadline_rows=1`, status `failed`, SQLSTATE `none`, elapsed 4543 ms, display `query deadline exceeded`. One page came back. The second page did not get a fresh 4500 ms. The server-timeout control used wall 1800 ms on a lock with no sleep: status `failed`, SQLSTATE `57014`, elapsed 1590 ms, 0 rows, display `query deadline exceeded sqlstate=57014`.

`CancelHandle::cancel` (lines 803–824) checks generation, acceptance, pid, and `backend_start` before `cancel_query`. A handle kept after the query finished returned `not_sent`. The next query on that session succeeded (4 rows). A handle taken while the next query was in flight returned `sent`, display `query canceled sqlstate=57014`, status `failed`. `classify_pg` (line 1708) maps a `57014` whose message starts with `canceling statement due to user request` to `Canceled`, and any other `57014` to `Timeout` while keeping the SQLSTATE. All 7 call sites pass `deadline_owned=false`, so an in-flight user cancel stays `Canceled`. The client-deadline branch (lines 1215–1223) does not read the server error. It returns `Timeout { sqlstate: None }`.

Immediate drop, with no lock, completed `partial`. The durable complete row says `partial`. A complete-hook failure stayed `partial` with `end_audit_ok=false` and did not insert a complete row. Neither drop was `succeeded`.

Drop while `query_portal` waited on `ACCESS EXCLUSIVE` did not take the shutdown branch. Wall for that case was 3000 ms. Result: `blocked_drop_status=failed`, `blocked_drop_sqlstate=57014`, `blocked_drop_elapsed_ms=2713`. That elapsed is the server timer, not an immediate cancel. The shutdown arm (lines 1182–1196) would have completed `partial` with step `consumer` and called `cancel_query`. The measured summary is the portal error arm (lines 1204–1211): `partial=false`, `desync=false`, then `cleanup` (lines 1076–1082) awaits rollback because `desync` is false, and `publish_stop` still drops the socket. The session was closed. The status is `failed`. The same shape was preserved on attempt 6 (2703 ms) and attempt 7 (2710 ms) before this deciding run. The statement stayed inside the client budget. This review does not edit `read_pages`.

## F4. Role reuse and when the audit row is written

Contract line 19 says a second role returns `RoleReuse` before the guard and before the audit hook. The code at lines 614–617 returns `RoleReuse` only when `bound_role` is already set. `bound_role` is stored at lines 952–953 after a successful read whose `reuse` flag is true. A guard refusal never reaches that store.

Fresh result after a successful east bind: a west request returned `dedicated session cannot change role` and `role_reuse_audit_delta=0`. Fresh result after `INSERT` was refused by the guard: `after_guard_refusal_other_role=allowed`. The later role ran. That is the contract sentence and the code, recorded side by side.

A role that passes `role_shape_ok` and then fails the flag check, or that has no `pg_roles` row, is `RoleParameter` inside the producer, after `admit`. `oo_r_a184_bypass` (`BYPASSRLS`, `INHERIT`), `oo_r_a184_login` (`LOGIN`), and `oo_r_a184_missing` each returned `role parameter is not the nologin shape` with audit delta 2. That is the admit row plus the complete row. The user statement was not accepted. The socket was dropped.

## F5. The catalog is trusted input

`PolicyCatalog` and `PolicyIdentity` are caller input. The guard's `function_schema_ok` allows `depot` because the catalog says so. Docs rule R4, a `pg_proc` whitelist that excludes volatile functions, is not what this slice implements. The a179 review already recorded that boundary for `03512f91`. This review did not rerun `tests/sql_guard.rs` and did not rebuild `pg_query`.

The fixture created volatile `depot.a184_touch`, `depot.a184_pause`, and `depot.a184_who`. Touch was executed when the catalog listed it. `a184_pause` omitted from the catalog returned `function or operator is not allowed`, audit delta 0, call count 0. `a184_who` returned the `SET LOCAL ROLE` identity, and a second query after rollback returned it again. This is not a function-whitelist proof.

## F6. Clippy allows

On the fixed source, before the overlay existed, the smoke counted arguments at angle-bracket depth 0: `run_producer` 12 (line 916), `drive` 10 (line 968), `read_pages` 8 (line 1146). Each has `#[allow(clippy::too_many_arguments)]`. The allow matches the count. Focused clippy `-D warnings` passed on that source and, after the harness fixes below, on the review test.

The review test's own allows are on `LiveAudit::insert` (8), `run_ok` (11), `open_stream` (11), and `exec_result` (11). Attempt 7 is the clippy failure that named those four and a one-trip loop. The loop is now a `match`.

## F7. What stays open

- the 500 ms process-isolated parser worker, authentication, and any untrusted admission path
- a production audit WAL
- row-level security, claims, and row filters
- a connection pool
- result OIDs outside bool, int2, int4, int8, text, and null
- docs R4 as a generated function whitelist
- parent SP06-W04, SP06-W05, and SP06-W06
- making the `read_pages` shutdown arm preempt a `query_portal` that is blocked in `Execute`

The limited acceptance is the trusted entry, the mapped physical `SELECT`, the role and column checks, the typed decode, the bounds that the oracle measured, and the failure statuses that stayed off `succeeded`.
