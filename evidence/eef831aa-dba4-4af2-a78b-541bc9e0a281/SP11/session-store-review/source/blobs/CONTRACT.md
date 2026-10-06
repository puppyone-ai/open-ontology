# session-store-v1

Durable PostgreSQL adapter for the fixed pure reducer `session-transition-v1`.
The reducer commit is `b0f7da4771e3c3625a45329c721e174f4380571c`
(tree `a25733870f1ffac55c057dd399270905ccca9151`). This crate calls
`session_transition::transition`. It does not reimplement transition rules.

This crate is a standalone workspace. It path-depends on `../session-transition`
only. It does not path-depend on `oo-gateway`, `oo-pipeline`, or `oo-connectors`.
a207 remains the only writer of the connector manifest and the root lock.
Direct crate versions and checksums are the pins already in the fixed archive
`9951cda7f128e30f1f6ce02e409d542d20a9e4fb`: tokio 1.53.1, tokio-postgres 0.7.18,
rustls 0.23.45, tokio-rustls 0.26.5, tokio-postgres-rustls 0.14.0,
postgres-types 0.2.14, postgres-protocol 0.6.12, serde 1.0.229, serde_json 1.0.151.
SHA-256 is the in-crate FIPS 180-4 function from the reducer. No new registry
and no new digest crate.

Public authentication is not implemented. A client-supplied principal is data
the reducer stores. It is not a login. The adapter refuses when
`identity.principal_id` equals `oo_session_admin`, `oo_session_runtime`, or the
connected `current_user`. Install and bootstrap require `current_user` to be
`oo_session_admin`. Transition, get, and reconcile require `current_user` to be
`oo_session_runtime`. There is no `SET ROLE` to the principal and no control IdP.

## TLS

`rustls_connector` follows `oo_gateway::policy::rustls_connector` and the
oo-pipeline / oo-connectors `PgSslMode::VerifyFull` rule: aws-lc-rs, an empty
root store plus the caller PEM, default protocol versions, hostname
verification, no client auth, no webpki roots, no native roots. The only mode
opened by this crate is `SslMode::Require`. Production connects with
`tokio_postgres_rustls::MakeRustlsConnect::new` on that `ClientConfig`.

The hide-commit connector is a test fault for a lost COMMIT reply. It uses the
same `ClientConfig`. It is not a plaintext path. The CLI cannot select it.
`CommitFault::None` is the production path. `RollBackBeforeCommit` sends an
acknowledged `ROLLBACK` and does not send `COMMIT`. `DropCommitResponse` writes
the simple-query `COMMIT`, waits until the server response bytes arrive, then
returns `UnexpectedEof` so the client never observes `CommandComplete`.

A COMMIT I/O error with no SQLSTATE is `commit_unknown`. It is not
`rolled_back`. The caller reconciles on a fresh connection by event id.
A server `ErrorResponse` (`Error::code` / `as_db_error`) is a definite
`rolled_back`. This crate does not call `CancelToken`, `pg_cancel_backend`, or
`pg_terminate_backend`, and it does not describe a lost reply as a cancel.

## Schema

Schema `oo_session_control`. Tables `install_manifest` (singleton, `sql_sha256`)
and `session_head`. Names are distinct from `oo_control` and from `schema_000x`.
`revision` and `owner_generation` are `numeric(20,0)`. `state_json` is `text`
holding compact serde JSON, not `jsonb`. The row key is `(project_id, session_id)`.
The current bound tuple is seven columns, all null or all present, and
`bound_project_id` must equal `project_id`. `journal_len` is 0 through 1024.

The journal cap is the reducer's `DEDUP_JOURNAL_CAP` (1024). `dedup_exhausted`
stays a refusal. This crate does not raise the cap and does not drop entries.

Roles are created outside this migration. `oo_session_admin` owns the tables
because it runs the migration. `oo_session_runtime` receives `USAGE` on the
schema and `SELECT`, `UPDATE` on `session_head` only. It receives no
`INSERT`, `DELETE`, or `TRUNCATE`, and no privilege on `install_manifest`.
`PUBLIC` is revoked. There is no row security policy that treats a client GUC
as an identity. No grant is issued on source or business tables.

## Install

One transaction. `search_path` is `pg_catalog`. Advisory transaction lock
`881275031`. The SQL file is hashed with the in-crate SHA-256 before execution.

- Absent schema: execute the embedded SQL, insert the hash, verify relations,
  columns, named check and primary-key constraints, and the runtime grants,
  then commit. The SQL file has no `IF NOT EXISTS`, no `CREATE OR REPLACE`,
  and no repair statements.
- Present schema, matching hash, matching shape and grants: commit without
  executing the SQL again. Disposition `replay`.
