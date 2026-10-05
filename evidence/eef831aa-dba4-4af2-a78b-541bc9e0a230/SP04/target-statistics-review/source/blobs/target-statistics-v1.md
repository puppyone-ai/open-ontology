# target-statistics/v1

Explicit maintenance of mapped shadow statistics. Protocol `target-statistics/v1`. A trusted operator connection, a mapping identity, and a bounded logical-table list produce one `ANALYZE` of those mapped `_oo` shadows and a measured `pg_class.reltuples` receipt.

This is not an import step. `full_import` and `import-pg` do not call it. It does not connect to a source database. It does not publish a table, grant a column, or claim that the estimate is an exact row count.

## Request

`analyze_mapped_shadows` takes one idle target `PgConnection` and one `StatisticsRequest`:

| Field | Rule |
|---|---|
| `project` | Closed source token. Must equal `source`. The policy catalog uses this token as its project key. |
| `source` | `physical_mappings.source`. |
| `config_digest` | 64 lowercase hex characters. |
| `source_generation` | Integer ≥ 1. Must equal `max(run_generation)` for that source and digest. |
| `tables` | 1 through 8 logical namespace/table pairs. No duplicates. No physical names. |
| `statement_timeout_ms` | 1 through 30000. Set with `set_config(..., true)` in the maintenance transaction. |
| `lock_timeout_ms` | Same bound and the same local setting. |

A closed field, an empty list, a duplicate, or a timeout outside that range returns before any statement. A logical name that is not a `pgident/v1` table name is `Bound`. Text such as a quote or a second statement is not interpolated.

## Transaction

The connection must not already be inside a transaction. The call opens one transaction and sets `statement_timeout`, `lock_timeout`, and `search_path=pg_catalog` locally. It then refuses:

- `current_user` of `oo_pipeline_runtime`, or a non-superuser `pg_has_role(current_user, 'oo_pipeline_runtime', 'member')`. A superuser is not treated as that member.
- `transaction_read_only`, `default_transaction_read_only`, or recovery
- a generation that is not the stored maximum (`StaleGeneration`)
- a lookup that raises `OO024` (`MissingTable`)
- a stored physical name that is not the recomputed `pt_` name (`ForgedRelation`)
- no ordinary table in `_oo` with that name (`MissingRelation`)
- a relation whose `relkind` is not `r` or whose owner is not `oo_target_owner` (`ForgedRelation`)
- a caller without `MAINTAIN` on that relation (`RoleDenied`)

Only after every requested table passes does the call run `ANALYZE` on `"_oo"."pt_…"` using `quote_physical_name`. The quoted text is the stored mapping identifier. It is not a caller string.

The same transaction reads `reltuples`. `-1` is `EstimateUnknown` and the transaction rolls back. Any other negative, NaN, or infinite value is `EstimateUnusable` and rolls back. A finite value of at least zero is rounded half away from zero into `estimated_rows`. The receipt stores `reltuples_bits` from `f32::to_bits` and sets `estimate_is_exact` false and `published` false.

`55P03` is `LockTimeout`. `57014` is `StatementTimeout`. Both roll the maintenance transaction back.

## Connection disposition

`ConnectionDisposition::Reusable` means one simple-query probe of `statement_timestamp() > transaction_timestamp()` is false. A simple Query sets both timestamps together on the first command of a transaction, so no caller transaction remains. The extended protocol sets the transaction timestamp at Bind and the statement timestamp at Execute, and that comparison is true on an idle connection. `FreshConnection` means rollback failed, the simple-query comparison is still true, the probe itself failed, or the driver reported `25P02`. The call does not roll back a transaction it did not open. A refusal before `BEGIN` leaves the connection reusable.

The display text has the reason and the disposition. It has no row value, no sample, and no password.

## What a receipt is not

`ANALYZE` updates the planner estimate. It does not prove the exact cardinality. A later policy catalog load may use that estimate when `UnknownRows::Refuse` would have rejected `-1`. The receipt is not publication, not a column grant, and not a classification. Catalog facts on the receipt are the identity, the physical name, the measured bits, the rounded estimate, the timeouts, and the disposition.
