# Gateway policy catalog

Contract `oo.gateway.policy-catalog/v1`. Library `oo_gateway::policy::catalog`.

This is the live PostgreSQL loader for one `sql_guard::PolicyCatalog`. A trusted administrator session reads the current physical mapping, the role shape, `pg_class` / `pg_attribute`, and `has_column_privilege`. The caller supplies that session. The loader does not accept a token, a password, a DSN, or a claims map, and it does not authenticate the session.

Published classification is a `CompiledPolicy` from `compile_column_acl`. That allowlist only restricts columns. It is not a grant and it is not a user claim. A column is visible only when it is in the allowlist and `has_column_privilege` for the policy role returns true. A published column the role cannot select is omitted and counted in `pending_unclassified`. A published column whose type is not one supported base type refuses the load. A hidden column, including `_oo_pos`, is counted and its name is not returned.

## What this slice loads

`load_policy_catalog` checks the identity, the limits, and that every compiled policy shares one project, role, config digest, generation, and login before it uses the client. `PolicyIdentity.project` is the mapping source. `PolicyIdentity.role` is `oo_r_<slug>`. `PolicyIdentity.policy_id` is the config digest.

It then requires `pg_stat_ssl.ssl` for this backend and opens one `REPEATABLE READ READ ONLY` transaction. The transaction sets `search_path` to `pg_catalog` and `statement_timeout` to 30 seconds, and the loader reads those settings back. Catalog SQL is parameterized. The only text interpolated into SQL is a fixed `FROM` clause in this module. Logical names, role names, and type names travel as parameters.

Inside that snapshot the loader checks:

1. `max(run_generation)` in `_oo.physical_mappings` for the source and config digest equals the policy generation. A later generation stored under another config digest is not this maximum.
2. `pg_roles` has one row for the role and all seven privilege flags are false (`rolcanlogin`, `rolsuper`, `rolcreatedb`, `rolcreaterole`, `rolinherit`, `rolbypassrls`, `rolreplication`).
3. Each base type `bool`, `int2`, `int4`, `int8`, and `text` exists once in `pg_catalog` with `typtype = 'b'`. OIDs come from that row. None are hard-coded.
4. The operator subset below exists once each, with the stated schema, signature, volatility, kind, language, and extension checks.
5. `pg_catalog.count(*)` exists once, with `prokind = 'a'`, `pronargs = 0`, immutable, internal, normal aggregate, result `int8`, and not an extension member.
6. Each logical table and column has one `physical_mappings` row and `_oo.lookup_physical_mapping` returns that same `pt_` or `pc_` name.
7. `pg_class.reltuples` for the mapped table. `-1` uses the caller's `UnknownRows` policy (`Refuse`, `Zero`, or `Saturate`). Any other negative, NaN, or infinite value is refused. A finite value of at least zero is rounded half away from zero into `estimated_rows`.

Counts run before the detail rows are collected. A count above the caller limit refuses the load. Limits also have absolute caps: 32 tables, 512 columns, 8 functions, 128 operators. `max_operators` must be at least the size of the subset below, so a caller cannot ask for a partial whitelist.

The transaction is committed, not dropped. After commit, `ensure_committed` requires `pg_current_xact_id_if_assigned()` to be null. That null is a weak check. An open `REPEATABLE READ READ ONLY` transaction that has not assigned a transaction id also returns null while `xact_start` is set, so the check does not prove the session is idle. A failed rollback replaces the original error with an incomplete-transaction error. Display text carries a five-character SQLSTATE or a static label. It does not carry SQL, a DSN, a password, or a hidden column name. `OO024` from lookup is `MappingAbsent`.

The loader does not `SELECT` a mapped table, sample its rows, or write. One call is one role and one project.

## Operator and function subset

This slice resolves 54 builtin comparison operators and one aggregate. The full R4 list in `docs/07-gateway.md` stays open.

Operators are `<`, `<=`, `<>`, `=`, `>`, `>=` for these signatures:

| Left | Right |
| --- | --- |
| bool | bool |
| int2 | int2 |
| int2 | int4 |
| int4 | int2 |
| int4 | int4 |
| int4 | int8 |
| int8 | int4 |
| int8 | int8 |
| text | text |

An integer literal uses the `int4` OID. The `int2`/`int4` and `int8`/`int4` pairs are what let that literal sit on either side of an `int2` or `int8` column. Text literals use `text`. Boolean literals use `bool`. `null_test_allowed` is true for this subset.

Each operator must be a binary `pg_catalog` operator whose result is `bool`, implemented by an immutable, non-set-returning, two-argument internal function with the same argument and result types. The operator and the function must not be extension members (`pg_depend.deptype = 'e'`).

`count(*)` is the only function. Its `argument_type_oids` are empty and `aggregate_star` is true. The loader does not accept a function or operator name from the caller. `pg_sleep`, a user function, `count(column)`, and operators outside the table above are not entered into the catalog.

## What stays open

Authentication of the administrator and of the login is open. This loader trusts the TLS client it is given.

The audit WAL, row-level security, the public worker, and the connection pool are open. `SP06-W01` and `SP06-W02` in the plan stay open. This contract does not close them.

The rest of the R4 whitelist is open. Arithmetic, pattern operators, `lower`, `upper`, `coalesce`, hashes, `date_trunc`, `isfinite`, `length`, `round`, `trunc`, and `count` of a value are not resolved here.

A non-superuser grantor is not proven here. The live fixture uses `oo_fixture_admin` and applies frozen `0000`, `0001`, `0003`, `0004`, and `0005` itself. `0002` is not applied.
