# FH-P2 handoff

Status: partial. S6-M-06 and S6-M-07 are not closed.
Base: `803b9354888a7357a1cf73508cd7807cc5201afd`.
The pass-1 report and all unsuccessful development evidence are preserved.

## Implemented

- Applied the approved `FH-P-CONNECTOR-API.patch`. Fresh native column facts
  are implemented for PostgreSQL/Kingbase PG mode, MySQL and Oracle.
  Unsupported adapters retain the default `None`.
- Metadata includes native ordinal, declaration, nullability, charset,
  collation and primary/unique-key positions. Target storage follows the
  chunk reader's physical mapping rather than legacy COPY varchar storage.
  PostgreSQL enum/domain evidence is retained; complex unique indexes are
  conservatively unproven.
- Migration 0129 makes `_oo_load.column_catalog` formal owner-side state.
  Baseline publication and the source catalog seal commit together.
  Snapshot/C5 rebaseline replaces the seal. Legacy hashes are not backfilled.
  C5 may refresh observed columns without expanding the sealed projection.
- `oo_load::schema::classify` is the common C5 classifier, before keyed,
  layered, keyless and empty-table branches. Additions and unpublished-only
  changes pass; new columns stay outside the projection. Proven same-target
  widening passes. Changed-target compatible widening uses a complete new
  physical generation. Other published changes produce named drift.
- Rebuild changes the local intake view transactionally; an unexpected view
  dependency fails without CASCADE. Immutable earlier publication generations
  are not dropped or rewritten. Existing publication/semantic gates remain.
- Explicit table-access probes preserve the driver's permission reason.
  Deterministic timeout tests use a table lock on PG/MySQL and a fixture-owner
  VPD delay on Oracle. The reader's statement budget is 500 ms. Oracle thin
  cancellation can close its connection; recognized timed-out calls reconnect
  before another table is attempted. Unrelated connection errors remain global.
- The existing C5 contract is preserved: one retry, stale payload retained,
  immutable cycle replay, exit 0/3/4 for healthy/partial/all-stale, and exit 1
  for source-wide or target failures.

## Classes and reasons

The three-engine live T9 matrix exercises these source-to-target mappings:

| Engine | Same-target widening | Changed-target widening |
|---|---|---|
| PostgreSQL 17 | varchar(80) -> varchar(160), both text | integer -> bigint |
| MySQL 8.4 | varchar(80) -> varchar(160), both text | int -> bigint |
| Oracle 23 | VARCHAR2(80) -> VARCHAR2(160), both text | NUMBER(10,0)/bigint -> NUMBER(19,0)/numeric(19,0) |

The harness checks zero new generations for the string case and exactly one
for the changed-target case. It also checks `additive`, `unpublished_only`,
published drop/rename/nullability refusal and missing-catalog refusal.

Named reasons are `schema_unproven`, `source_permission`, `source_timeout`,
or `schema_drift:<column>:<kind>`. Drift kinds are `dropped`, `renamed`,
`nullability`, `type`, `key`, `collation`, and `ordinal`. The classifier tests
also refuse narrowing, signedness changes and decimal fractional/integer
capacity loss. Unknown declarations do not count as proven widening.

## C1: not implemented

C1, including its MySQL handoff path, still uses the pre-existing source-wide
`PgSync` binding, lineage and
durable cursor. It does not use the new classifier, produce per-table stale
outcomes, or have a table-local recovery fence. Handoff does not yet supply
native facts to its rebuilt baseline, so that baseline remains unproven
rather than inheriting a potentially obsolete column seal. No skipped CDC batch is
acknowledged on the false premise that a quarantined table was applied.
Strict C1 T4 failures remain failures; their expectations were not relaxed.

Required recovery semantics for the remaining implementation:

1. Persist quarantine and the table's last applied position before permitting
   healthy-table progress or source feedback past unapplied events. Its
   table-local position and retained data must not advance while stale.
2. A quarantined table must not resume from the healthy tables' newer cursor,
   even if its permission or schema problem disappears.
3. C5/rebaseline must prepare a new generation under the table lock, preserve
   the old generation until success, and establish an explicitly aligned new
   source position before rejoining C1. Clear stale state only with that
   successful transition. Reusing an old overlay or guessing alignment is
   forbidden.

These are required follow-ups, not behavior delivered by this branch.
The existing full-source C1 failure behavior remains fail-closed.

## Validation scope

Raw outputs: `evidence/fhp2-20261107/` plus the six final engine run directories
listed in the result tables. Every final engine command uses the same frozen
`evidence/fhp2-20261107/oo-load-final` binary. Ignored/not-run cases are not
passes. No baseline-failure exemption was used.

The live T9 harness proves source-to-local-target C5 behavior. It does not
prove a public Gateway query, customer acceptance, or concurrent Gateway
readers across a changed-type activation. The unchanged pass-1 publication
tests are not substituted for that missing widening-publication acceptance.
Other PG/MySQL/Oracle versions and Kingbase PG mode were not live-tested in
this pass. Keyless same-target widening's zero-generation property also
needs a dedicated live test; its source-digest path can still stage a whole
generation on metadata-only changes.

## Final results

Frozen mysql-binlog binary SHA-256: `a032772901f763cf39cbc8301fe5610ae9ab3ffca3968602edb64826c2a2b987`.

