# a127 findings

These are observations from the owned PostgreSQL 17.10 fixture and the finite peer. Assertion updates follow the server terminator that the fixture returned. They do not drop the same-cut, exactly-once, verb, SQLSTATE 22012, or SQLSTATE 57014 checks.

## Protocol

Execute with a positive limit N that returns N rows sends `PortalSuspended` even when no further row exists. The next demanded Execute, which returns zero rows, sends `CommandComplete` with `consumed_rows` 0. A zero-row statement completes on the first Execute. Limit 0 runs to completion. The driver does not send that follow-up Execute by itself.

After `PortalSuspended` and before `ReadyForQuery`, `pg_stat_activity.state` is `active`. The live test asserts `active`. That state also shows the page loop did not send `Sync`.

`pg_export_snapshot()` returns three hexadecimal segments, for example `0000001C-00000002-1`. The live test accepts three non-empty ASCII hex segments and rejects any extra text. The id is used only after that check, inside a quoted `SET TRANSACTION SNAPSHOT`.

`SELECT (1/0)::int4` raises SQLSTATE 22012 at Parse because the server folds the constant. `SELECT (1 / $1)::int4` bound to 0 raises 22012 at Bind, during `open_named_portal`, before Execute. The passing live test took the open `Err` branch, required 22012, rolled back, and required the connection to be unpoisoned. Execute-time 22012 accounting remains in `execute_error_resyncs_aborted_transaction`, which passed.

`pg_cancel_backend` on `pg_sleep` yields 57014. The passing live test includes that path. The finite peer covers a held Execute whose dropped pull poisons the connection.

A concurrent writer, after the first suspended page, committed an update of id 3 and an insert of id 4. The reader continued on the original repeatable-read snapshot: note `three-original`, row 4 absent, follow-up count 3. The same snapshot id was applied by the oracle transaction.

## Compile repairs

Attempt 1 failed in `portal.rs` with E0106 (`prefix(self) -> &[u8]` missing a return lifetime) and E0308 (a flush `std::io::Error` returned where `sqlx_core::Error` was required). The repair is `prefix(&self)` and `Err(Error::Io(err))`. No `'static` lifetime was added and no row bytes were leaked to satisfy the borrow checker.

Later compile failures were E0277 (`expect_err` needs `Debug` on portal types that hold `PgRow`; tests use `must_err`), E0505 (the cancel future was block-scoped), and E0502 (the error branch checked `named_portal_poisoned` while the transaction still borrowed the connection; that check runs after rollback).

## Fixture repairs

Attempt 7 inserted id 4 after an earlier attempt had already committed that row (23505). The live test now asks the admin role, in autocommit, to delete `id >= 4` and restore the id 3 note before the reader begins. Those statements are classified as admin verbs.

The cumulative PostgreSQL log counted 85 reader statements because failed attempts share the log. `reader_forbidden=0`. The raw log was not committed.

## Launcher

One host invocation between attempt 9 and attempt 10 exited before `docker run`. An unquoted heredoc under `set -u` expanded a dollar sign in the prior-attempt cause. No attempt directory was created and no builder was left running. The passing invocation records `prior_attempt=10`.

## Left in place

Vendor `#[expect(dead_code)]` warnings in `message/flush.rs` and `message/mod.rs` are pre-existing. Clippy on `oo-connectors` still exited 0. Those files were outside the write scope.

Statement-id wrap, `Execute` body-size encoding width, and Bind result-format length stay as they were in the accepted driver. Portal ids use `checked_next` and refuse `u32::MAX`. The default executor's wrapping statement ids were not changed.

R2-P is unchecked until an independent review of `182702e6e67d16de10f280eaf422fce4e2f5a5f2`.
