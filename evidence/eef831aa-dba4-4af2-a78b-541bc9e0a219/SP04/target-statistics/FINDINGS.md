# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a219`. Product commit `613aa981dc2c57c0005cc6589e1a8ee7757f2a69`. The green overlay is attempt 7 in effect and is stored as `linux/overlay/` rather than under `attempt-N`. Attempts 1–6 are unchanged.

## F1. Idle detection has to be a simple query

Attempts 4 and 6 returned `OpenTransaction` with disposition reusable on a connection that had no caller transaction. `pg_stat_activity.xact_start < query_start` is true for an implicit single statement, because the transaction timestamp is recorded first. `query_scalar` uses the extended protocol: Bind stamps `transaction_timestamp` and Execute stamps `statement_timestamp`, so `statement_timestamp() > transaction_timestamp()` is also true on an idle connection.

The shipped probe is one `raw_sql` simple query of that comparison. PostgreSQL sets both timestamps together on the first command of a simple Query. A later command means the caller already opened a transaction. The open-transaction case still sleeps 10 ms after `BEGIN` so the following probe is clearly a later command. Attempt 6 note: `linux/overlay/attempt-6/note.txt`.

## F2. This sqlx implements Executor for `&mut PgConnection` only

Attempt 5 failed to compile `execute(tx)` and `fetch_one(tx)` where `tx` is `&mut Transaction`. The patched Executor impl is for `&mut PgConnection`. `ANALYZE` and the `reltuples` read pass `&mut **tx`.

Attempt 4 clippy, with `-D warnings`, rejected those same sites when the callee takes `&mut PgConnection` (`explicit_auto_deref`). `operator_kind`, `set_local`, `ensure_current_generation`, and `resolve_one` therefore take the transaction value and rely on deref coercion. The two Executor calls stay double-dereferenced. Clippy on the green run exited 0.

## F3. A bound `begin` Result holds the connection

Attempts 1 and 2 were `E0499`. A helper that borrows the connection and the transaction together cannot roll back and then probe. Binding `let started = conn.begin().await` keeps that borrow until `started` drops at the end of the function. The call is `conn.begin().await.map_err(...)?`. A failed begin reports `FreshConnection` and does not probe. Rollback uses `abort_with!`, which consumes the transaction before `probe`.

## F4. The live test must bind the request before the mutable connection

Attempt 3 compiled the library and failed the live test with `E0502`: `self.identity(...)` while `&mut self.target` was live. `maintain` and `expect_open_transaction` build the request first. The green live test exited 0.

## F5. Clippy is not on the cargo volume

`rustup component add clippy` writes into the container's writable layer. Attempt 3 exited 1 because the component was absent. `inside.sh` installs the 1.94.0 clippy component before `cargo clippy`. The green component exit is 0.

## F6. The measured estimate matched the row count and is still an estimate

After `ANALYZE`, `reltuples_after_bits` is `1073741824`, which is `2.0f32`. `estimated_rows` is 2 and `SELECT count(*)` is 2. The receipt stores `estimate_is_exact=false` and `published=false`. Import state stayed `awaiting_publish`. The unit test `reltuples_rounding_is_not_an_exact_claim` passed. The bits are the planner estimate, not a cardinality proof.

## F7. The policy catalog proof is an evidence harness

`overlay/catalog_proof.rs` was copied to `crates/oo-gateway/tests/target_statistics_catalog_proof.rs` inside the fixed archive. `oo-gateway` source in the product commit is untouched. The ignored test loaded the existing policy catalog with `UnknownRows::Refuse` and the query returned count 2. `statistics_published=false`. Libclang came from the a200 extract script with a219 paths. Its exit is 0. The fact files contain no password text (`secret-scan.exit` 0).

## F8. The live timeout is the lock timeout

The fixture holds `ACCESS EXCLUSIVE` on the shadow and sets `lock_timeout_ms` to 400. The call returns `LockTimeout` and disposition reusable, and the following read shows the known estimate is still present. `57014` maps to `StatementTimeout` in the library. This run did not provoke a statement timeout.

## F9. Source writes stayed at the import baseline

The source audit count before import was 2, from the two inserted rows. After import, refusals, `ANALYZE`, the lock, and the forged-relation swap, the audit count was still 2, the reader count was 0, and source `app.party.reltuples` was still `-1`. The maintenance call was given the target connection only.

## F10. Scope that this commit does not close

`full_import` and `import_cli` are unchanged. There is no new CLI and no dependency edit. A caller that wants known statistics calls `analyze_mapped_shadows`. Import still leaves `reltuples` at `-1` until that call. The plan card M1-D06A is not marked accepted here. M1-I01, M1-I02, and any parent goal stay open.
