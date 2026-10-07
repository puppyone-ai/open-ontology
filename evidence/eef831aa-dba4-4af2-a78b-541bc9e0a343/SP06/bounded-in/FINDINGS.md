# FINDINGS

Verdict: **APPROVE_LIMITED** for the bounded literal `IN` slice.

1. `AEXPR_IN` is admitted only for one visible scalar column and a bounded literal list. The AST kind is not rewritten to `OR` or `AND`. PostgreSQL evaluates the preserved predicate, including `NOT IN` that contains `NULL`.

2. Integer literals keep the snapshot integer OID. An `int8` column uses the existing cross-type `=` or `<>` operator. `NULL` is authorized with the same-type operator and is not deleted. No cast is inserted.

3. Hidden columns are not granted by the list. A hidden left side stays `COLUMN_NOT_ALLOWED` with the name omitted. Subquery, `ANY` / `ALL`, functions, casts, parameters, floats, and lists outside 1..=32 fail closed.

4. An admitted `IN` is a real filter for R9. `OR true` beside it still folds true. Existing budgets, parameter refusal, and logical-to-physical rewrite still apply. The output alias remains the logical column name.

5. A signed integer constant such as `-1` is an integer literal in this parser. A unary operator expression such as `-qty` is refused. The slice contract records that distinction. The pre-code draft had called every leading minus a unary operator.

6. `contracts/gateway/sql-guard-v1.md` still says `IN` is refused. It was not edited. `testing/real-db/bounded-in/CONTRACT.md` is the freeze for this slice.

7. The positive fixture is four rows on one private PostgreSQL 17.10, executed by the compiled `query-pg`. It does not replace the a317 100000-row run and does not accept that run.

8. `Cargo.lock`, `query_executor`, policy, `lib.rs`, and `query-pg` were the archive bytes. No new dependency. No `cargo update`.
