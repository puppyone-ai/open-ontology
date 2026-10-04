# Gateway SQL admission

Contract `oo.gateway.sql-guard/v1`. Library `oo_gateway::sql_guard`.

This slice admits one top-level `SELECT`. The caller supplies the project, role, and policy identity, the visible relations and columns, the approved logical-to-physical mapping, and function and operator OIDs that a future adapter has already resolved. `guard_select` parses with `pg_query` 6.2.1 (libpg_query, embedded PostgreSQL 17.7, `PG_VERSION_NUM` 170007), rewrites that AST, and deparses. It does not connect to PostgreSQL, authenticate, resolve OIDs from names, execute SQL, or write an audit record.

The column ACL contract remains the grant primitive. It does not link a parser. This contract is the admission slice, and it does link `pg_query`.

## Pin

`pg_query` is `=6.2.1`, the crates.io primary release of the 6.2 series dated 2026-09-30. The crate checksum is `da43168013d137b3ca49b0044c1108ec09ff596a0626d4db89209c0a1d450cd2`. The Rust wrapper's manifest license is MIT. Vendored PostgreSQL sources are under the PostgreSQL license, and the rest of libpg_query is BSD-3-Clause. Product code and this crate's tests are safe Rust (`forbid(unsafe_code)`). The FFI stays inside `pg_query`.

A plan is refused when `protobuf.version` is not 170007. Parser warnings are discarded. A parse failure returns `SQL_PARSE_ERROR` and drops the parser text. This slice does not attach a sqlparser dialect hint.

## Call

`guard_select(&PolicyIdentity, &PolicyCatalog, sql) -> Result<GuardedPlan, GuardError>`.

The identity argument and `PolicyCatalog.identity` must be equal byte for byte. Each of project, role, and policy id is non-empty, at most 128 bytes, and free of NUL and control characters. A mismatch is `SCHEMA_INVALID` ("policy identity does not match"). A malformed snapshot is `SCHEMA_INVALID` with a static reason and no caller text.

A usable relation has a closed schema and name, at least one visible column, closed physical names, and non-zero column type OIDs. Closed identifiers match the column ACL rule: at most 63 bytes, a leading lowercase letter or underscore, then lowercase letters, digits, and underscores. Duplicate logical or physical relations, and duplicate column names inside one relation, are refused. An empty relation list is valid. A fully hidden table is omitted from the snapshot, so it fails the same way as a missing table.

Function entries match on schema, name, argument type OIDs, and the aggregate-star flag. Operator entries match on schema, name, and both operand type OIDs. OIDs must be non-zero. `aggregate_star` requires an empty argument list. `pg_catalog` is allowed for functions and operators. `information_schema`, `_oo`, and names ending in `__stage` are not. A unique name without a matching type signature is not a match. The guard does not invent an OID.

`LiteralTypes` supplies integer, text, and boolean OIDs for literals. `None` leaves that literal unresolved. No PostgreSQL type OID is hard-coded in the guard. `null_test_allowed` gates `IS NULL` and `IS NOT NULL`. `large_table_row_threshold` is the R9 cutoff. `estimated_rows` and `pending_unclassified` are adapter fields. The guard does not read `pg_class`.

## Plan

`GuardedPlan` returns the identity, the original SQL, the executed SQL, the SHA-256 of each exact UTF-8 string (lowercase hex), the `pg_query` fingerprint of the executed SQL, parser version 170007, star expansions, and recognized bindings.

The fingerprint is a clustering key. It is not proof that two statements mean the same thing.

Executed SQL is the deparse of the rewritten AST. The original text is never returned as executed SQL. After deparse, the guard parses the result again and deparses that tree. The second deparse must equal the executed SQL. It then runs the same admission on a derived catalog whose logical names are the approved physical names, and requires the same executed SQL and an empty star list. Any failure in that chain is `STATEMENT_NOT_ALLOWED` with a static serialize reason (`deparse`, `reparse`, `version`, `idempotent`, `fingerprint`, or `recheck`). The recheck error and the recheck SQL are not returned. Bindings on the plan are from the original statement.

Star expansions record the qualifier, the logical schema and relation, and the logical columns in catalog order. Bindings record relations (alias and physical names), select-list items, functions, operators, and the count of admitted null tests. A select-list item is a column, a function, a literal (`integer`, `text`, `boolean`, or `null`), or a compound expression.

## Supported statement

Exactly one top-level `SelectStmt`, with no `WITH`, set operation, `INTO`, locking clause, `DISTINCT`, `DISTINCT ON`, `GROUP BY`, `HAVING`, `WINDOW`, or `VALUES`.

`FROM` may be empty (a constant or function target list with no column reference). A from item is a schema-qualified base relation or a `JOIN` of those relations: comma form, `CROSS`, `INNER`, `LEFT`, `RIGHT`, and `FULL`, with an optional `ON` expression. `NATURAL`, `USING`, a join alias, `LATERAL`, a subquery, a range function, and `TABLESAMPLE` are refused.

A relation alias replaces the table name in qualifiers. `schema.relation` qualifies a range only when that range has no alias. Alias column lists are refused. The same exposed name cannot be bound twice. `ONLY` is refused. A catalog qualifier is refused.

The select list may contain column references (unqualified, `qualifier.column`, or `schema.relation.column` when the range has no alias), `*`, `qualifier.*`, `schema.relation.*`, integer, text, boolean, and null literals, `count(*)`-style calls, and other function calls whose argument types resolve and match one snapshot entry. A comparison is a binary `=`-style operator whose operand types match one snapshot operator. `AND`, `OR`, and `NOT` are admitted in expressions. `IS NULL` and `IS NOT NULL` are admitted on a non-row column when the snapshot allows null tests. The boolean literal cast of an `A_Const` string in `{t, f, true, false, 1, 0, yes, no, on, off}` to `bool` or `pg_catalog.bool` is admitted. `ORDER BY` accepts a column reference. `LIMIT` and `OFFSET` accept a non-negative integer constant.

