# oo.source-control-api/v1

SP03/SP05 product surface for B02. Schema `oo_source_control`. This file, `openapi/source-control-api.openapi.yaml`, and `schema/` were frozen before the service.

This service registers a local private PostgreSQL source or a bounded file object, queues an import, calls the fixed producers below, and stores catalog, profile, state, and redacted errors. It does not build, review, classify, publish, grant, or release. `build_review` is `not_claimed`. `release` is `not_claimed`. `published` is false on every success body. `customer_integration` is `not_claimed`. A green test does not accept SP03, SP05, or B02.

## Frozen acceptance

| Clause | Proof |
|---|---|
| Real OIDC current | Every request forwards only `__Host-oo-session` to `GET /control/v1/identity/current` on the configured origin. Caller JSON is not authority. |
| Local PG source or bounded upload | `registerSource` admits only an allowlisted host. `uploadSourceObject` stores bytes under the server object directory, capped by `upload_max_bytes`. |
| 202 import | `createImport` commits the operation and returns 202 `queued`. |
| Real producers | `pg_inspect` spawns pinned `pg-onboarding inspect`. `file_replace` spawns pinned `file-import-pg`. No in-process substitute. |
| Durable catalog, profile, state, redacted error | PostgreSQL rows in `oo_source_control`. Error text is a static token. Passwords are not stored. |
| List, detail, cancel, restart | The operations below. Restart resumes `failed`, `canceled`, or `resume_uncertain` only. |
| Password is a SecretRef | Request keys `password`, `passwd`, `secret`, `authorization`, `credential`, `private_key`, `ca_pem`, and `api_key` are `malformed`. The server resolves `secret_ref` to an operator file. |
| Enterprise sources are not called | A host outside `admitted_hosts` is `source.not_admitted` and spawns nothing. |
| 100000 rows in private PostgreSQL | `file_replace` with `max_rows` 100000. `pg-onboarding` inspect omits budget, so its producer default remains 10000 rows. This service does not patch that producer. The PG source proof is a real inspect inside that cap. |
| Failure is not ready | Public status has no `ready` value. A non-zero producer leaves `failed`. |
| Cancel, restart, idempotency, cross-project | Same-body replay, different-body `409`, cancel before spawn, restart after cancel or crash, other-project `403` with the same body as a missing id. |
| Exchange with a373 | `getSourceCatalog` returns `oo.source-control.exchange/v1`. `source_ref` matches `^[a-z][a-z0-9_]{0,62}$`, the same token `createBuild.source_ref` already requires. This service does not write selfbuild, the frontend, or the root workspace. |

## Frozen dependencies

| Product | Commit | Use |
|---|---|---|
| control-identity | `a6f92da8a0bf872f8a30efb116c2c7b14463505b` | Live `getIdentityCurrent` only. Schemas at `089b00d7f70da1bc42f7e201dfa070748c651d5b` still describe the envelope this service checks. Not a path dependency. |
| pg-onboarding | `50d4e705a2bb4ba014919ef1e071da065cf8e9b7` | `inspect` only. `apply-reviewed` is not called. |
| file-import-pg | `b60f954897a6d305e72bd5deb82871474ddaae57` | Replace-only CSV, TSV, or JSONL. The tool contract also names archive `72dc9e2db841de4de368216bbbe13d738f850ea9`. |
| oo-pipeline `init-pg`, `oo-pipeline`, oo-builder `oo-builder`, `oo-file-parser-worker` | workspace commit that built the pinned bytes | Child binaries `pg-onboarding` and `file-import-pg` already require. This service pins path and SHA-256. It does not reimplement them. |

## Operations

Anything else is `not_found` or `method.not_allowed`. There is no pause, resnapshot, DBA apply, secret read, review, or publish route.

| B02 concept | operationId | Method and path | Success |
|---|---|---|---|
| connector matrix | `getConnectorMatrix` | `GET /control/v1/connectors` | 200 |
| source config | `registerSource` | `POST /control/v1/sources` | 201 |
| source list | `listSources` | `GET /control/v1/sources?project_id=` | 200 |
| source detail | `getSource` | `GET /control/v1/sources/{source_ref}` | 200 |
| bounded upload | `uploadSourceObject` | `POST /control/v1/sources/{source_ref}/objects` | 201 |
| import | `createImport` | `POST /control/v1/imports` | 202 `queued` |
| import list | `listImports` | `GET /control/v1/imports?project_id=` | 200 |
| import detail | `getImport` | `GET /control/v1/imports/{operation_id}` | 200 |
| cancel | `cancelImport` | `POST /control/v1/imports/{operation_id}/cancel` | 202 |
| restart | `restartImport` | `POST /control/v1/imports/{operation_id}/restart` | 202 |
| catalog | `getSourceCatalog` | `GET /control/v1/sources/{source_ref}/catalog` | 200 exchange |

`source_ref` and `operation_id` are single path segments. `operation_id` is `op_` plus 32 lowercase hex characters.

## Identity

The service calls the configured `https` origin at exactly `/control/v1/identity/current`. It uses the configured CA, does not follow redirects, and sends only the session cookie. A 401 from identity is `401 unauthenticated`. Any other failure, redirect, or untyped body is `503 dependency.not_ready`. `install_state` must be `initialized`.

