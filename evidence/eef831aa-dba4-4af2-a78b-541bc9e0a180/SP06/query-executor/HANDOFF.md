# Handoff

Source commit: `8a289f0c699abb8c707ca45ad45cb036abc3fc05`

Contract: `contracts/gateway/query-executor-v1.md` (`oo.gateway.query-executor/v1`).

Library entry: `oo_gateway::query_executor::DedicatedSession::execute_trusted_select`.

## Read first

1. `REPORT.md` for the green exits and the oracle.
2. `FINDINGS.md` for the statement-timeout restart, the function-stat flush, and the clippy scope.
3. `linux/live/pg-test.log` and `live/oracle.txt` for the raw comparison.
4. `linux/live-attempt-1` through `linux/live-attempt-8` for the misses. Do not delete them.

The fixed guard ancestor is `03512f91324ac33e772e9702fe806e8e04037ec4`. It is an ancestor of the source commit. The source commit's parent is `5abe6bdcfa75c9b6f70a7f756481bb25ddc193d9`.

## What a reviewer should check

- The caller cannot supply executed SQL. `SelectRequest` has `original_sql` only. `guard_select` runs first.
- Success awaits `rollback` and re-reads `backend_pid` and `backend_start` before reuse. `Transaction` drop is not treated as a confirmed rollback.
- `CancelHandle` stores the generation from query start. A late handle returns `NotSent`. The live oracle records `late_cancel=not_sent then sent`.
- Unknown result OIDs fail before `bind`. The live numeric column was granted to role A and still returned `TypeRefused` for OID 1700, with no `note_read` calls.
- Cross-role reuse returns `RoleReuse` before the guard and before audit.
- `admit` failure does not start the user statement. The fixture insert is a second connection, not a WAL writer.
- Page size is the constant 1. `query_portal(..., 0)` is not used for user SQL.

## Rerun

The green command is `linux/launch-live.sh`. It expects `oo-a180-pg` on `oo-a180-net`, the pinned Rust image already present, and fixture secrets mounted at `/run/oo-a180/fixture`. Inside the container, `linux/run-live.sh` fetches with `--locked`, runs the lib tests, installs clippy for 1.94.0 when `HOME=/tmp` has none, runs the scoped clippy gate, then `OO_A180_LIVE=1` for the integration test.

This handoff's cleanup removes that PostgreSQL, the builder, the volumes, the network, and `/tmp/oo-a180-eef831aa-dba4-4af2-a78b-541bc9e0a180`. A later rerun has to build those again. It must stay inside the same cap: one builder and one PostgreSQL for this task.

## Cleanup handles

Owned by this task and removed after the evidence commit:

- container `oo-a180-rust194`
- container `oo-a180-pg`
- volumes `oo-a180-cargo`, `oo-a180-target`, `oo-a180-pgdata`
- network `oo-a180-net`
- directory `/tmp/oo-a180-eef831aa-dba4-4af2-a78b-541bc9e0a180`

No `docker system prune`. No other task's containers, volumes, or networks.

## Leave open

Do not mark SP06-W04, SP06-W05, or SP06-W06 accepted from this slice. Do not describe the gateway as accepted. The 500 ms parser worker, authentication, the production audit WAL, row filters, and a pool are still absent.
