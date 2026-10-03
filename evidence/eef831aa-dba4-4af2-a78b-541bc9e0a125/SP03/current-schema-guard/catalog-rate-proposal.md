# Minimal extension for pre-SQL row bounds

Status: proposal only. `crates/oo-connectors/src/catalog.rs`, `crates/oo-connectors/src/access.rs`, `crates/oo-connectors/src/types/read.rs`, and `crates/oo-connectors/src/ident.rs` stay unmodified in this session.

## Why authorized files cannot close the catalog bound

`PgSession::collect_catalog` calls `session::fetch_all` / `session::fetch_one` on these statements, in this order when the selection has columns and those columns have enum or domain types:

1. `sql::KEYWORDS` (`fetch_all`)
2. `sql::TABLES` (`fetch_all`)
3. `sql::COLUMNS` (`fetch_all`)
4. `sql::ENUM_LABELS` (`fetch_all`, skipped when the collected type id list is empty)
5. `sql::DOMAIN_CHECKS` (`fetch_all`, skipped when that list is empty)
6. `sql::PRIMARY_KEYS` (`fetch_all`)
7. `sql::UNIQUE_KEYS` (`fetch_all`)
8. `sql::FOREIGN_KEYS` (`fetch_all`)
9. `sql::POLICIES` (`fetch_all`)
10. `sql::ACL` (`fetch_all`)
11. `sql::CAPTURED_AT` (`fetch_one`)
12. `sql::FACTS` (`fetch_one`)

`sqlx_core::Query::statement` is `pub(crate)`, so `oo-connectors` cannot read the SQL back out of the `Query` value to discover a `LIMIT`. A `VecDeque` of per-call bounds armed from `snapshot.rs` is wrong: the enum and domain calls are skipped when `type_ids` is empty, and the next bound would be applied to the wrong statement.

A static `LIMIT` literal appended in `sql.rs` would compile without a `catalog.rs` edit, and it would cap the bytes the server sends. It would also let a truncated result look complete. `reserved_covers_server` accepts every returned word that is already in the embedded set; a cut keyword list that happens to be a subset would pass. Column, key, policy, and ACL collectors treat the returned rows as the full set. That is a silent catalog miss, so this session does not add those literals.

`session.rs` can charge `min(budget.max_rows(), 50_000)` before an unarmed `fetch_all`. That stops a later counter from moving after a large response, and `within_rows` still rejects `len > max_rows`. It does not stop the server from sending more rows than the charge. That is the overshoot the receipt rejects. This session does not treat the unarmed charge as compliance for `collect_catalog`.

`access.rs` reaches `session::fetch_all` / `fetch_one` for `FACTS`, `ROLE`, `ASSUMABLE`, `MEMBERSHIP_TRUNCATED`, `DIRECT_MEMBERS`, `ROLE_ATTRS`, `POLICY_COUNTS`, `PREFLIGHT`, `TABLES`, `ROLE_VISIBLE`, `SCAN_COUNTS`, `DATABASE_CREATE`, `SCHEMA_CREATE`, `GLOBAL_TABLE_WRITE`, `GLOBAL_COLUMN_WRITE`, `DEFINER_REACH`, `PUBLICATIONS`, `PUBLICATION_TABLES`, `SLOTS`, and `QUOTE_IDENT`. The same bound gap applies to each unbounded statement. `types/read.rs` uses `session::fetch_all` for `TYPE_FACTS` and `ENUM_LABELS`, and one private `fetch_all` on a caller-built statement (around the `AssertSqlSafe` call). That private fetch would not observe a charge placed only inside `session::fetch_all`. The owner open path calls `collect_catalog`. The live regression calls `inspect_access` and then `catalog`.

## Exact change, when a later card makes the paths writable

In `catalog.rs`, immediately before each fetch, charge the same integer the statement uses as its `LIMIT`, then bind that integer. Fail closed when the returned length equals the limit (the result may be truncated). Do not charge after `fetch_all` returns.

| Statement | Limit | Charge | Full-limit result |
| --- | --- | --- | --- |
| `KEYWORDS` | `PG17_RESERVED.len() + 1` (79 + 1 = 80 on this tree) | that limit | `catalog("keyword_list")` |
| `TABLES` | `selection.tables().len()` | that limit | existing missing-table / shape failure |
| `COLUMNS` | `min(max_rows, oids.len().saturating_mul(1600))` | that limit | `catalog("column_list")` |
| `PRIMARY_KEYS` | `oids.len().saturating_mul(32)` | that limit | `catalog("primary_key")` |
| `UNIQUE_KEYS`, `FOREIGN_KEYS`, `POLICIES`, `ACL`, `ENUM_LABELS`, `DOMAIN_CHECKS` | remaining tokens in the current 1s window, at most 50_000, and at most `max_rows` | that same number, as the SQL `LIMIT` | `budget("max_rows_per_sec")` |
| `CAPTURED_AT`, `FACTS` | 1 | 1 | `catalog` fact already used for a bad row |

1600 is PostgreSQL `MaxHeapAttributeNumber`. 32 is `INDEX_MAX_KEYS`. 79 is the length of `PG17_RESERVED` in `ident.rs` on this tree. 50_000 and the 1s window are the fixed ledger constants, not `PgBudget` fields.

`sql.rs` gains one trailing bind on each of those statements (`LIMIT $n` with `n` the next free parameter). `catalog.rs` binds it. No new crate, no `PgBudget` knob, no cap above 4, no fifth socket.

Apply the same charge-then-`LIMIT` pair in `access.rs` for every unbounded `fetch_all`: `ASSUMABLE`, `DIRECT_MEMBERS`, `ROLE_ATTRS`, `DATABASE_CREATE`, `SCHEMA_CREATE`, `GLOBAL_TABLE_WRITE`, `GLOBAL_COLUMN_WRITE`, `DEFINER_REACH`, `PUBLICATIONS`, `PUBLICATION_TABLES`, and `SLOTS`. Charge 1 before each `fetch_one` (`FACTS`, `ROLE`, `MEMBERSHIP_TRUNCATED`, `ROLE_VISIBLE`, `SCAN_COUNTS`, `QUOTE_IDENT`). Charge the bound-array length before `PREFLIGHT` and the selected-oid length before `POLICY_COUNTS` and the access-path `TABLES` call. `types/read.rs` must charge before its `TYPE_FACTS` and `ENUM_LABELS` calls, and the private `fetch_all` must charge its existing `limit` argument before the round trip.

Until that extension lands, owner and guard statements that this session controls (startup identity, version, session guards, probe `LIMIT 2`, and the single current-catalog statement) charge their own bound before the round trip and put the same bound in the SQL.
