# Findings

Verdict: **APPROVE LIMITED** commit `3fba40dd9fdebaa219ab4eb7fdb39b8fcf1a1e4a`. No product change is requested. The fresh proof is `linux/probe-4/out/a164-outcome.txt` and `linux/probe-3/out/depth-outcome.txt`.

## Rollback depth

`PgSession::begin` sends the custom statement through `PgTransactionManager::begin`, which increments `transaction_depth`. A raw `ROLLBACK` used to leave that depth at 1, so the next custom `BEGIN` was `InvalidSavePointStatement` and the fact `driver`. `prepare_snapshot` takes that path: `inspect_access` begins and rolls back, then `catalog` begins again (`crates/oo-connectors/src/snapshot.rs` lines 472–477).

`crates/oo-connectors/src/session.rs` lines 757–783 now call `PgTransactionManager::rollback`. At depth 1 the manager sends `sql::ROLLBACK` and decrements depth (`crates/oo-connectors/vendor/sqlx-postgres/src/transaction.rs` lines 51–56). If the depth is not 1, or the depth-1 text is not `ROLLBACK`, the session is poisoned and the fact is `rollback`. No savepoint rollback is sent. `linux/probe-3/out/depth-outcome.txt` records depth 0 after inspection and after catalog, depth `1_0_1` for an explicit cycle, `depth0_poison=rollback_no_sql`, and `depth2_poison=no_rollback_sql` with the backend gone after drop.

## Command boundary

`crates/oo-pipeline/src/import_cli.rs` lines 87–92 check target readiness before `prepare_snapshot`. A missing `full-import/v1` row exits 1 with `oo-pipeline: target schema is not ready`, empty stdout, and no `0005` relation (`linux/probe-4/out/a164-not-ready.exit`). The command does not install a schema.

Stdout for the sealed run is `oo.pipeline.import-pg-result/v1`, state `awaiting_publish`, source generation 11, table count 2. Run `48ffd752-5224-4bca-ac1b-3525237b624e` is the stored row. The test compared that row's digests and applied counts with stdout, and compared shadow values with `yard.hulls` and `yard.fasteners`. A second snapshot stream produced the same canonical digests. `pg_publication` stayed 0. The agent received SQLSTATE `42501`.

`RunSummary` and `RunState` have public fields (`crates/oo-pipeline/src/full_import.rs` lines 62–117). The comment above `RunSummary` says a caller cannot build `AwaitingPublish` except through the import entry. That comment is older than this commit: `full_import.rs` is blob `dfaf2470419dc5e07fbe92ae60b099ab96ab788c` at both `cc764cbc` and `3fba40dd`. The new contract and `import_cli.rs` lines 8–12 say the opposite, and the review constructed an in-memory `AwaitingPublish` value whose run id is absent from `_oo.full_import_run`. Stdout is the returned summary. The database row is the seal. That is not publication.

## What stayed outside this approval

a159 (`476d21c7`) remains the proof for cancel, source loss, deadline, quality, receipt conflict, and the rest of the unpublished import campaign. This review ran the new command, the rollback depth correction, the existing `prepare_snapshot` live test, and the existing `import-pg` live test. It did not publish, grant the agent, certify every PostgreSQL type, or close SP04.

`linux/probe/` failed to compile the first review test (`find_column` lifetime). `linux/probe-2/` compiled and failed because the audit count included the fixture owner's setup inserts. Those logs are kept. They are harness failures, not product failures. The product tests in those same runs passed: depth, the existing prepare test, and the existing import test.
