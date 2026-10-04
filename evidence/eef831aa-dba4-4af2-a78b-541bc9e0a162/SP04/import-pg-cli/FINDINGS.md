# Findings

Author record for `3fba40dd9fdebaa219ab4eb7fdb39b8fcf1a1e4a`. The green gate is `linux/live-6/`. Independent review has not run.

## Session rollback clears driver depth

`PgSession::begin` goes through `PgTransactionManager::begin`, which increments `transaction_depth`. The old `rollback` sent the allowlisted `ROLLBACK` text and left that depth at 1. The next custom `BEGIN` on the same session is `InvalidSavePointStatement`, mapped to `SourceError::protocol("driver")`.

`prepare_snapshot` hits that path: `inspect_access` begins and rolls back, then `catalog` begins again. `SnapshotOwner` begins once, so the a159 import did not. `linux/live/` through `linux/live-3/` failed with fact `driver`. `linux/live-4/` tagged the failure at `catalog` after `inspect_access` had allowed registration.

The committed rollback calls `PgTransactionManager::rollback`. At depth 1 the manager sends `sql::ROLLBACK` and then decrements depth. If the depth is not 1, or the depth-1 statement is not `sql::ROLLBACK`, the session is poisoned and the fact is `rollback`. No savepoint rollback is sent on that path. `linux/live-6` prepare and the binary import both passed after this change, which is the first green use of two begins on one preparation session.

## The live-5 import assertion rejected a real physical name

`linux/live-5/import.exit` is 101. The binary had already exited 0 and written `awaiting_publish` (`linux/live-5/out/success.stdout`, stderr empty). The test helper required every byte of the physical name, including `pt_` and `pc_`, to be a hex digit. `p` and `t` are not. The helper now checks the 60 lowercase hex characters after the prefix, which is the same rule the command uses. `linux/live-6` is the green rerun. The live-5 database was dropped by the test cleanup.

`linux/live-5/connectors-clippy.exit` is 1 because that container had not installed the clippy component. The library tests in that run passed (64). Clippy for both packages is the live-6 exit 0.

## In-memory RunSummary

`RunState` and `RunSummary` still have public fields. A Rust caller can construct `awaiting_publish` in memory. The command does not. Stdout is the value `import_unpublished` returned. The `_oo.full_import_run` row is the seal. A failed stdout write after that return exits 1 and leaves the row. `full_import.rs` was not edited. The disclosure is in `contracts/pipeline/import-pg-cli-v1.md` and the `import_cli` module docs.

## Vendor noise

Clippy stderr for both packages repeats two unfulfilled `#[expect(dead_code)]` annotations in `crates/oo-connectors/vendor/sqlx-postgres`. The package command still exited 0. Those files were not edited.

## What this run did not redo

a159 (`476d21c7`) remains the proof for cancel, source loss, deadline, quality, receipt conflict, and the rest of the unpublished import campaign. This run checked the new command path: public preparation, registration refusal, catalog drift after prepare, a missing `0005`, and one sealed two-table import. It did not publish, grant the agent, certify every PostgreSQL type, or close SP04.
