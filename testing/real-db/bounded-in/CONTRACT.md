# Bounded typed literal IN predicate

Status: frozen before the guard change. This file is not a new development unit and does not accept a parent goal, M1-I01, M1-I02, or SP06-W02.

Session `eef831aa-dba4-4af2-a78b-541bc9e0a343`. Fixed product archive `21cc96241a841b47dbbcb116549030e38a643ba0`. That commit is an ancestor of the worktree. Product bytes under test are `git archive` of that commit plus the owned overlay below. Dirty files outside the overlay are not mounted.

Owned paths:

- `crates/oo-gateway/src/sql_guard.rs`
- `crates/oo-gateway/tests/bounded_in.rs`
- `testing/real-db/bounded-in/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a343/SP06/bounded-in/**`

No other path is edited. `query_executor`, policy, the root manifest, and the root lock stay untouched. No new dependency. No `cargo update`. No `cargo generate`. a341 owns `pack_catalog` and its tests. a313 remains the root manifest and lock writer.

The a317 fixture, gold, and decisions under `testing/real-db/pg-e2e` stay read-only. `wide-qty` stays `SELECT entry_id, qty FROM shop.entry WHERE entry_id IN (0, 1, 2) ORDER BY entry_id` with expected exit 0. This line does not rewrite that statement into `OR`, and it does not rerun a317.

## What the guard admits

`guard_select` may admit one `AEXPR_IN` node when every condition below holds. The node is left as `IN` or `NOT IN`. It is not rewritten to `=`, `<>`, `OR`, or `AND`.

The operator token on the node is exactly one unqualified string: `=` for `IN`, or `<>` for `NOT IN`. Any other token, including a schema-qualified operator, is refused.

The left side is one in-scope scalar column reference. The existing column resolver authorizes it. A hidden, missing, or ambiguous column is `COLUMN_NOT_ALLOWED` and the requested name is omitted. The column reference is rewritten to the approved physical name, the same way a comparison rewrites it. The select-list output alias stays the logical column name. A row constructor, function, cast, parameter, subquery, or any other left side is refused.

The right side is one explicit list of scalar literals. The list length is at least 1 and at most `sql_guard::IN_LIST_LIMIT` (32). An empty list and a longer list are refused. Each element is an integer, text, or boolean `A_Const`, or a SQL `NULL`. A negative integer constant is an integer literal: the parser stores the sign on the constant, and `IN (-1)` keeps `-1`. A column, function, cast, parameter, subquery, array constructor, `ANY` / `ALL`, or unary operator expression in the list is refused. A column named only inside the list does not become visible.

A non-null literal is typed by the snapshot `LiteralTypes` entry for that kind. The pair `(column type, literal type)` must match one already-resolved snapshot operator whose token is `=` or `<>` as above. Integer literals stay on the integer literal OID. An `int8` column therefore needs the existing cross-type operator against that integer OID, the same operator `entry_id = 0` already needs. No cast is inserted. A missing or ambiguous operator is `FUNCTION_NOT_ALLOWED`.

`NULL` is kept in the list. It has no literal OID. Authorization uses the same-type operator `(column type, column type)` for the same token. The `NULL` node is not deleted and is not rewritten to `IS NULL`.

Each authorized element records one operator binding. The executed SQL is still the deparse of the `IN` node.

## NULL and NOT IN

PostgreSQL defines `x IN (a, b, ...)` as `x = a OR x = b OR ...`, and `x NOT IN (a, b, ...)` as `x <> a AND x <> b AND ...`, with three-valued logic. This guard does not perform that rewrite. PostgreSQL evaluates the preserved predicate:

- A non-null left value equal to any non-null element makes `IN` true and `NOT IN` false.
- `NULL` on the left, or a `NULL` element when no non-null element compared equal, makes `IN` unknown.
- A `NULL` element makes `NOT IN` unknown for every left value that is not equal to a non-null element. `WHERE` drops unknown, so `NOT IN` that contains `NULL` returns no row.
- Removing `NULL` from `NOT IN`, or rewriting either form into `OR` / `AND` of comparisons, would change those results. Both are refused by this contract.

