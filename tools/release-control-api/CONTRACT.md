# oo.release-control-api/v1

SP08 release and rollback product API. Schema `oo_release_api`. This file, `openapi/release-control-api.openapi.yaml`, and `schema/` were frozen before the service.

This service is the authenticated management surface for one project's publication pointer. It is not SP08 acceptance, not a customer approval, not an owner certification, and not a production release. `full_sp08_accepted` is false on every body. `production_ready` is false on every body and is a `FALSE` column. `customer_gate_verified`, `owner_authenticated`, and `runsc_verified` are false the same way. `certificate_class` is only `not_issued` or `test_scope_not_production`. A private fixture that passes a test-scope gate does not promote itself.

## Frozen inputs

| Input | Pin | How this service uses it |
|---|---|---|
| control-identity | service `a6f92da8a0bf872f8a30efb116c2c7b14463505b`, schemas `089b00d7f70da1bc42f7e201dfa070748c651d5b` | Live `GET /control/v1/identity/current` only. Not a path dependency. Caller JSON is not authority. |
| release-coordinator | `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3` | Path consumer of that tree. Stage, gate, checkpoint, plan, activate, rollback, and status are that crate's functions. This service does not reimplement gate admissibility or pointer CAS. |
| selfbuild reviewed pack | operator file named by `config_commit` | Envelope `oo.selfbuild-control-api/v1` with `compiled_pack=not_published`, `published=false`, `active=false`, and producer `oo.pg-builder.pack-combined/v1`. Two role packs. This service does not compile a second pack and does not call a moving selfbuild process. |

`config_commit` is the 64-hex commit of the reviewed pack file. It is not a production git signature.

## Operations

These are the only HTTP operations. Anything else is `not_found` or `method.not_allowed`. There is no force, waive, grant, or secret-read route.

| B05 name | operationId | Method and path | Success |
|---|---|---|---|
| publish | `createRelease` | `POST /control/v1/releases` | 202 `queued` |
| publish | `listReleases` | `GET /control/v1/releases?project_id=` | 200 |
| publish | `getRelease` | `GET /control/v1/releases/{operation_id}` | 200 |
| publish | `getActiveRelease` | `GET /control/v1/releases/active?project_id=` | 200 |
| pack | `getActiveRolePack` | `GET /control/v1/releases/active/pack?project_id=&role=` | 200 |
| publish | `rollbackRelease` | `POST /control/v1/releases/{operation_id}/rollback` | 202 `queued` |
| gates | `getRelease` field `gates` | the get above | stored producer dispositions |

`operation_id` is `op_` plus 32 lowercase hex characters. `active` is not an operation id.

## Identity

Every request forwards only the opaque `__Host-oo-session` cookie to the configured identity origin at exactly `/control/v1/identity/current`. The origin is `https` with no userinfo, path, query, or fragment. The service uses the configured CA, does not follow redirects, and has no identity cache. `install_state` must be `initialized`. `authority` must be `management_session`.

| Condition | HTTP | Code |
|---|---|---|
| No session cookie, or identity `401` | 401 | `unauthenticated` |
| Identity unreachable, redirect, or untyped body | 503 | `dependency.not_ready` |
| Project outside the verified session, or outside this deployment | 403 | `forbidden` detail `scope` |
| Missing capability | 403 | `forbidden` detail `capability` |
| Pack role is not the sealed role | 403 | `forbidden` detail `role` |
| Bad `Origin` | 403 | `origin.rejected` |
| Bad `X-OO-CSRF` | 403 | `csrf.rejected` |
| `identity_revision` differs | 409 | `revision.conflict` |

A missing operation and an operation in another project use the same `403` `forbidden` body. The body does not say which case it was.

| Identity capability | Scopes |
|---|---|
| `project.admin` | `release.create`, `release.read`, `release.rollback`, `pack.read` |
| `audit.read` | `release.read`, `pack.read` |
| `review.decide` | `release.read` |
| `ontology.draft` | `release.read` |
| `business.record`, `identity.current`, `binding.read`, `session.revoke` | none |

The role name is not authority. `review.decide` cannot publish or roll back.

Mutations require `Origin` equal to one configured deployment origin, `X-OO-CSRF` equal to the current envelope `csrf_token`, and `identity_revision` equal to `revision`.

## Publish

