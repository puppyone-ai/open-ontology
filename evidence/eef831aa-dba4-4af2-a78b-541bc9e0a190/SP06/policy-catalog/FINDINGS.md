# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a190`. Source `78a60fdeabcada0d024fc6f9a81e0e6b184260e8`. Archive `3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6`.

## Loader

The public path is `oo_gateway::policy::catalog`. Callers that already hold a trusted TLS administrator client can build a `sql_guard::PolicyCatalog` without inventing OIDs. The loader does not authenticate that client, does not take a password or a DSN, and does not read a claims map. Published classification only restricts the column list. Visibility is the intersection of that list with `has_column_privilege(..., 'SELECT')`.

Identity text, project, role, config digest, generation, and login are checked before the first catalog query. Empty input, mixed policies, and a mismatched identity return before a snapshot. A stale generation or digest fails the `max(run_generation)` check for that source and digest. A missing role fails the `pg_roles` lookup. Ambiguous mapping rows fail the load. No substitute OID is written when a required object is absent.

One transaction is `REPEATABLE READ` and `READ ONLY`. `search_path` is set to `pg_catalog` and `statement_timeout` to 30 seconds, then both are read back. Catalog statements are parameterized. The only interpolated SQL text is the fixed `FROM` clause for the operator query and the `count(*)` query. Logical names travel as parameters. After commit, `pg_current_xact_id_if_assigned()` must be null. A rollback that itself fails becomes `TransactionIncomplete`. `OO024` from lookup is `MappingAbsent`. Other database errors expose a five-character SQLSTATE or the static text `postgres error`.

`estimated_rows` comes from `pg_class.reltuples`. The value `-1` is unknown and follows `UnknownRows::{Refuse, Zero, Saturate}`. There is no default. Other negative, NaN, and infinite values are refused. A finite value of at least zero is rounded half away from zero.

## Whitelist that actually loaded

PostgreSQL 17.10 in this container returned 54 comparison operators for the nine signatures in the contract, all in `pg_catalog`, immutable, internal, and not extension members. `count(*)` was OID 2803. The same server has one argument-taking count, `count(any)`, OID 2147. There is no `count(int4)` row. The loaded catalog contains 2803 and does not contain 2147. `pg_sleep` and int4 `+` are present in `pg_catalog` and absent from the loaded catalog. `SELECT fixture.touch()` was refused by the guard after a user function was created.

Integer literals are `int4`, so the subset includes the int2/int4 and int8/int4 pairs in both directions. Same-type int2 and int8 pairs are also present. Arithmetic, pattern operators, and the rest of the R4 list are not resolved.

## Live fixture

The fixture is generation 11, digest `a190` repeated, namespace `billing`, table `invoice`, roles `oo_r_clerk` and `oo_r_auditor`. Columns in `attnum` order are qty int2, amount int4, seq int8, memo text, paid bool, and secret text. The shadow also has `_oo_pos numeric`. Clerk's policy lists columns in a different order; the catalog returns attnum order. Auditor's catalog contains amount only.

Before `ANALYZE`, `reltuples` was exactly `-1` (`to_bits` matched). Refuse, Zero, and Saturate produced unknown, 0, and `u64::MAX`. After `ANALYZE`, both roles saw `estimated_rows` 3. Revoking `SELECT` on paid dropped it from the clerk catalog and raised pending from 2 to 3. Adding column `extra` raised pending to 3 and left the name out of the catalog. Changing published memo to domain `public.a190_memo` refused the load with `UnsupportedType`. The error text did not contain `memo`.

Clerk `SELECT *` matched an administrator query of the physical columns, ordered by amount: the three inserted rows. `count(*)` where paid is true returned 1. Equality filters for amount 10, qty 2, seq 100, memo `alpha`, paid false, and `memo IS NULL` returned the expected cells. Auditor `SELECT amount` returned -4, 10, and 20. Auditor `SELECT qty` was `ColumnNotAllowed` and the display text did not contain `qty`.

`pg_catalog.pg_sleep(0)`, `count(amount)`, and `amount + 1` were `FunctionNotAllowed`. `INSERT` was `Guard(Write)` before a role was bound. Direct `SET ROLE oo_r_clerk` statements against secret, an insert, and `extra` returned SQLSTATE `42501`.

`pg_stat_user_tables` seq_scan, idx_scan, and tuple insert/update/delete deltas were 0 across each loader window. `_oo.physical_mappings` stayed at 16 rows: two generations, one namespace, one table, and six columns.

## Limits of this proof

The pre-edit smoke compiled the archive without these files. The focused run is the proof of the loader.

Clippy covered `oo-gateway` lib and `policy_catalog_pg_live` only. `policy_pg_live` and `query_executor_pg_live` were not run. No full security campaign ran.

The live grantor was `oo_fixture_admin`. A non-superuser grantor was not exercised. The executor under test is the archive executor, which binds the role after `guard_select`. The test asserts `bound_role()` is empty after a guard refusal on a fresh session.

The post-commit xid check is true for this read-only snapshot because the snapshot does not assign a transaction id. The explicit commit is the completion step. The xid check rejects a session that still holds an assigned id.

Display text and the oracle were checked for SQL fragments, hidden names, physical names, and the fixture passwords. That check is the test's assertions plus a byte scan of this evidence tree. It is not a general log-redaction audit.