| Identity capability | Scopes |
|---|---|
| `project.admin` | register, upload, create, cancel, restart, and every read |
| `audit.read` | `source.read`, `import.read`, `catalog.read`, `connector.read` |
| `identity.current` alone | `connector.read` |

A project id outside the verified session is `403 forbidden` with `detail` `scope`, including when the id exists in another project.

Mutations require `Origin` equal to one configured deployment origin, `X-OO-CSRF` equal to the current envelope `csrf_token`, and `identity_revision` equal to the envelope `revision`. Uploads send `identity_revision` in `X-OO-Identity-Revision` because the body is raw bytes. A stale identity revision is `409 revision.conflict`. A stale `expected_source_revision` is the same code.

## Register and upload

`registerSource` body keys are `project_id`, `identity_revision`, `source_ref`, `kind`, `idempotency_key`, plus kind fields.

| kind | Required | Rejected |
|---|---|---|
| `pg_local` | `secret_ref`, `connection` (`host`, `port`, `database`, `username`), `tables` (`schema`, `table`) | file fields |
| `file_local` | none beyond the common keys | `connection`, `secret_ref`, `tables` |

`connection.host` must be one of `admitted_hosts` and must not contain a password. `secret_ref` must be `secret://` plus 1..=128 of `[A-Za-z0-9_./-]`, and must name an operator file. The response never contains `secret_ref`, a password, a filesystem path, or argv.

`uploadSourceObject` accepts `application/octet-stream` up to `upload_max_bytes`. Header `X-OO-SHA256` must equal the SHA-256 of the body. The source kind must be `file_local`. The bytes are a regular file under the server object directory.

The same idempotency key and the same body hash replay the resource. The same key and a different body are `409 idempotency.conflict`. The key is 16..=128 of `[A-Za-z0-9_-]`.

## Import

`createImport` body keys are `project_id`, `identity_revision`, `source_ref`, `kind`, `idempotency_key`, `expected_source_revision`, and kind fields.

| kind | Required | Producer |
|---|---|---|
| `pg_inspect` | none | `pg-onboarding inspect` |
| `file_replace` | `object_sha256`, `profile`, `budget` | `file-import-pg` |

`file_replace` profile is the file-import profile object. `strategy`, paths, worker, target, and password are not caller fields. The server writes them into the producer request. Budget caps are `max_rows` 1..=100000, `max_total_bytes` 1..=`upload_max_bytes`, and both deadlines 1..=120000. Other closed file-import inputs stay as that producer's contract.

Public status is only `queued`, `running`, `awaiting_review`, `failed`, `canceled`, `cancel_requested`, or `resume_uncertain`. `ready` and `ready_for_trusted_query` are not public statuses. A producer success of `awaiting_review` is stored as `awaiting_review` with `published` false. Any other producer outcome is `failed`.

The worker is serial per project. Cancel of `queued` becomes `canceled` without a spawn. Cancel of `running` sets `cancel_requested` and signals the child process group. The operation becomes `canceled` only after the child is reaped. Cancel of a terminal success is `409 conflict`.

On process start, `running` and `cancel_requested` become `resume_uncertain`. A stored child process group is signaled first. `resume_uncertain` is not `awaiting_review`. `restartImport` queues another attempt with the same input hash and a new producer directory. `pg-onboarding` creates that directory. It is not reused.

## Exchange

`getSourceCatalog` returns one `oo.source-control.exchange/v1` object: `project_id`, `source_ref`, `revision`, `kind`, `state`, `catalog_sha256`, `profile_sha256`, `catalog`, `profile`, `published` false, `build_review` `not_claimed`, `release` `not_claimed`. `catalog` for `pg_inspect` is the producer document `oo.source.pgsql/v1`. `catalog` for `file_replace` is the producer result `oo.pipeline.file-import-pg-result/v1` with `rows` and `state`. No secret and no path is added. A visible source with no catalog document is `404 not_found` detail `catalog`. An unknown source and an other-project source are both `403 forbidden` detail `scope`.

a373 may copy `source_ref` into its own operator record. This service does not call a373 and does not accept a build request.

## PostgreSQL

`migrations/oo_source_control_v1.sql` runs once in one transaction. The manifest stores that file's SHA-256. The same hash replays. A different hash is install drift and is not repaired. The script creates no login and grants no data role. Audit rows are insert-only.

`synchronous_commit` is on for these transactions. The login is the operator user in the config. The password and the CA come from files.

## Commands

```text
source-control-api install --config CONFIG
source-control-api serve --config CONFIG
```

| Condition | Exit |
|---|---|
| malformed argv or config | 1 |
| refused business rule, or a pinned binary hash mismatch | 2 |
| installed or listening | 0 |
| commit failed after the transaction began | 3 |

Stdout is one JSON object and contains no cookie, CSRF token, password, path, or argv.

## Dependencies

Direct pins already in the repository root lock. No new crate, no version bump, no root manifest edit, no root lock edit, no `cargo update`. `rust-version` is 1.94.0. Safe Rust only.

## Not claimed

MySQL, Parquet, Excel, SQLite, SQL dump, pause, resnapshot, DBA apply, secret read, semantic publish, build review, role grant, and an enterprise source connection.