`createRelease` keys are exactly `project_id`, `identity_revision`, `idempotency_key`, `config_commit`, `candidate_id`, `version`, `base_version`, `role`, `scope`, and `expected_version`. `base_version` and `expected_version` are both null or the same token. `scope` is `test_scope` or `production`. Unknown fields, duplicate keys, `force`, `waive`, `all_accepted`, `allaccepted`, `product_mode_can_activate`, and `customer_approved` are `400` `malformed` and write nothing.

The server loads `{pack_dir}/{config_commit}.json` and `{pack_dir}/{config_commit}.gates.json`. The caller does not upload pack bytes. The pack must be a reviewed selfbuild envelope: two role packs, `model_publication` false, outcome `packed` or `reviewed_unchanged`, and no pack file text with an `UNREVIEWED` prefix. The selected `role` must be one of those packs. Policy, knowledge, and catalog version come from that role's visibility object.

The same idempotency key and the same canonical body replay the operation. The same key and a different body are `409` `idempotency.conflict`. The operation row and its queue audit commit in one transaction, then the response is `202`.

The worker calls the producer, in order: `install` (fixture authority, replayable), `stage`, gate records from the intake file, checkpoints from that file, then either a production `activate` or a test-scope `plan` followed by `activate` only when the plan is admissible. A refused plan does not call test-scope `activate`. `production` is never rewritten to `test_scope`. Gate issuer class is only `test_issuer`, `fixture_authority`, or `component_authority`. A fixture gate uses the installed authority hash. A `test_issuer` row, including one named `customer`, is passed through and cannot set `customer_gate_verified`. Missing `authenticated_owner` and `runsc` rows are not invented. The producer decides admissibility.

`production_ready`, `customer_gate_verified`, `owner_authenticated`, and `runsc_verified` are written as SQL `FALSE`. A producer body that claims any of them true is not stored as success.

## Rollback

`rollbackRelease` keys are exactly `project_id`, `identity_revision`, `idempotency_key`, and `expected_version`. The path operation must be the active publish. Before `recover` with action `rollback`, the service checks that the restored column set is a subset of the active column set and that both publishes share role, policy, knowledge, catalog version, and review admission. An empty base is a subset. A wider restored pack is `409` `compat.refused` and the producer is not called. The check runs again in the worker. The producer then moves the pointer and writes its `rolled_back` audit in its transaction. This service writes its own audit in the result transaction. A refused producer rollback leaves the pointer where it was.

## Reads

`getActiveRelease` reads `oo_release_control.publication_status` as `oo_release_reader`. It does not use the admin login for that select. `getActiveRolePack` calls producer `status` and returns path, sha256, and byte length for the sealed role only. File text is not stored and is not returned. `certificate_class` on a test-scope active row is `test_scope_not_production`.

## Restart and failure

On process start, `running` operations become `resume_uncertain`. An `applying` candidate is sent to producer `recover`, which sets `failed` without moving the pointer. `queued` work is eligible again. `resume_uncertain` is not activated. A missing gate, a refused plan, a production block, or a compat refusal does not change the active pointer.

## PostgreSQL

`migrations/oo_release_api_v1.sql` runs once in one transaction. The manifest stores that file's SHA-256. The same hash replays. A different hash is install drift and is not repaired. Audit rows are insert-only. The script grants no data role and creates no login.

`synchronous_commit` is on. The API login is `oo_release_api_admin`. The producer login is `oo_release_admin`. The publication login is `oo_release_reader`. Passwords and CA bytes come from files named by the config. They are not argv, environment, or response fields.

## Commands

```text
release-control-api install --config CONFIG
release-control-api serve --config CONFIG
```

| Condition | Exit |
|---|---|
| malformed argv or config | 1 |
| refused business rule | 2 |
| installed or listening | 0 |
| commit failed after the transaction began | 3 |

Stdout of install, and the first stdout line of serve, is one JSON object. It contains no cookie, CSRF token, password, path, or argv.

## Dependencies

Direct pins already in the repository root lock, plus path package `release-coordinator`. No new crate version, no root manifest edit, no root lock edit, no `cargo update`. `rust-version` is 1.94.0. Safe Rust only.

## Not claimed

Customer approval, authenticated owner, runsc, replica production admissibility, a production certificate, SP08 acceptance, and a second gate engine.
