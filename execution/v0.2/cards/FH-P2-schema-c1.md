## Card FH-P2 — schema classification, C1 isolation, exact reasons (S6-M-07, S6-M-06; D22)

Branch `v0.2/fh-p` reset to `803b9354888a7357a1cf73508cd7807cc5201afd` (v0.2/field-hardening: your pass 1 plus lanes Q and R merged). Commit as the owner,
no agent trailers, trailer `Plan: S6-M-06 S6-M-07`. Read your pass-1 report `tools/oo-load/VALIDATION-FH-P.md` and
`evidence/fhp-20261107/signoff.json`, plan §4 rows S6-M-06/S6-M-07, §5 T9 list, D22.

Coordinator decisions:
- `tools/oo-load/FH-P-CONNECTOR-API.patch` is APPROVED as written (additive `column_shape`, default None = unproven).
  You now also own `crates/oo-connectors/**` (lanes Q and R are finished). Migrations: **0127–0134 only**.
- Not yours: `deploy/lite/**`, `crates/oo-cli/**`, `crates/oo-gateway/**`, `tools/pg-builder/**`, `execution/**`,
  `specs/**`, `crates/oo-pipeline/src/bin/install-retained-target.rs` (the coordinator wires installers).

Build:
1. Apply the patch; implement `column_shape` for PostgreSQL (incl. KingbaseES PG mode), MySQL (5.7/8.0/8.4/9.x) and
   Oracle (11g XE/19c/21c/23ai) from native metadata within the existing doctor GRANT lists; `target_type` uses the
   exact mapping the chunk reader uses. Other engines keep None.
2. Sealed column catalog as formal target state (migration), written when a table's baseline is sealed and refreshed
   on each successful rebaseline; one shared classifier in oo-load used at every call site (C5 layered/range/empty and
   C1), with exactly the classes in plan S6-M-07: additive and unpublished_only pass (new columns not published);
   same-target-type widening passes with no rebuild; target-type-changing compatible widening rebuilds that table into
   a new physical generation switched atomically at publish (old generation serves meanwhile); other published-column
   changes → stale `schema_drift:<column>:<kind>`; no sealed catalog or None shape → `schema_unproven`.
3. C1 (log CDC) per-table isolation with the same status/stale fields and exit codes; define recovery semantics for a
   stale C1 table (its position does not advance; it rejoins via C5/rebaseline) and document them in the report.
4. Exact reasons: source permission loss must be `source_permission` (not `schema_unproven`) on PG/MySQL/Oracle;
   statement timeout `source_timeout` (add a deterministic timeout injection to the harness, e.g. a lock held on that
   table with a short statement/lock timeout, or per-engine `max_execution_time`/resource limits).
5. Tests: classifier unit tests per class per engine type mapping; T4 strict cases and T9-01…T9-07, T9-11 in
   `t3t4.py`/`t9_isolation.py` must pass on PostgreSQL 17, MySQL 8.4 and Oracle 23 with the final binary
   (sequential per engine, at most two DB containers, label oo.lane=fhp); full T3/T4 on those three; pipeline lib,
   oo-load (default and mysql-binlog), oo-connectors all-features lib, workspace clippy -D warnings, fmt.
List secret-review-bound files you change. Report: commits, classes per engine, C1 semantics, tests with counts,
pass/fail per case per engine, gaps.
