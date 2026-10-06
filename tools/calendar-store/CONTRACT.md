# calendar-store-v1

Single-project PostgreSQL materialization of one resolved business calendar.
The calendar arithmetic is `business_calendar::resolve` from the reviewed helper
at commit `ec8b3953cdcb63035d64fbeca3310214aab002ef`. This crate does not
reimplement anchors, phrases, or zone folds.

This is the first dedicated Agent DB deployment child for that helper.
The database is bound to one project at install. The binding does not change.
A shared multi-project calendar is not created here. A later parent has to
authorize that explicitly. This view never takes a project argument.

Docs/05 §6.3 draws `oo_dict.calendar` with `now()` and `date_trunc`.
This crate does not. Those expressions shift a missing midnight and they read
the system clock. The stored row is the helper's civil date, midnight status,
and instant. A gap or an overlap keeps its status and a NULL instant.

`finance_approval`, `authenticated`, `ontology_published`, and `clock_used`
stay false. Activation does not close a financial period and does not publish
an ontology.

## Pins

Standalone workspace. Path dependencies are `../business-calendar` and, through
that crate, `oo-core`. a207 remains the only writer of the connector manifest
and the root lock. This crate does not edit either.

Direct versions are the pins already in the fixed root lock
`216e6df82a61d2bc22548ce1a5075d5f4a045648`:

| crate | version |
|---|---|
| serde | `=1.0.229` |
| serde_json | `=1.0.151` with `arbitrary_precision` |
| tokio | `=1.53.1` |
| tokio-postgres | `=0.7.18` |
| rustls | `=0.23.45` |
| tokio-rustls | `=0.26.5` |
| tokio-postgres-rustls | `=0.14.0` |
| postgres-types | `=0.2.14` |
| postgres-protocol | `=0.6.12` |

SHA-256 is `business_calendar::sha256_hex`. No new digest crate.
`rust-version` is 1.94.0. Edition 2024. Safe Rust only.

The child lock is a registry subset of that fixed root lock.
Path packages are only `calendar-store`, `business-calendar`, and `oo-core`.
A missing or mismatched registry triple is a supply conflict.
This crate does not run `cargo update` or `cargo generate-lockfile`.

## TLS

`rustls_connector` follows the reviewed session-store recipe and
`oo_gateway::policy::rustls_connector`: aws-lc-rs, an empty root store plus
the caller PEM, default protocol versions, hostname verification, no client
auth, no webpki roots, no native roots. The only mode is `SslMode::Require`.
Production connects with `tokio_postgres_rustls::MakeRustlsConnect`.

## Roles

Roles are created outside this migration.

| role | rule |
|---|---|
| `oo_calendar_admin` | install, put, and activate; `current_user` must be this role; not superuser; no `BYPASSRLS` |
| `oo_calendar_view` | `NOLOGIN`, not superuser, no `BYPASSRLS`, no `CREATEDB`, no `CREATEROLE`; owns `oo_dict.calendar` |
| allowlisted runtime roles | existing roles named by the install request; `SELECT` on the view only after activate |

The admin is a member of `oo_calendar_view` so it can create the view as that
owner and grant `SELECT` as that owner. Runtime roles are not members of the
view owner, and the view owner is not a member of a runtime role.
The connected user is not added to the allowlist.
`oo_calendar_admin`, `oo_calendar_view`, `public`, and `postgres` cannot be
allowlisted. Each allowlist name matches `^[a-z_][a-z0-9_]{0,62}$`, already
exists, is not superuser, and has no `BYPASSRLS`. At most eight names.
Duplicates are `input`.

The view owner receives `USAGE` on `oo_calendar_control` and `SELECT` on
`published`, plus column `SELECT` on `active_pointer`. It receives no
privilege on `snapshot`, `deployment`, `runtime_allow`, or `install_manifest`.
It receives no business-table or source-table privilege and no credential.

## Schema

Private schema `oo_calendar_control`. Readable schema `oo_dict`, holding only
the view `oo_dict.calendar`.

`install_manifest` stores the SHA-256 of `migrations/oo_calendar_control_v1.sql`.
`deployment` stores the one project and the four false flags.
`runtime_allow` stores the allowlist names and nothing else.
`snapshot` stores the YAML, the closure label, the closure record hash, the
config digest, the resolved digest, and the helper JSON.
`published` stores only the view columns for every snapshot.
`active_pointer` stores the one active snapshot id, or NULL before activation.
It does not store a config digest, a closure label, or the YAML.

The snapshot identity is SHA-256 of:

```text
calendar-store-snapshot-id-v1
project_id
calendar_version
config_sha256
as_of
closure_fingerprint
```

`closure_fingerprint` is `none`, or SHA-256 of the helper's closure date,
label, and record hash under the prefix `calendar-store-closure-v1`.
The resolved digest is not part of the identity.