- Hash mismatch, missing manifest, missing table, or grant or shape drift:
  roll back. Disposition is the error `install_drift`. No autofix.
- `GRANT` fails closed when `oo_session_runtime` does not already exist.
- Install `COMMIT` I/O with no SQLSTATE is `commit_unknown`. The caller retries
  install. A matching hash on the retry is replay. This crate does not claim
  that an unknown install commit rolled back.

`synchronous_commit` is `on`. A successful commit function return is the
server acknowledgement.

## Bootstrap

Admin only. The caller supplies one `SessionState` for one `(project_id, session_id)`.
The state is probed with `session_transition::transition` and contract string
`session-store-probe`, which is not `session-transition-v1`. A valid state
yields reducer reason `input_invalid` because `validate_state` runs before
`validate_command`. Reason `state_invalid` refuses before any insert.
Any other probe result is `reducer_disagreement` and inserts nothing.

Columns are derived from that state. The insert is not an upsert.
`23505` rolls back and a new transaction reads the row. Equal canonical state
is `replay`. A different state is `bootstrap_conflict`. The row is not updated.

The canonical empty session matches the reducer's requested shape: phase
`requested`, revision 0, owner generation 0, empty owner, no bound tuple, no
workspace, zero clocks, empty tool lists, empty journal, default release record.

## Transition

Each call opens a fresh TLS connection. `BEGIN`, then
`SET LOCAL search_path`, `lock_timeout` 10s, `statement_timeout` 15s,
`synchronous_commit` on. `SELECT ... WHERE project_id = $1 AND session_id = $2
FOR UPDATE`. The filter is the caller's project and session arguments.
`identity.project_id` must equal that project key or the call returns
`project_key_mismatch` and does not run the reducer.

The row's `revision`, `owner_generation`, `journal_len`, `session_id`, and
bound columns are canonical. They must match the parsed `state_json` before
the reducer runs. A mismatch is `consistency`. The adapter does not rewrite
the row to repair it.

The reducer input is the database snapshot plus the caller `expected_revision`,
`expected_owner_generation`, `event_id`, `now_ms`, `identity`, and `event`.
`now_ms` is the caller clock. This crate does not read a global clock.
The reducer contract string passed here is `session-transition-v1`.

An `applied` result is persisted only when all of the following hold: reason is
absent, reducer flags `database_used`, `auth_provider_used`, `runtime_used`,
and `physical_release_certified` are false, revision is the locked revision
plus one, owner generation is unchanged or plus one, the journal grew by exactly
one entry, that entry is this `event_id`, the prior journal prefix is unchanged,
and the bound project matches the row key. The `UPDATE` also predicates the
locked revision and generation. The row count must be 1. Any other reducer
result updates nothing.

`idempotent` and `refused` commit the read-only transaction. The reducer's
journal check runs before the stale check, so an identical event retried with
the pre-image expected revision returns the durable row and does not append.
A different payload under an event id already in the journal is
`event_id_conflict` and updates nothing.

Parallel calls that share an expected revision and use different event ids:
the lock admits one writer. The other reads the new revision and returns
`stale`. The stored revision increases by one and the journal gains one entry.

Unknown and rolled-back results omit the tentative reducer state.
The envelope `database_used` field is true when this adapter used PostgreSQL.
The nested reducer object keeps the reducer's own flags, which stay false.

## Get and reconcile

Get reads one key and returns the checked snapshot. A missing row is
`not_found`. Reconcile reads one key on a fresh connection and does not
transition. The event id is durable when it is present in the stored journal.
Otherwise the disposition is `absent`, including when the row does not exist.
A consistency mismatch is `consistency`, not `absent`.

## CLI

The process accepts no arguments. One JSON document on stdin, at most
1_048_576 bytes, contract `session-store-v1`. The password and the CA PEM are
fields of that document. There is no password file, environment variable, or
argv secret. Serde error text is not written to stdout or stderr.
The password buffer is zeroed after the call.

Exit 0: acknowledged applied, idempotent, install, bootstrap replay or insert,
get, reconcile durable or absent.
Exit 2: reducer refusal, not found, consistency, drift, conflict, project key,
login role, principal-not-authentication, state too large, reducer disagreement.
Exit 3: `commit_unknown`.
Exit 1: malformed input, TLS, or connect failure.

## Out of scope

Control IdP, harness launch, physical S1–S11 certification, and a full SP11
claim are out of scope. Reducer flags that name those certifications stay false.
Independent review a272 of the reducer was not present when this contract was
written. A later concrete a272 defect is a dependency failure of the affected
proof. This crate does not edit the reducer to clear that defect.
