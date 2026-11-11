# Gateway SQL admission

Admission is role-scoped, not a promise that PostgreSQL will execute a query
successfully. Use `oo describe` for visible columns and the runtime
`query_capabilities`, then `oo validate`. Functions and operators must exist
in that role's verified policy catalog; syntax support does not grant them.

## Comparisons and literal predicates

The comparison operators are `=`, `<>`, `<`, `<=`, `>` and `>=`.
Text/varchar columns can be compared with string literals. A bare string
beside a date or timestamp expression is admitted in these forms:

| Other operand | String literal |
| --- | --- |
| `date` | Valid `YYYY-MM-DD`, years 0001 through 9999 |
| `timestamp` | ISO date, or `YYYY-MM-DD HH:MM:SS[.ffffff]`; `T` also separates date/time |
| `timestamptz` | ISO timestamp with `Z`, `+HH`, `-HH`, `+HH:MM` or `-HH:MM` |

The guard rewrites that literal to an explicit `pg_catalog` type in the
executed AST. A date-only timestamp means midnight. Reversing the operands
does not change admission. The exact resulting comparator still needs its
verified catalog entry. Text *columns* are not implicitly cast to dates.
Invalid calendar dates, locale-dependent forms, named timezones, infinity,
relative dates and timezone-free timestamptz strings remain refused.

`BETWEEN` and `NOT BETWEEN` accept a visible column on the left and admitted
comparison operands as bounds. Both comparison signatures must be available.
The original predicate remains in the executed AST. Symmetric BETWEEN is
not admitted.

`IN` and `NOT IN` accept a visible column and at most 32 literal values.
Date/timestamp columns additionally accept matching explicit temporal
literals/casts of strings, or the ISO strings above. NULL elements remain
unchanged, including in `NOT IN`; they are never dropped or rewritten to
ordinary equality. Columns, functions and subqueries in the list remain
outside this literal-list grammar.

`LIKE`, `ILIKE` and their negations accept visible text/varchar columns and a
literal pattern, using verified builtin text operators. Patterns are bounded
to 1024 bytes and 32 unescaped wildcards.

```sql
SELECT order_id FROM app.orders
WHERE ordered_on BETWEEN '2026-01-01' AND DATE '2026-01-31';

SELECT order_id FROM app.orders
WHERE created_at >= '2026-01-01 00:00:00Z';

SELECT order_id FROM app.orders
WHERE ordered_on IN ('2026-01-01', DATE '2026-01-02', NULL);
```

## Scalar casts

`CAST` and `::` accept bool/boolean, int2/smallint, int4/integer, int8/bigint,
numeric, float8/double precision, text, varchar, date, timestamp, timestamptz
and interval. Arrays, domains, pseudo-types, schema-qualified user types and
type modifiers remain refused. Arguments are recursively checked, including
hidden columns and nested functions. String and NULL literals are supported.

Each new conversion needs its exact source/target grant from the live catalog.
The loader verifies builtin type input/output functions and pg_cast entries,
including volatility, language, return/argument types, extension ownership,
security-definer and custom-setting exclusions. String I/O fallback is granted
only when no declared cast overrides it. An absent cast grant is a refusal.
The parser subprocess transports these grants with bounded wire version 3.

Temporal string constants retain the ISO rules above, even with an explicit
cast. Timestamp/timestamptz conversions of columns may depend on the execution
session timezone; they do not assert source freshness or an atomic source cut.

## Verified scalar functions

The catalog additionally admits `extract(text,date|timestamp|interval)`,
`date_part(text,timestamp|interval)`, `lower(text)`, `upper(text)`,
`length(text)`, positional `substring(text,int4[,int4])`, `round(numeric,int4)`,
`round(numeric)` (rewritten to scale zero), `round(float8)` and `abs(numeric|int2|int4|int8|float8)`.
Text functions also accept binary-compatible varchar inputs. Regex substring
is not included. Every overload requires a unique verified builtin catalog
entry; allowing a name does not allow arbitrary overloads or user functions.

`date_trunc(text,timestamp)` accepts literal units from microseconds through
millennium, including week, month and quarter. Timestamptz overloads and
dynamic units are not part of this slice. `to_char(timestamp,text)` retains
its existing checked builtin overload and literal-format requirement.

`coalesce`, `nullif`, `greatest` and `least` admit common scalar types,
including NULL branches and text/varchar promotion. Other mixed-type branches
require explicit casts. Every argument is checked even in an unreachable branch.
`nullif` and min/max expressions require their verified comparison signature.
`BETWEEN` also accepts admitted scalar expressions, including `EXTRACT`.

## Security, freshness and remaining limits

The entire AST still passes the write detector, SQL-byte/node/depth budgets,
visible-relation/column binding, physical-name rewrite, deparse/reparse check,
and a second admission against the physical catalog. Planner cost, role
execution, read-only transactions and execution limits are unchanged. Writes,
DDL, COPY, system-relation reads and non-allowlisted functions remain refused
with closed reason codes.

Literal dates describe a query window, not source freshness. They do not
replace publication/freshness receipts or turn `source_time=unmeasured` into
a measured timestamp. `now()` and `current_date` remain unadmitted in this
implementation; no runtime-clock freshness guarantee is made.

V3-I-08 is not complete: subqueries/CTEs, the remaining
pure-function allowlist and full ERP/T6 admission are still outstanding.
Clock expressions and the remaining admission corpus are not yet complete.
