# enum-store-v1

Single-project PostgreSQL materialization of one `enum-history/v1` report.
`tools/enum-history` stays the compiler. This crate re-executes `enum_history::compile`
on the caller YAML, civil `as_of`, observations, and threshold spelling, then stores
that report. It does not convert business rows, infer a new enum class, or certify
finance, semantics, metrics, or an owner. Shared-project authentication remains a
parent requirement. One deployment binds one `project_id`.

The private schema is `oo_enum_control`. The readable object is `oo_dict.enums`,
owned by `NOLOGIN` role `oo_enum_view`. The view columns are `enum_id`, `code`,
`description`, `class`, `valid_from`, `valid_to`, `version`, `as_of`, `project_id`,
and `status`. `description` is the value label. `class` is the raw class.
`project_id` is the single project binding. There is no view argument and no clock
call. Dates are civil dates. Unbounded ends stay null.

`status` is one of `approved`, `undetermined`, `unexplained`, `superseded`, or
`deprecated`, copied from the report. A superseded or deprecated row stays that
status when `historical_business_class` is true. An unknown observation code is
stored as `status = unexplained` with a null class, and every compiler warning is
stored in `oo_enum_control.notice`. Those rows are not dropped. An
`inferred_unapproved` refusal inserts nothing.

Snapshot identity is the SHA-256 of `project_id`, `enum_version`, the YAML
SHA-256, `as_of`, the observations SHA-256, and the threshold spelling. Omitted
threshold uses the spelling `0.95`. `0.950` is a different snapshot. The report
SHA-256 is stored and is not part of the identity. The same identity and the same
snapshot, notice, and published payload replay. Any stored difference is
`snapshot_drift` and the stored row is left unchanged. A different
`expected_report_sha256` inserts nothing.

`activate` is an expected-current compare-and-swap. It runs only after the
snapshot, notices, and published lines are present and match the stored counts.
The same transaction replaces `oo_enum_control.served` from those published lines
and grants `SELECT` on the view. `served` is a column projection, so the view is
a simple select. A runtime role has no `UPDATE` privilege. Two activations that
both expect the current pointer to be null produce one winner and one
`cas_conflict`. Grants that fail roll the pointer and the served rows back.

Install executes `migrations/oo_enum_control_v1.sql` once, in one transaction,
with fixed identifiers and parameterized values, and records that file's SHA-256.
The same hash, catalog, project, and allowlist replay. A different hash or shape
is `install_drift` and is not repaired. A foreign `oo_dict` schema or a foreign
`oo_dict.enums` is `collision` and is not dropped. Runtime roles are an explicit
allowlist. They are not members of `oo_enum_view`. Before activate, a runtime
`SELECT` is `42501`. After activate, runtime `SELECT` is limited to the view.
`oo_enum_view` can read `served` and cannot read `snapshot.enum_yaml`.

## Shared `oo_dict`

`oo_dict` may hold `enums` and `calendar` together. The schema owner is the
admin who created it: `oo_enum_admin` or `oo_calendar_admin`. Adoption does
not run `ALTER SCHEMA` and does not take ownership. A third owner is foreign.

Hash recipe, unchanged file bytes:

| file | digest |
|---|---|
| `tools/enum-store/migrations/oo_enum_control_v1.sql` | `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd` |
| `tools/calendar-store/migrations/oo_calendar_control_v1.sql` | `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` |

`migration_sha256` is SHA-256 of the exact enum file bytes via this crate's
`sha256_hex`. No byte is added, removed, or normalized.
`install_manifest.sql_sha256` stores that digest. Replay accepts only that
digest. Any other stored digest is `install_drift` and is not rewritten.
The executed adoption text is not the hash input. The SQL file has no
`IF NOT EXISTS`, no `CREATE OR REPLACE`, and no `DROP`.

Both role pairs may exist before either install. This installer and the
calendar installer take advisory lock `881275280` before their own install
lock. `dict_create_open` and `dict_create_close` are parameterless
`SECURITY DEFINER` SQL functions, `search_path = pg_catalog`, owned by the
schema's creating admin. `PUBLIC` has no execute. When `oo_calendar_admin`
already exists and is a login non-superuser without `BYPASSRLS`, `CREATEDB`,
or `CREATEROLE`, and `oo_calendar_view` is the `NOLOGIN` view role of which
that admin is a member and not the reverse, the enum install grants that
admin `USAGE` on `oo_enum_control`, `SELECT` on `install_manifest` and
`runtime_allow`, and `EXECUTE` on the two functions. The manifest grant
exposes the migration digest only. A later appearance of those roles does not
add the grant. A superuser or
reversed membership is `collision` (`sibling_role`) and creates nothing.

Adoption, before any enum object is created, requires all of the following.
Otherwise the result is `collision` (`oo_dict`) and `oo_enum_control` is
absent. A sibling manifest the caller is not permitted to read is that same
refusal, not a query error, and it creates nothing:

- `oo_dict` owner is `oo_calendar_admin`.
- The only dictionary relation is view `calendar`, owned by `oo_calendar_view`,
  with `security_barrier=true`, `security_invoker=false`, the calendar column
  list, and a definition that reads `oo_calendar_control.published` and
  `oo_calendar_control.active_pointer` and contains no clock or private
  calendar token.
- `PUBLIC` has no privilege on the schema or that view. PostgreSQL records
  the schema owner's own privileges in `nspacl`; that owner entry is not
  another grant. `oo_calendar_view` has `USAGE` and not `CREATE`. An
  allowlisted runtime role may have `USAGE` after activate. Any other schema
  grant, and any `CREATE` that is not the owner's, is foreign.
- `oo_calendar_control` is owned by `oo_calendar_admin` and its manifest digest
  is the calendar digest above.
- The calendar adoption functions match the fixed grant and revoke bodies, and
  `oo_enum_admin` can execute them.

The open function grants `USAGE, CREATE` on `oo_dict` to `oo_enum_view`.
The enum view is then created by `SET LOCAL ROLE oo_enum_view`. The close
function revokes `CREATE` in the same transaction. `CREATE` does not remain.
A wrong body is not called.

When the calendar admin already exists, install also defines
`dict_grant_runtime(text)`, owned by this admin, `SECURITY DEFINER`,
`search_path = pg_catalog`, with no `PUBLIC` execute. It grants `USAGE` on
`oo_dict` only to a role named in `oo_calendar_control.runtime_allow`.
Activate calls the calendar function when this admin does not own `oo_dict`.
The owner still grants directly. Activate stops if the runtime role still
lacks `USAGE`. The function is absent when the calendar roles were absent
at install.

Replay with no calendar view accepts this catalog with the adoption functions
absent. That is an install from the accepted SQL before those functions
existed. It also accepts the functions when their owner, body, definer bit,
and `search_path` match. Replay with the calendar view present requires the
verified calendar catalog and exact functions on both sides. A wrong shape is
`install_drift`. Nothing is dropped and nothing is rewritten.

Install, put, activate, and read keep the `enum-store-v1` signatures. Each
product keeps its own project binding. This shared schema does not certify
finance, semantics, metrics, or an owner.

Receipts set `database_used` true and `finance_certified`, `semantic_certified`,
`owner_authenticated`, `metric_certified`, and `clock_used` false. The nested
report must keep its own six flags false. This crate does not schedule a sweeper
and does not read the current clock.
