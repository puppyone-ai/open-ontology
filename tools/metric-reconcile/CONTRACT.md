# metric-reconcile v1

Contract name: `metric-reconcile/v1`.

This crate compiles one core-valid OOS metrics document and, on `evaluate`, runs the metric statement and a separate operator control statement on one private TLS PostgreSQL session. It is the SEM-D05 / E42 metric template and G5b component. It is not a publisher, not an authenticator, and not the nightly or customer control-total job.

`owner_authenticated` is always false. `publication` is always false. `full_gates` is always false. `source_modified` is always false. `metric_certified` is true only when an evaluate report has G5a `passed` and G5b `passed` together, the enum classes are reviewed at the closed anchor, the control path is independent, and a supplied previous document does not require owner review. A compile-only report never sets `metric_certified`.

Omitted gates G1, G2, G3, G4, G6, G7, G8, G9, G10, G11, G12, and G13 are listed as `not_run`. They are not passed.

## What this crate uses

Metric YAML is parsed only by `oo_core::oos::parse_metrics_yaml`, which already runs `validate_metrics`. Enum YAML is parsed only by `oo_core::oos::parse_enum_yaml`. This crate does not invent metric fields. `replaced_by` is not a field of core `Metric`. The report copies `status`, `owner`, and `synonyms` from the parsed metric and sets `replaced_by` to null with `replaced_by_core_field: false`.

SQL admission uses `oo_gateway::guard_select`. That guard admits one plain `SELECT`. It rejects `EXPLAIN`, `$n` parameters, `GROUP BY`, `IN`, float literals, and non-boolean casts. This crate does not send metric text through `DedicatedSession` or `query-pg`. Those paths cannot `EXPLAIN`, cannot bind parameters, and refuse numeric result OIDs.

`EXPLAIN` and the value queries run in this crate's operator adapter. The adapter prepares only the guard-restored statement, with typed binds. It does not accept a second SQL string from the caller.

## Dependencies

Direct pins, exact versions already in the repository root lock:

| crate | version |
|---|---|
| oo-core | 0.1.0, path `../../crates/oo-core` |
| oo-gateway | 0.1.0, path `../../crates/oo-gateway` |
| serde | 1.0.229, feature `derive` |
| serde_json | 1.0.151, feature `arbitrary_precision` |
| sha2 | 0.10.9, default features off, feature `std` |
| tokio | 1.53.1, default features off, features `io-util`, `macros`, `net`, `rt-multi-thread`, `sync`, `time` |
| rustls | 0.23.45, default features off, features `std`, `tls12`, `aws-lc-rs` |
| tokio-rustls | 0.26.5, default features off, features `tls12`, `aws-lc-rs` |
| tokio-postgres | 0.7.18, default features off, feature `runtime` |
| postgres-types | 0.2.14, default features off |
| postgres-protocol | 0.6.12, default features off |
| tokio-postgres-rustls | 0.14.0, default features off, feature `aws-lc-rs` |

No new crate, no version bump, no root manifest edit, no root lock edit, no `cargo update`. The child `Cargo.lock` may contain only registry packages whose name, version, source, and checksum already appear in the root lock, plus the path packages `metric-reconcile`, `oo-core`, and `oo-gateway`. `rust-version` is 1.94.0. Edition 2024. Safe Rust only.

Decimal values use `oo_core::ExactDecimal`. Tolerance math is integer digit arithmetic. `f64` is not a comparison path.

## CLI

```text
metric-reconcile compile
metric-reconcile evaluate --config CONFIG
```

The request is one JSON object on stdin. `CONFIG` is a filesystem path. The password and the CA PEM are read from paths inside that file. They are never argv, never part of the JSON request, and never written to stdout, stderr, or the report.

| condition | exit | stdout |
|---|---|---|
| extra argument, missing command, empty stdin, stdin over 2_097_152 bytes, invalid JSON, unknown field, bad JSON type | 1 | empty |
| `outcome` `refused` | 2 | one report |
| `outcome` `compiled` or `evaluated` | 0 | one report |

Stderr for exit 1 is `metric-reconcile: malformed <reason>`. Stderr for exit 2 is `metric-reconcile: refused <reason>`. The reason is a static token. Gate `failed` and gate `block` on an evaluated report still exit 0. There is no force flag. An `expected` field is malformed.

`compile` does not open a socket. `evaluate` opens one TLS session only after the local checks below have passed.

## Request

Unknown fields are rejected. `previous_metric_yaml` may be omitted. `date` inside `anchor` may be omitted; that omission is a gap, not a default.