`IN (0, 1, 2)` keeps the integers 0, 1, and 2. Deparse may change whitespace and quoting. It may not change the values or replace `IN` with `OR`.

## What stays refused

These shapes fail closed. They do not fall through to execution:

- a subquery `IN` list, including `NOT IN (SELECT ...)`
- `= ANY`, `= ALL`, and an array constructor
- a user function or any other function in the `IN` node
- a cast, including the boolean literal cast, inside the `IN` node
- a parameter placeholder
- a hidden column, whether it is the left side or a list element
- a list longer than 32, a unary operator expression such as `-qty`, a float literal, and a bit string

The boolean literal cast remains admitted only where the previous slice already admitted it, outside this node.

Existing protections stay in force: the 64 KiB text budget, the node and depth budgets, parameter refusal, logical-to-physical rewrite, the operator and function snapshot, scope resolution, and R9. An admitted `IN` predicate is a real filter. It does not fold to true. `OR true` beside it still folds to true, and an unfiltered star of a large relation is still `COST_TOO_HIGH`.

## Positive fixture

One private PostgreSQL 17.10 and the compiled `query-pg` from the archive plus this overlay run the shop shape `SELECT entry_id, qty FROM shop.entry WHERE entry_id IN (0, 1, 2) ORDER BY entry_id`.

The proof chain is:

1. `guard_select` on the catalog loaded from that database admits the original SQL.
2. The executed SQL is the deparsed AST. It still contains `IN (0, 1, 2)` and does not contain `OR`. Logical names are rewritten to the approved physical names.
3. `query-pg` summary and audit hashes equal that plan's original and executed hashes.
4. The returned cells are entry `0` with the int8 minimum, entry `1` with the int8 maximum, and entry `2` with zero.
5. The same executed SQL, read by the administrator, returns those cells.
6. `IN (0, NULL, 2)` returns entries 0 and 2. `NOT IN (0, NULL)` returns no row. `NOT IN (0, 1)` returns the other stored entries. `entry_id = 0` still returns the minimum row.
7. Hidden `memo`, `INSERT`, a parameter, `ANY (ARRAY[...])`, a cast, and `abs(0)` still exit 6 with `query-pg: guard` and no audit file.

The fixture table holds four rows, not the a317 100000-row load. Passing it does not rerun a317 and does not accept the parent.

## Resources

One Rust builder, 6 GiB, 1 CPU. One private PostgreSQL 17.10, 1 GiB, 1 CPU. The global running rust-builder cap is 10. This task uses at most one builder, including install and inventory. Host `cargo`, `rustc`, `rustdoc`, and `rustup` defer the launch.

Images, already local, no pull:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

`linux/arm64`. Toolchain text must start with `rustc 1.94.0 `. `--cap-drop ALL`. PostgreSQL adds only `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, and `SETGID`. No privileged mode, no host network, no published port, no Docker socket, no host home, no shared cargo cache, no auth environment, no password on argv.

| Use | Value |
| --- | --- |
| Owner label | `a343` |
| Session label | `eef831aa-dba4-4af2-a78b-541bc9e0a343` |
| Network | `oo-a343-eef831aa-net` |
| Host and alias | `oo-a343-pg` |
| Volumes | `oo-a343-eef831aa-cargo`, `oo-a343-eef831aa-target`, `oo-a343-eef831aa-pgdata` |
| Private tmp | `/tmp/oo-a343-eef831aa-dba4-4af2-a78b-541bc9e0a343` |
| Database | `oo_a343` |
| Login | `oo_a343_login` |

Cleanup removes only objects whose `oo.owner` is `a343` and whose `oo.session` is this UUID. It does not prune and does not remove a foreign name.

## Pre-edit smoke

Before the guard change, one builder runs the archived `oo-gateway` `sql_guard` tests. `OO_A175_LIVE` stays unset. The smoke fetches the locked crates into this session's cargo volume, then tests run `--locked --offline`. Toolchain text starts with `rustc 1.94.0 `. A failed smoke stops the change.