`WHERE true` folds to true. `WHERE false` is a real filter. `WHERE 1 = 1` is not proved to be a tautology and counts as a filter. `LIMIT` and `JOIN ON` do not count as the R9 filter.

## Rules

| Rule | This slice |
|---|---|
| R1 | One top-level `SelectStmt`. `WITH` is refused, so a read CTE is not admitted. `EXPLAIN`, `SHOW`, `SET`, `COPY`, `VACUUM`, and a second statement are `STATEMENT_NOT_ALLOWED`. |
| R2 | The walker visits every protobuf node, including CTE, subquery, and set-operation children. `InsertStmt`, `UpdateStmt`, `DeleteStmt`, and `MergeStmt` anywhere in that tree are `write statement is not allowed`, including `WITH x AS (DELETE ...) SELECT ...` and `EXPLAIN INSERT`. Those shapes are not otherwise admitted. |
| R3 | `SELECT INTO` and `FOR UPDATE` / `SHARE` / `NO KEY UPDATE` / `KEY SHARE` are refused. |
| R4 | A function or operator is admitted only when one snapshot entry matches the schema (if written), the name, and the type signature. Casts other than the boolean literal form above are refused. No OID is derived from a name. |
| R5 | Relation schemas `pg_catalog`, `information_schema`, `_oo`, and `*__stage` are `system schema is not allowed` on user SQL. The name is not echoed. Approved physical relations may live in `_oo`; the recheck uses that surface only for names the mapping already produced. CTEs are refused, so a CTE name cannot qualify a column. |
| R6 | A base relation must be schema-qualified (`UNQUALIFIED_RELATION`). When exactly one visible relation has that relname, the hint is `use` plus that relation's `schema.name`. Otherwise the hint is the literal `use schema.name`. A hidden relation is not suggested, and the requested name is not echoed. |
| R7 | A relation that is missing or absent from the snapshot is `RELATION_NOT_FOUND` ("relation is not visible") in both cases. A column that is missing, hidden, or ambiguous is `COLUMN_NOT_ALLOWED`. The requested name is omitted. The message lists visible `alias.column` or `schema.relation.column` labels and, when the count is non-zero, `N columns are pending classification`. |
| R7a | A projection star expands to the visible columns of the matched range, in catalog order. A bare `*` walks `FROM` order. `count(*)` is not rewritten. `JOIN USING`, `NATURAL`, subquery, and CTE stars are refused rather than expanded. The expanded tree is deparsed, reparsed, and admitted again. |
| R8 | SQL longer than 64 KiB, or a decoded tree with more than 10,000 protobuf `Node` values or `Node` depth greater than 128, is `STATEMENT_NOT_ALLOWED` ("statement exceeds the admission budget"). The byte check runs before parse. The walk aborts at the node or depth budget. Depth counts the statement node as 1. `pg_query` 6.2.1 decodes protobuf with prost's default recursion limit, which rejects a nest of 48 `NOT` operators as a parse error before a decoded tree can reach depth 128. That refusal is `SQL_PARSE_ERROR`, not a returned plan. A nest of 40 `NOT` operators still decodes and is admitted. The 500 ms parse limit and the restricted worker process are not implemented. |
| R9 | An original projection star that covers a relation whose `estimated_rows` is greater than `large_table_row_threshold`, with no `WHERE` or with a `WHERE` that folds to true, is `COST_TOO_HIGH` (HTTP 422). The message names the visible `schema.name` and does not include the row estimate. The hint is `add a filter or use export`. The check uses the original star set, so expansion cannot bypass it. A non-star projection of a large table is allowed. `SELECT small.*` from a join is allowed when `small` is at or under the threshold. `SELECT *` that also covers a large relation is refused. |

`GuardError::code` maps to `oo_core::api::ErrorCode`. HTTP status follows that type: 400 for these codes, 422 for `COST_TOO_HIGH`. Hints are returned for an unqualified relation and for `COST_TOO_HIGH` only.

## What stays open

SP06-W02 stays open. This slice does not close the parent work item.

Not implemented, and refused rather than guessed when one of these appears in SQL:

- parser worker isolation and the 500 ms parse kill
- authentication, token exchange, HTTP, portals, `CancelToken`, and the pool
- SQL execution, `EXPLAIN` cost, statement timeout, and row truncation
- row-level security and claims binding
- the audit WAL
- sqlparser dialect hints
- parameter placeholders
- casts other than the boolean literal form, and implicit casts
- set-returning functions, window functions, `GROUP BY`, `DISTINCT`, set operations
- `WITH` / CTE scope, subqueries, `LATERAL`
- `NATURAL` and `JOIN USING`, including their merged columns
- `LIKE`, `ILIKE`, `SIMILAR`, `IN`, `BETWEEN`, `ANY`, and `ALL`
- `ORDER BY` an output alias or an ordinal
- float and bit-string literals
- volatile-function policy beyond "the OID must already be in the snapshot"
- database `GRANT` enforcement (the column ACL primitive remains a separate call)

`count(*)` can be admitted when the snapshot has that aggregate OID. A column-level `GRANT` without table `SELECT` may still reject `count(*)` at execution (`42501`). Admission is not that privilege check.

Publication of a read API in front of this function is open. No release is claimed here.
