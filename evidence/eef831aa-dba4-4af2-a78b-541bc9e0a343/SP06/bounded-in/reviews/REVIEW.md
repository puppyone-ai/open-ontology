# Review

Verdict: **APPROVE_LIMITED**.

Scope is the bounded literal `IN` predicate in `sql_guard` only. This is not acceptance of SP06-W02, M1-I01, M1-I02, the parent goal, or the a317 100000-row driver. It is not a new counted item.

The reviewer is this session. No subagent was used. The review was written after the green final run, against the source that the run compiled and the evidence that run wrote.

## What was checked

`admit_op` sends only `AEXPR_IN` to `admit_in`. The node stays `IN` or `NOT IN`. Nothing in the guard rewrites it to `OR` or `AND`. The deparsed form recorded by the failing assertion run, and then kept by the green tests, is `pt_entry.pc_entry IN (0, 1, 2)`.

The left side is one column, authorized by the existing column resolver and rewritten to the physical name. A hidden left side is `COLUMN_NOT_ALLOWED` and the name is omitted. A column that appears only in the list is refused as `IN predicate` and is not rewritten.

The right side is 1 to 32 integer, text, boolean, or `NULL` constants. Each element records one existing snapshot operator. Integer literals use the existing `(int8, int4)` operator. `NULL` uses the same-type operator and stays in the tree. No cast is inserted.

`IN (-1)` is a signed integer constant. A unary operator expression such as `-qty` is still refused. Subquery, `ANY`, function, cast, parameter, float, empty list, and 33 elements fail closed.

`WHERE` fold of an admitted `IN` is a real filter. `true OR entry_id IN (...)` still folds true. Join `ON` does not satisfy R9.

The final builder compiled archive `21cc96241a841b47dbbcb116549030e38a643ba0` plus `sql_guard.rs` and `tests/bounded_in.rs`. `query_executor.rs`, `policy.rs`, `policy_catalog.rs`, `lib.rs`, `query-pg.rs`, and `Cargo.lock` matched that archive. Rust 1.94.0. `sql_guard` 24 passed. `bounded_in` 11 passed. Clippy `-D warnings` on the library and `bounded_in` passed. The lock hash stayed `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`.

The private PostgreSQL fixture returned entry 0 with the int8 minimum, entry 1 with the int8 maximum, and entry 2 with zero. `IN (0, NULL, 2)` returned 0 and 2. `NOT IN (0, NULL)` returned no row. `NOT IN (0, 1)` returned 2 and 4. Seven guard refusals exited 6 with `query-pg: guard` and no audit file. Stored rows were unchanged. The administrator re-read of the executed SQL matched those cells.

## Limits

`contracts/gateway/sql-guard-v1.md` still describes `IN` as refused. That file was not an owned path. The slice contract is `testing/real-db/bounded-in/CONTRACT.md`.

The live table has four rows. a317 was not rerun. Passing this fixture does not accept `wide-qty` on the 100000-row load.

`IN_LIST_LIMIT` is 32. It is public on `sql_guard` and is not re-exported from the crate root, because `lib.rs` was not edited.

The select-list alias stays the logical column name. That is the existing rewrite, not a second grant of the column.
