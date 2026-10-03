# HANDOFF — a130 reader primitives

Independent functional review of the fixed archive `ccd93e1adc34b24ab998d90972549f8cffb59f0d`. This evidence commit is the review record. It does not change product source and it does not close a checkbox.

| | |
| --- | --- |
| Product commit | `ccd93e1adc34b24ab998d90972549f8cffb59f0d` |
| Tree | `ef43877346afadcdccaf26fe6982c6b4e8b65902` |
| Parent | `182702e6e67d16de10f280eaf422fce4e2f5a5f2` |
| Schema ancestor | `2254eab39c14a1902e65e72eb3674debc1c54f03` |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a130` |

## Verdicts

| Slice | Verdict |
| --- | --- |
| Schema and current catalog | APPROVE |
| Catalog rate | APPROVE |
| Named portal | APPROVE |

The driver integration gate stays open. `crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs:590` (`incoming view shrank during advance`, blob `f41e0e4dd6baca062fb332ab66da3f5e9c0600fd`) is a known failure from a128's 60000-row read and is assigned to a129. This review did not rerun that payload and does not clear it. R1 and whole-reader completion still depend on that gate.

## Read first

1. `FINDINGS.md` — slice evidence, source lines, SQL audit, and the three failed proof attempts.
2. `REPORT.md` — images, toolchain, commands, and exits.
3. `linux/proof/console.log` — the passing four-test run.
4. `linux/proof/sql-audit.txt` and `linux/proof/postgresql.log` — own reader log, suffix after `a130_own_log_cut`.
5. `linux/proof/attempt-1`, `attempt-2`, and `attempt-3` — compile and overlay failures, left as they finished.
6. `linux/author/console.log` — unmodified `catalog_rate_live`, `snapshot_live`, `pg_portal_bounds`, `pg_portal_live`.
7. `linux/smoke/console.log` — lib 55 and clippy under Rust 1.94.0.

a125 `snapshot_live` exit 101 at `max_rows_per_sec` remains the pre-rate-repair record. a128's later green `snapshot_live` on `ccd93e1a` superseded that binary. This session's own unmodified `snapshot_live` also exited 0. Those older files were not rewritten.

## Still open

All 87 goals. Arrow consumption of portal rows. R3. R4. Import. CLI. Harness. Extensions. Rate control of the whole reader. The process-local `MAX_ROWS_PER_SEC` ledger is not backend heap and is not caller-retained RSS.

`PgConnector` still exposes `new`, `connect`, `inspect_access`, and `catalog`. The named portal remains a caller statement on an existing `sqlx` transaction.

## Runtime

`cleanup.txt` records removal of this session's containers, network, volumes, and `/tmp/oo-a130-eef831aa-dba4-4af2-a78b-541bc9e0a130`. a125, a128, and other sessions' volumes were left in place. Nothing was pruned. Nothing was pushed.