```json
{
  "metric_yaml": "schema_version: oos/v1\nmetrics: []\n",
  "previous_metric_yaml": null,
  "enums": ["schema_version: oos/v1\nenum: src.receipt.status\n..."],
  "as_of": "2026-04-01",
  "identity": {
    "project": "demo",
    "role": "oo_r_finance",
    "policy_id": "policy-1",
    "knowledge_id": "knowledge-1"
  },
  "anchor": {"name": "last_closed_period_end", "closed": true, "date": "2026-04-01"},
  "anchor_bound": ["period_end"],
  "source": "tms_pg",
  "parameters": {
    "period_end": {"type": "text", "value": "2026-04-01"}
  },
  "catalog": {
    "relations": [],
    "functions": [],
    "operators": [],
    "literal_types": {"integer": 23, "text": 25, "boolean": 16},
    "null_test_allowed": true,
    "large_table_row_threshold": 100000
  }
}
```

The document must contain exactly one metric. `identity.role` is the session role and must match `oo_r_` plus a lowercase ident. `identity.project`, `policy_id`, and `knowledge_id` are non-empty text of at most 128 characters with no NUL or other C0 controls. The guard identity is project, role, and policy id. `knowledge_id` is provenance only.

`anchor.name` is a core `AnchorName`. `anchor.date`, when present, is a core `CivilDate`. `as_of` is the same civil date. `source` must equal `control_total.source`. Every name in `anchor_bound` is a text parameter whose value equals `anchor.date`. The metric statement and the control statement receive that same parameter map.

`catalog` is the allowlist passed to `guard_select`. Relation and column idents must already satisfy the guard's closed-ident rules. This crate copies the request identity onto the catalog. It does not resolve OIDs from names.

At most 8 enum documents, 32 parameters, 32 anchor bindings, 32 relations, and 64 columns on a relation. Parameter names match `[A-Za-z_][A-Za-z0-9_]{0,63}`.

## Local checks, before any execute

Order, first failure wins. A failure here does not open PostgreSQL. `database_used` stays false. G5b is `block` / `not_executed`. `metric_certified` stays false.

1. `metric_count` when the document does not contain one metric.
2. `anchor_open` when `closed` is false.
3. `gap_anchor` when `closed` is true and `date` is absent.
4. `anchor_name` when the anchor name is not `control_total.as_of`.
5. `as_of_anchor` when `as_of` is not the anchor date.
6. `source_mismatch` when `source` is not `control_total.source`.
7. `unsupported_template` for `$n`, `$tag$` or `$$`, `E'...'`, an unclosed string or comment, a colon that is not a cast and not a parameter, or more than 32 parameters. `::` is a cast marker, not a parameter. The cast is left in the SQL. The guard then accepts or rejects it. This crate does not invent a cast syntax.
8. `unsupported_parameter_type` when a parameter type is not `text` or `int`. Decimal, bool, and null parameters are unsupported before execute. Result values may still be decimal, int, or null.
9. `parameter_value` when an int is not a base-10 integer in range for the catalog integer OID (`int2`, `int4`, or `int8` only).
10. `missing_parameter` or `unused_parameter`. Every template name used by the metric SQL or the control SQL must be present. Extra names are refused. Each metric `tests` entry must supply exactly the metric parameter names. `expect.rows` must be present.
11. `probe_collision` when the SQL already contains `oo_mr_probe_` or `810316`.
12. `negative_code` when, outside strings and comments, the metric SQL or the control SQL contains `<>`, `!=`, or `NOT IN`. `IS NOT NULL` is not a negative code. The statement is not executed.
13. `same_path` when the whitespace-normalized bound SQL strings are equal, or both guarded fingerprints are equal. The control text is the operator YAML. It is never copied from the metric SQL and never installed as a function.
14. `enum_document` for a core enum parse failure, overlap, reversed or empty interval, reserved class, review contradiction, or an approved inferred code without `review.state = approved` and `review.by`.
15. `missing_enum` when an `enum_filters` entry names an enum that was not supplied.
16. `unknown_class` when a requested class is not a declared business class. `undetermined` and `unexplained` are not business classes.
17. `inferred_unapproved` when the class has an active inferred code that is not an approved business binding.
18. `class_gap` when the class has no reviewed business code whose half-open interval contains `as_of`. A gap does not invent a code.
19. `previous_metric` when the previous document does not parse to exactly one metric.
20. `guard` when `guard_select` rejects the shape probe. The probe substitutes each text parameter with `'oo_mr_probe_NN'` and each int parameter with `810316000 + n`. Those literals are not the caller values. Values are not interpolated. If the deparsed probe does not contain each sentinel once, the reason is `probe_lost`.

Enum intervals follow enum-history: `[valid_from, valid_to)`, missing ends unbounded, touching ends allowed, overlap refused. An approved code with a declared business class is reviewed. A superseded or deprecated code is reviewed only when its class is a declared business class and `review.state` is approved with `review.by` present. The positive code list is reported. It is not written into SQL.

A semantic difference is grain, enum filters, SQL text, time dimension, default anchor, or control anchor, source, SQL, or tolerance. Status, owner, and synonym differences are reported beside those fields and also set `owner_review_required`. No field is rewritten. When `previous_metric_yaml` is omitted, `owner_review_required` is false.