| Command | Result | Log in fhp2-20261107 |
|---|---|---|
| `cargo check --workspace --locked` | pass | final-check.log |
| `cargo test --locked -p oo-pipeline` | 359 pass, 55 ignored; library 266 pass, 15 ignored | final-pipeline.log |
| `cargo test --locked -p oo-connectors --all-features --lib` | 331 pass, 7 ignored | final-connectors.log |
| `cargo test --locked --manifest-path tools/oo-load/Cargo.toml` | 43 pass, 7 ignored | final-load-default.log |
| Same command with `--features mysql-binlog` | 44 pass, 7 ignored | final-load-binlog.log |
| `cargo clippy --workspace --locked -- -D warnings` | pass | final-clippy.log |
| Connector all-feature clippy, `-D warnings` | pass | final-connectors-clippy.log |
| Touched-crate fmt checks; Python compile checks | pass | final-fmt.log, final-python.log |
| check_spec.py, check_plan.py, check_v02_plan.py | pass (document checks only) | check-spec.log, check-plan.log, check-v02-plan.log |

Final engine commands:

```sh
python3 tools/oo-test/t9_isolation.py --engine ENGINE --run-id RUN --binary evidence/fhp2-20261107/oo-load-final
OO_TEST_LANE=fhp python3 tools/oo-test/t3t4.py --engine ENGINE --versions VERSION --run-id RUN --binary evidence/fhp2-20261107/oo-load-final
```

| Engine | T9 pass/fail | Full T3/T4 pass/fail/not-run/N/A |
|---|---|---|
| postgres 17.11 | 11/0 | 10/5/0/2 |
| mysql 8.4.7 | 11/0 | 10/5/0/2 |
| oracle 23.5.0.24.07 | 11/0 | 9/5/1/2 |

Each engine has five strict T4 failures: every C5 step passes, but the C1
step fails for add_column, drop_column, rename_column, widen_type and
widen_string. PostgreSQL reports `sync_rebaseline_required_lineage_or_scope`,
MySQL reports `binlog_statement_new_generation_rebaseline`, and Oracle
reports `logminer_table_rebaseline`. These are unmet card exits, not exempted
baseline failures. Oracle retention_expiry is not run because its pinned
slim image has no RMAN. Oracle resetlogs_incarnation passes.

### T9 per case

| Case | PG 17 | MySQL 8.4 | Oracle 23 |
|---|---|---|---|
| T9-01 | passed | passed | passed |
| T9-02 | passed | passed | passed |
| T9-03 | passed | passed | passed |
| T9-03-unpublished | passed | passed | passed |
| T9-04 | passed | passed | passed |
| T9-05 | passed | passed | passed |
| T9-06-drop | passed | passed | passed |
| T9-06-rename | passed | passed | passed |
| T9-06-nullability | passed | passed | passed |
| T9-07 | passed | passed | passed |
| T9-11 | passed | passed | passed |

### Full T3/T4 per case

| Case | PG 17 | MySQL 8.4 | Oracle 23 |
|---|---|---|---|
| add_column | failed | failed | failed |
| drop_column | failed | failed | failed |
| rename_column | failed | failed | failed |
| widen_type | failed | failed | failed |
| widen_string | failed | failed | failed |
| add_table | passed | passed | passed |
| drop_table | passed | passed | passed |
| pk_change | passed | passed | passed |
| source_restart_mid_chunk | passed | passed | passed |
| network_drop_mid_c1 | passed | passed | passed |
| network_drop_mid_c5 | passed | passed | passed |
| target_restart_mid_publish | passed | passed | passed |
| sigkill_mid_cycle | passed | passed | passed |
| retention_expiry | passed | passed | not_run |
| slot_invalidation | passed | not_applicable | not_applicable |
| gtid_purge | not_applicable | passed | not_applicable |
| resetlogs_incarnation | not_applicable | not_applicable | passed |

### Engine run IDs

- postgres: `fhp2-final-t9-postgres-20261107`; `fhp2-final-t3t4-postgres-20261107`.
- mysql: `fhp2-final-t9-mysql-20261107`; `fhp2-final-t3t4-mysql-20261107`.
- oracle: `fhp2-final-t9-oracle-20261107`; `fhp2-final-t3t4-oracle-20261107`.

All six engine runs used the hash above. All reported cleanup lists are empty.
Post-run Docker inventory also found no remaining fhp containers or volumes.

## Integration

- No Cargo.lock, root manifest, existing migration, deployment, CLI, Gateway,
  builder, spec, execution record, or customer-system changes.
- `FH-P2-INSTALLER.patch` is the exact requested coordinator-owned installer
  change. `git apply --check` passes. It was not applied. Runtime `PgTarget`
  already installs migration 0129 idempotently.
- Secret-review-bound changed files (review ledger not self-approved):
  `crates/oo-connectors/src/mysql/engine.rs`,
  `crates/oo-connectors/src/oracle/engine.rs`,
  `tools/oo-load/src/lib.rs`, `tools/oo-load/src/main.rs`,
  `tools/oo-test/t1.py`.
- All fixture resources use `oo.lane=fhp`; engine runs are sequential, with
  at most one owned source and one owned target database at once.

Remaining gates are C1 isolation/classification/recovery, its strict T4
acceptance, changed-type Gateway activation acceptance, and the unmeasured
table-shape/version cases above. This is not a complete D22 acceptance.