`config_sha256` and `resolved_sha256` are 64 lowercase hex digits.
`resolved_sha256` is SHA-256 of the compact `serde_json` encoding of the
helper `Report`. Field order is the struct order. Non-ASCII is not escaped.
The helper report is hashed unchanged, including `database_used: false`.

## Install

One transaction. `search_path` is `pg_catalog`. Advisory lock `881275289`.
`lock_timeout` 10s, `statement_timeout` 15s, `synchronous_commit` on.

Before any create, a present `oo_dict` schema or a present `oo_dict.calendar`
is `collision`. The transaction rolls back. Nothing is dropped and nothing is
overwritten.

Absent private schema: execute the embedded SQL, insert the manifest, the
deployment, the allowlist, and a NULL active pointer, then verify catalogs.
The SQL file has no `IF NOT EXISTS`, no `CREATE OR REPLACE`, no `DROP`, and
no repair. A missing prerequisite role fails closed before the create.

Present schema: the stored SQL hash, owners, relations, columns, constraint
names, view owner, view options, and the view owner's grants must match.
The deployment project and the allowlist set must match the request.
Hash, shape, project, or allowlist disagreement rolls back with `install_drift`,
`project_binding`, or `allowlist`. No autofix.

A foreign `oo_dict` owner or a foreign view owner is drift or collision.
Replay does not execute the SQL again.

Install does not `GRANT` the view to runtime roles.

## Put

The caller supplies the original calendar YAML, the explicit `as_of`, the
optional closure, a calendar version token, and the expected resolved digest.
The caller does not supply anchor dates. There is no `SystemTime`, no
`Utc::now`, and no default today.

`business_calendar::resolve` runs on those inputs. A helper refusal inserts
nothing. The recomputed digest must equal `expected_resolved_sha256`.
A mismatch is `resolved_mismatch` and inserts nothing.
`finance_approval`, `authenticated`, `database_used`, and `clock_used` on the
helper report must be false. `contract` must be `oo.business-calendar/v1`.

The project argument must equal the deployment project. Any other project is
`project_binding`.

Same identity, same resolved digest, same YAML, same closure, and same report
JSON is `replay`. The row is not updated.
Same identity with any stored difference is `snapshot_drift`. The stored row
stays as it is.

Configured anchors are the ten `AnchorName` values. A name the helper does
not return is `absent`, with NULL date and NULL instant. The crate does not
invent a month or a week. `week_start` stays inside the private JSON.

| midnight | `*_midnight_status` | `*_midnight` |
|---|---|---|
| unique | `unique` | that timestamptz |
| gap | `nonexistent` | NULL |
| overlap | `ambiguous` | NULL |
| out of range | refusal, no insert | |

An ambiguous midnight does not pick a fold. A gap is not moved forward.

## Activate

Admin only. One transaction. The request names the expected current pointer
(`NULL` before the first activation), the snapshot id, the project, the
config digest, the calendar version, and the normalized `as_of`.
All of them must match the stored snapshot and the deployment project.
The pointer update is:

```text
WHERE singleton AND project_id = $project
  AND snapshot_id IS NOT DISTINCT FROM $expected_current
```

Zero rows is `cas_conflict`. The pointer is unchanged.
A content mismatch is `activation`. The pointer is unchanged.
The statement does not touch the deployment flags.

After the pointer update, the same transaction grants each stored runtime
role `USAGE` on schema `oo_dict` and `SELECT` on `oo_dict.calendar`.
The `SELECT` grant runs as `oo_calendar_view`.
A failed grant rolls back the pointer update.
No source or business grant is issued.

## View

```text
security_invoker = false
security_barrier = true
owner = oo_calendar_view
```

The view joins `published` to `active_pointer` on the snapshot id and the
project id. It has no parameter and it does not read `current_setting`.
Before activation the view is empty. After activation it is the one active
row.

Columns are `project_id`, `business_tz`, `as_of`, `calendar_version`, and for
each of the ten anchors `*_date`, `*_midnight_status`, `*_midnight`.
`as_of` is the stored instant. The view body does not call `now`,
`current_date`, `current_timestamp`, `clock_timestamp`, `date_trunc`, or
`current_setting`.

`PUBLIC` has no privilege. A runtime role that is not in the allowlist has
no privilege. An allowlisted role, before activate, has no privilege.
After activate it can `SELECT` the view and cannot `SELECT` the private
snapshot, the YAML, the closure label, the record hash, the config digest,
or the report JSON.

## Read

`read_view` connects as the supplied login over TLS and selects the fixed
column list from `oo_dict.calendar`. It does not accept a project id.
The login must not be `oo_calendar_admin` or `oo_calendar_view`.
Date columns come back as `YYYY-MM-DD`. Instants come back as UTC
`YYYY-MM-DDTHH:MM:SSZ`. A NULL instant stays NULL.

## Out of scope

No public gateway, no calendar-owner UI, no authenticated finance close, no
global semantic publication, no metric certification, no multi-project shared
view, and no claim that SP08 or SEM-D03 is accepted for the parent.
Production release remains a parent requirement.
