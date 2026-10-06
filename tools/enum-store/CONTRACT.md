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
is `install_drift` and is not repaired. A pre-existing `oo_dict` schema or
`oo_dict.enums` is `collision` and is not dropped. Runtime roles are an explicit
allowlist. They are not members of `oo_enum_view`. Before activate, a runtime
`SELECT` is `42501`. After activate, runtime `SELECT` is limited to the view.
`oo_enum_view` can read `served` and cannot read `snapshot.enum_yaml`.

Receipts set `database_used` true and `finance_certified`, `semantic_certified`,
`owner_authenticated`, `metric_certified`, and `clock_used` false. The nested
report must keep its own six flags false. This crate does not schedule a sweeper
and does not read the current clock.