`compile` stops after these checks. G5a is `block` / `not_executed` because tests and role `EXPLAIN` did not run. A guard failure on `compile` sets G5a to `failed` and the outcome to `refused`.

## Evaluate

The adapter uses `oo_gateway::connect_require_tls`. The server name is the config host. Certificate verification is required. There is no `NoTls` path.

Config file, unknown fields rejected:

```json
{
  "host": "oo-a316-pg",
  "port": 5432,
  "database": "oo_a316",
  "user": "oo_a316_login",
  "password_file": "/secrets/login.pass",
  "ca_file": "/secrets/ca.crt",
  "statement_timeout_ms": 5000,
  "max_result_bytes": 8192
}
```

`statement_timeout_ms` is 1 through 30000. `max_result_bytes` is 1 through 65536. The password file must be at most 4096 bytes, mode `0600` or stricter, and must not contain NUL. One trailing newline is stripped. The CA file must be at most 1_048_576 bytes.

Each statement runs in its own `READ ONLY` transaction:

1. `SET LOCAL statement_timeout`.
2. `SET LOCAL transaction_read_only = on`.
3. `SET ROLE` to the identity role. The role text is the already-checked `oo_r_` ident. It is not quoted from raw SQL.
4. Prepare the restored statement. Binds are PostgreSQL text or int values.
5. Fetch through a portal. A value query fetches at most 2 rows. A test fetches `expect.rows + 1`, at most 65. `EXPLAIN` fetches at most 32 text rows.
6. `COMMIT`. If commit is not confirmed, `ROLLBACK`. If rollback is not confirmed, the session is dropped and the outcome is `rollback_uncertain`.

`EXPLAIN (FORMAT TEXT)` is prefixed by the adapter to the restored metric statement only. A caller `EXPLAIN` never reaches the guard as an admitted statement. The value query is not `EXPLAIN`.

Value results:

| OID | canonical value |
|---|---|
| null | null |
| int2, int4, int8 | exact integer, scale 0 |
| numeric 1700 | `ExactDecimal` from the binary numeric, display scale kept |
| anything else | `shape` |

Numeric NaN, infinity, a short payload, or a fractional digit past `dscale` is `shape`, not a rounded value. One row and one column is the value. Zero rows or two rows is `shape`. A portal that hits the byte cap is `truncated`. SQLSTATE `57014` is `timeout`. These four cannot pass and cannot set `metric_certified`.

The control statement is the independent oracle. The request has no expected-value field. Comparison uses the two executed values.

G5a is `passed` only when parse, guard, enum review, negative-code absence, every test row count, and role `EXPLAIN` all succeeded on this evaluate. A test with the wrong row count sets G5a `failed`. A missing explain sets G5a `block`.

G5b:

| comparison | status |
|---|---|
| both values are decimals and every declared tolerance holds | `passed` |
| both values are decimals and a declared tolerance does not hold | `failed` |
| null, shape, truncated, timeout, rollback uncertain, connect failure, or not executed | `block` |

`tolerance.abs` and `tolerance.rel` are optional. Absent tolerance means exact equality. When both are present, both must hold. A negative tolerance is `invalid_tolerance` before execute. Relative tolerance against a zero control fails when the absolute difference is not zero. Scale is numeric: `0.010` and `0.01` are the same tolerance, and `1.0` equals `1.00`. The hand literal `1000000000000000000.01` is the live exact value. It is not compared through `f64`.

`metric_certified` stays false when G5a is not `passed`, G5b is not `passed`, the enum check forbids certification, or `owner_review_required` is true. A failed comparison does not flip the metric to certified.

## Report

One compact JSON object. Field order is the struct order. `receipt_sha256` is the SHA-256 of that same JSON with `receipt_sha256` set to the empty string.

Always present: `contract`, `outcome`, `owner_authenticated`, `metric_certified`, `publication`, `full_gates`, `database_used`, `source_modified`, `reason`, `gates`, `omitted_gates`, `lifecycle`, `provenance`, `enum_resolution`, `semantic_diff`, `metric_query`, `control_query`, `comparison`, `tests`, `explain`, `values`, `receipt_sha256`.

`gates` has two entries, G5a then G5b. Each has `gate_id`, `status` (`passed`, `failed`, or `block`), and `reason`. `provenance` includes the declared owner, source, anchor name and date, identity tuple, `password_logged: false`, and the connection host, database, and user when evaluate ran. Query objects include the raw SQL hash, the bound SQL, the guard fingerprint, and the executed SQL hash. `values.metric` and `values.control` are decimal strings or null. They are omitted from the success path only when the query did not produce a canonical value; the field is then JSON null.

## Limits

Stdin is 2_097_152 bytes. Core already bounds each SQL text. The guard budget remains 64 KiB. This crate adds no publish, no nightly schedule, no owner login, and no write to the source database.
