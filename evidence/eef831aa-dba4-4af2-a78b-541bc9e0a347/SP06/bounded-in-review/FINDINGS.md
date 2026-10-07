# FINDINGS

Verdict: **APPROVE_LIMITED** for bounded literal `IN` and `NOT IN` on commit `6f87942f1c66ba1a9787b7d7194e58a507af63ef`.

1. `AEXPR_IN` is admitted for one visible scalar column and 1 to 32 integer, text, boolean, or `NULL` literals. The deparsed predicate stays `IN` or `NOT IN`. The live statement for `entry_id IN (0, 1, 2)` contains that list and contains no `OR`, no `AND`, and no cast.

2. An `int8` column and an integer literal resolve the snapshot operator `(int8, int4)` with token `=` or `<>`. `NULL` stays in the AST and uses the same-type operator. The live `NOT IN (0, NULL)` result was empty, which is PostgreSQL's evaluation of the preserved predicate. No cast is inserted to force a type.

3. `IN (-1)` is an integer constant and uses the cross-type `=` operator. `IN (-qty)` is refused as `unary operator`.

4. A missing cross-type operator refuses an integer list. A missing same-type operator still admits an integer list and refuses `IN (NULL)`. An unresolved integer literal type refuses the list. A text literal against an `int8` column is `FunctionNotAllowed`.

5. Thirty-two literals are admitted. Thirty-three literals are `IN predicate`. An empty `IN ()` is refused before execution. A hidden left column is `ColumnNotAllowed`, and the display omits the requested name. A column used as a list element is `IN predicate` and does not echo that name.

6. `IN (SELECT ...)` is parsed as a SubLink and refused as `subquery`. A cast, a function call, a parameter, a float literal, `ANY`, and `EXISTS` fail closed with `cast`, `function`, `parameter`, `float literal`, `ANY predicate`, and `subquery`. `LIKE` and `BETWEEN` stay refused.

7. An admitted `IN` is a real R9 filter. A large starred `shop.entry` with that filter is expanded and admitted. `true OR` beside it folds true and the large star is `CostTooHigh` on `shop.entry`. `false OR` beside it stays a real filter and keeps both `IN` and `OR`. A `JOIN ON` list does not supply the WHERE filter: the unfiltered large star remains `CostTooHigh`. A non-star `true OR IN` keeps `IN`. Equality `entry_id = 0` stays equality and uses the same cross-type operator.

8. A statement longer than `SQL_BYTE_BUDGET` is `Budget` before parse. The output alias stays the logical column name. Relation and column names in the executed SQL are the physical mapping.

9. The private `query-pg` run returned int8 minimum, int8 maximum, and zero for `WHERE entry_id IN (0, 1, 2)`. The three operator OIDs matched the single loaded catalog operator for `(entry_id type, integer literal type, "=")`. Eight closed statements exited 6 with stderr `query-pg: guard` and no audit file. Stored rows were unchanged.

10. `Cargo.lock` and the mounted gateway sources other than the review harness were the `6f87942` bytes. No new dependency. No `cargo update`. No product write.

11. The a317 numeric-approved rerun on archive `21cc96241a841b47dbbcb116549030e38a643ba0` stopped at `query-wide-qty` exit 6, stderr `query-pg: guard`, because that older guard refused `AexprIn`. The frozen statement was `SELECT entry_id, qty FROM shop.entry WHERE entry_id IN (0, 1, 2) ORDER BY entry_id`. This limited approval makes a same-fixture rerun on `6f87942` possible. It leaves that integration unaccepted. a317 data, gold, and decisions were not modified.

12. The author packet under session `eef831aa-dba4-4af2-a78b-541bc9e0a343` is a separate self-review. This gold uses row `(7, 11)` and eight refusals, and it was produced by this session's compiled `query-pg`.
