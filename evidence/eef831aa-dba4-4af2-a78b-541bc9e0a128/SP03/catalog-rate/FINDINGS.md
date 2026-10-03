# FINDINGS — bounded catalog reads

Session `eef831aa-dba4-4af2-a78b-541bc9e0a128`. Commit `ccd93e1adc34b24ab998d90972549f8cffb59f0d`.

## F1. Fresh snapshot opens no longer exhaust the one-second allowance

a125 `snapshot_live` exit 101 remains the pre-edit record: fresh open panicked with `max_rows_per_sec` at `snapshot_live.rs:654` after `same_cut` on `snap_src` with `max_rows=1000`. This session did not rerun that binary on the pre-edit source.

On this commit, unmodified `snapshot_live` completed `same_cut`, `schema_and_privilege`, and its SQL audit. `verify/out/snapshot_live.exit` is 0. The test file was not edited. There is no sleep and no raised 50000 cap.

## F2. A 50000-row catalog of a tiny selection fits, repeatedly

`catalog_rate_live` opened source `rate_big` with `max_rows=50000` and ran `catalog` four times on `snap.parent` and `snap.child`. Elapsed time from before connect through the fourth catalog was 186.976333ms, inside one window. One success shows the first metadata read is no longer charged as 50000 rows. Four successes show unused reserved allowance was returned: the unsettled ceilings for two tables, including the enum, unique, and foreign-key sentinels, sum above 50000.

The same process then ran three `max_rows=1000` catalogs and `inspect_access` on source `rate_ord`.

## F3. A 32-column primary key is complete, and 128 policies are not

Source `rate_pk` catalogs `rate.wide`. The test requires 32 columns and 32 primary-key attributes. Source `rate_cut` catalogs `rate.pol` with 128 policies and requires `catalog` / `catalog_truncated`. Both assertions passed inside the same 1.40s test.

## F4. A timed-out reserved read denies the next query before keywords

A 60000-row result tripped the patched sqlx buffer (`incoming view shrank during advance`) in attempt 3. That payload is not the denial method.

The passing run holds `ACCESS EXCLUSIVE` on `rate.bulk`, then `read_typed` with `max_rows=50000` and a 400ms statement timeout. Catalog metadata finishes, the data `SELECT` reserves the remaining allowance, and the server returns `statement_timeout`. The reservation is kept. The next connect on `rate_deny` returns `max_rows_per_sec`. Wall time for that burn and denial was 648.401458ms. The server log after `a128_deny_cut` does not contain `pg_get_keywords`.

## F5. Reader SQL stayed free of source DML, DDL, and COMMIT

The passing `catalog_rate_live` log had 179 reader statements and 0 reader write words. Admin writes were 8. `snapshot_live` on that same log reported 843 `oo-pg-source` statements and 3 writer writes. Secret hits were 0.

## F6. Failed attempts kept

| Attempt | Exit | What it shows |
| --- | --- | --- |
| smoke | 0 | Pre-edit lib and clippy. No live fixture |
| `verify/attempt-1` | 1 | `RowHold` has no `Debug`, so `unwrap_err` did not compile. Admin SQL was `&String`. `snapshot_live` exit 0. Clippy component missing, exit 1 |
| `verify/attempt-2` | 101 | Lib 55 passed. `sqlx::query` rejected `&&str`. `snapshot_live` exit 0 |
| `verify/attempt-3` | 101 | Live read of `rate.bulk` panicked in sqlx `buffered.rs:590`. Clippy `redundant_static_lifetimes`. `snapshot_live` exit 0 |
| `verify/attempt-4` | 101 | `&mut *admin` on an owned `PgConnection`. `snapshot_live` exit 0 |
| `verify/out` | 0 | Lib, `catalog_rate_live`, `snapshot_live`, clippy |

a125 reports, the a125 exit 101 log, and the a125 volumes were not rewritten or removed.
