# oo.selfbuild-control-api/v1

E56-D1/1. Schema `oo_selfbuild_api`. This file, `openapi/selfbuild-control-api.openapi.yaml`, and `schema/` were frozen before the service.

This is the authenticated management API for one self-build and review workflow. It is not a UI, not a harness, not Git publication, not an active release, and not a PostgreSQL `GRANT`. `compiled_pack` is not semantic publication and not the active version. A green test does not accept SP07.

The child review ledger stays `authenticated: false`, `authority: not_verified`, and `management_identity: not_claimed`. This service says `management_identity: verified` only after `getIdentityCurrent` from the configured control-identity server returns a typed success envelope. A caller JSON body, a header role, or `{"authorized": true}` is not that envelope.

## Frozen dependencies

| Product | Fixed commit | Use |
|---|---|---|
| control-identity contract and schemas | `089b00d7f70da1bc42f7e201dfa070748c651d5b` | On-wire adapter only. Pinned copies live in `schema/identity-*.pinned.json`. The live a370 implementation is not a source dependency. |
| pg-builder and combined pack | `be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba` | `catalog` inputs, `profile`, `comments`, `seal`, `cards`, `generate`, `pack-combined` |
| selfbuild-rules | `e5427c1a18e2ddbb480791d214c76888eb7ca998` | S2–S4 |
| selfbuild-suggestions | `d22187ea0fce8e420323660841f2da3b447d5de8` | S6–S8, generated and unmerged |
| review-ledger | `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b` | S9/S10 durable decide path |

Description, core, and source pins stay read-only. This crate does not path-depend on a moving workspace. Accepted binaries are archived under this task's evidence after they are built from those commits.

Public authentication against the real identity service waits until that fixed service is delivered. This crate does not stub, mock, or accept a JSON file as authority. The adapter can be written and schema-checked against the pinned identity documents now.

## Operations

The operation IDs below stay frozen. Overlay routes are additive and use the same identity, CSRF, and error envelope. Anything else, including `POST /control/v1/builds/{build_id}/publish`, is `not_found` or `method.not_allowed`. There is no publish, rollback, grant, or secret-read route.

| operationId | Method and path | Success |
|---|---|---|
| `createBuild` | `POST /control/v1/builds` | 202 queued operation |
| `listBuilds` | `GET /control/v1/builds` | 200 |
| `getBuild` | `GET /control/v1/builds/{build_id}` | 200 |
| `cancelBuild` | `POST /control/v1/builds/{build_id}/cancel` | 202 |
| `getBuildCards` | `GET /control/v1/builds/{build_id}/cards` | 200 |
| `getBuildCandidates` | `GET /control/v1/builds/{build_id}/candidates` | 200 |
| `listReviewItems` | `GET /control/v1/reviews` | 200 |
| `getReviewItem` | `GET /control/v1/reviews/{item_id}` | 200 |
| `previewReviewBatch` | `POST /control/v1/reviews/preview` | 200, no writes |
| `decideReviewItem` | `POST /control/v1/reviews/{item_id}/decision` | 200 |
| `listClassifications` | `GET /control/v1/classifications` | 200 |
| `previewClassifications` | `POST /control/v1/classifications/preview` | 200, no writes |
| `confirmClassifications` | `POST /control/v1/classifications/confirm` | 200 |
| `compileBuildPack` | `POST /control/v1/builds/{build_id}/pack` | 200 |
| `getBuildPack` | `GET /control/v1/builds/{build_id}/pack` | 200 |
| `getOverlay` | `GET /control/v1/builds/{build_id}/overlay` | 200 |
| `postMaintenance` | `POST /control/v1/builds/{build_id}/maintenance` | 200, `llm_calls` 0 |
| `postCurated` | `POST /control/v1/builds/{build_id}/curated` | 200, does not rewrite generated |
| `postReview` | `POST /control/v1/builds/{build_id}/review` | 200, `business_owner_signed` false |
| `postConfirm` | `POST /control/v1/builds/{build_id}/confirm` | 200, still not a business-owner signature |
| `compileRolePack` | `POST /control/v1/builds/{build_id}/role-pack` | 200, `compiled_pack` `not_published` |
| `getRolePack` | `GET /control/v1/builds/{build_id}/role-pack` | 200 |
| `postGates` | `POST /control/v1/builds/{build_id}/gates` | 200, `publication` `refused` |

`cancelBuild` is `POST /control/v1/builds/{build_id}/cancel`, not `DELETE` on the build. `previewReviewBatch` is a static path and is not an item id. A sealed project reads the operator directory named by `sealed_bundle` and does not spawn the foreign binaries. The same HTTP calls serve every source adapter. Sources do not each carry a copy of this service.

A missing dependency returns `503` `dependency.not_ready`. It does not return a successful identity, job, or pack. Errors are JSON and are not HTTP 200. Unknown fields, duplicate JSON keys, and trailing bytes are `400` `malformed`.

## Identity

Every request is checked against the operator-configured control-identity origin. The path is exactly `/control/v1/identity/current`. The origin is `https` with no userinfo, path, query, or fragment. The service forwards only the opaque `__Host-oo-session` cookie. It uses the configured CA file, pins that origin, and does not follow redirects. A caller URL is ignored.

Authority is only a typed current envelope with `install_state=initialized`, `authority=management_session`, and the server's `projects`, capabilities, and `revision`. Caller role, project, subject, and `authorized` are not read as authority. There is no identity cache.

Mutations require `Origin` to equal one configured deployment origin (never `*`), header `X-OO-CSRF` equal to that response's `csrf_token`, and `identity_revision` equal to that response's `revision`. A stale revision is `409` `revision.conflict`.

| Condition | HTTP | Code |
|---|---|---|
| No session cookie, or identity `401` | 401 | `unauthenticated` |
| Identity unreachable, non-success, redirect, or untyped body | 503 | `dependency.not_ready` |
| Authenticated, missing scope, or object outside verified projects | 403 | `forbidden` |
| Bad origin or CSRF | 403 | `origin.rejected` or `csrf.rejected` |
| Revision or idempotency conflict | 409 | `revision.conflict` or `idempotency.conflict` |
| Control database or required binary missing | 503 | `dependency.not_ready` |

An object id outside the caller's verified projects uses the same `403` `forbidden` body as a missing id. The body does not say whether the id exists.

## Scopes

Identity capabilities map to these scopes. A reviewer cannot grant or certify. An auditor is read-only. A business owner is limited to the operator object scope and receives no automatic data access. No scope issues a PostgreSQL grant.

| Identity capability | Scopes |
|---|---|
| `project.admin` | `build.create`, `build.read`, `build.cancel`, `cards.read`, `candidates.read`, `review.read`, `classification.read`, `classification.preview`, `pack.compile`, `pack.read` |
| `ontology.draft` | `build.read`, `cards.read`, `candidates.read`, `review.read`, `classification.read`, `classification.preview` |
| `review.decide` | `build.read`, `cards.read`, `candidates.read`, `review.read`, `review.decide` |
| `audit.read` | `build.read`, `cards.read`, `candidates.read`, `review.read`, `classification.read`, `pack.read` |
| `business.record` | `review.read`, `classification.read`, `classification.preview`, `classification.confirm` |
| `identity.current`, `binding.read`, `session.revoke` | none |

`review.decide` does not include `classification.confirm` or `pack.compile`. `classification.confirm` does not include `review.decide`. `project.admin` does not include `review.decide` or `classification.confirm`.

The project id on the route or in the body must be one of the verified session's projects. Owner confirm also requires the object id on that subject's operator scope list.

## Create

`createBuild` body is `project_id`, `identity_revision`, `config_commit`, `source_ref`, `allowance_id`, `llm_secret_ref`, and `idempotency_key`. Those values must match the operator-installed project record and the verified revision. The body has no file path, binary argv, database password, source text, or model prompt.

Success is `202` with `status=queued`. The same key and the same canonical body replay that operation. The same key and a different body are `409` `idempotency.conflict`. The job row and its create audit event commit in one transaction. A lost commit reply is `commit.unknown` and is not retried.

Config files, secret files, source refs, and binary paths stay on the server as regular private files. Responses never contain them.

## Stages

The worker is serial per project. It reads the server-approved source through existing producers. It does not open a second source connection and it does not run two source stages at once.

| Stage | Producer | Bound |
|---|---|---|
| S0 | `oo-pipeline catalog-pg` stdin, then `pg-builder comments` and `seal` | catalog budget `statement_ms=30000`, `lock_ms=1000`, `connect_ms=30000`; child deadline 300s then kill |
| S1 | `pg-builder profile`, then `cards` | after S0 has exited; profile's fixed producer sets `statement_timeout=10s`; this service does not open another source connection |
| S2–S4 | `selfbuild-rules produce` | one sealed catalog, profile, and cards document; `auto_accept` comes from the installed allowance, default `none` |
| S5 | `pg-builder generate` | loopback `http://127.0.0.1:8317/v1/chat/completions`, model `grok-4.7`, bearer file only; a matching stored cache hit is reused and is not a fresh call |
| S6–S8 | `selfbuild-suggestions produce` without `--config` | generated, unmerged, uncertified |
| S9 | human `decideReviewItem` via review-ledger | not completed by the worker |
| S10 | human `confirmClassifications` via review-ledger | not completed by the worker |

`sample_policy` is `none`. Partial stage failures stay stored. Later stages are not marked succeeded. An unavailable or timed-out model leaves a structural draft or an S5 failure. It does not create an approved review, a pack, or a publication.

A project may set server-side `source_protocol`. That object is not part of `createBuild`. It names the capture, compare, and source-semantics protocols, the source and semantics commits, one to eight sealed type-basis files by absolute path and sha256, and up to four schema-audit logs by absolute path and sha256. The HTTP body stays the seven create fields. The selected protocol is included in the project input hash. Bytes that do not match the selected protocol, a basis hash, or a closed OOS type are `sealed.malformed` at the stage boundary. A column whose PostgreSQL OID has no closed OOS equivalent stays unknown, is omitted from generated YAML, and is not approved. `consistency_grade` and `delete_detection` stay unknown when the semantics profile has null. On this path S5 does not treat `structural_draft` as model success. A missing bearer file sets S5 `awaiting` with `detail_code` `model.blocked`, `llm_calls` 0, `builder_spawned` false, cards `ready` false, and no role pack. The service writes `oo.pg-builder.config/v1` and does not spawn. A present bearer file uses the existing `pg-builder` argv: `approve`, then `generate` (`--config`, `--seal`, `--cards`, `--approval`, `--cache`, `--out`), then `curated`. A missing binary is `builder.absent`. Missing seal, cards, catalog, or config inputs are `builder.inputs_absent`. A non-zero exit, a secret leak, or an outcome other than `validated` is `generate.failed` or `curated.failed`. None of those set Ready, a role pack, or publication. When pinned schema audits are present, S0 builds the catalog from the audited DDL and writes the producer seal and cards. A missing bearer still leaves S5 awaiting `model.blocked` and does not call the model. `compiled_pack` stays `not_published`. There is still no publish route.

Restart resumes a checkpoint only when the input hash, project, and config revision still match and the output hash is recorded. A running S5 checkpoint without an output hash is `resume.uncertain` and does not call the model again. A lost control-database commit is not replayed blindly.

Cancellation sets `cancel_requested` in the audit transaction, then kills the child and waits for that child to exit before `canceled`. A kill that does not reap the child leaves the job `cancel_requested`.

Suggestions `authorization.authorized` is that producer's seal flag. It is not this service's management identity.

## Review, classification, pack

Candidate ids are the server's current generation. `complete_command` and pack bytes from the caller are `malformed`. Membership requires the same project, table, column, and source hash as the stored catalog. A review-ledger receipt binds a command hash only. It is not membership proof. This service checks membership before it calls the ledger and refuses on mismatch.

The ledger control file and credential file are rebuilt on the server from the verified subject and the operator scope list. `authority` stays `not_verified` and `management_identity` stays `not_claimed` inside that file, because that is the ledger contract. Credentials are not returned.

`previewReviewBatch` and `previewClassifications` write nothing. `accept_all: true` is `409` `batch.accept_all` and writes nothing. Classification confirm requires every included visible column. Unknown or deferred columns stay hidden. `pii=unknown` is `409` `pii.unknown`. Confirmation does not issue a PostgreSQL grant.

`compileBuildPack` uses stored decisions and the operator role snapshot. It calls `pg-builder pack-combined` once. Producer bytes are kept. `review.state` is not invented. An `UNREVIEWED` prefix already in those bytes stays. The envelope sets `published=false`, `active=false`, and `producer_authenticated=false`. `management_identity` is `verified` only because this request's identity check succeeded, not because the producer said so.

## PostgreSQL

`migrations/oo_selfbuild_api_v1.sql` runs once in one transaction. The manifest stores that file's SHA-256. The same hash replays. A different hash is install drift and is not repaired. The script does not `GRANT` data roles and does not create a customer principal. Audit, decision, and pack rows are insert-only.

The install login is `oo_selfbuild_api_admin`. `synchronous_commit` is on for these transactions.

## Commands

```text
selfbuild-control-api install --config CONFIG
selfbuild-control-api serve --config CONFIG
```

| Condition | Exit |
|---|---|
| malformed argv or config | 1 |
| refused business rule | 2 |
| installed or listening | 0 |
| `commit.unknown` | 3 |

Stdout is one JSON object and contains no cookie, CSRF token, password, bearer, path, or argv.

## Dependencies

`rust-version` is 1.94.0. Safe Rust only. The root `Cargo.toml` and root `Cargo.lock` stay untouched. This crate is not published and it does not add a new public package.

After `ed16eb71a48c7e0c4a1c65c3f601a45077742922`, the coordinator allowed this crate's own `Cargo.toml` and `Cargo.lock` to path-depend on the existing `oo-core` at fixed commit `983577c9b89453b08ab4e1a02cab88df78c3e005` (it contains the `8397fd5b984df364cc3638ffa1e69ea06e125d74` source-bound derivation). The path is `../../../crates/oo-core`. It resolves when this crate is overlaid at `stage/w1/selfbuild-control-api` inside a read-only git archive of that commit. The archive's `crates/oo-core` is the compile input. Dirty workspace HEAD is not. Locked dependencies are the versions that path already requires. They are downloaded at those admitted versions and recorded in this crate's lock together with the core SHA and the archive hash. This replaces the earlier blanket ban on a path dependency. It does not relax OOS types, PII approval, curated overlay, or publication. `compiled_pack` remains `not_published`.

The same overlay calls the existing `pg-builder` already in that archive. The path is `../../../tools/pg-builder`. `seal_request`, `assemble_cards`, and `import_seal` are the functions whose source matches that commit. This crate does not copy them and does not edit `tools/pg-builder`. A capture-check file is not retagged. With pinned schema audits and a sealed type basis, S0 builds a catalog from the audited DDL and calls `seal_request` and `assemble_cards`. Without those audits the capture stays a compiler refusal and does not become `seal.json`. `oo.pipeline.native-sealed-catalog/v1` is not called. The root `Cargo.toml` and root `Cargo.lock` stay untouched.

A measured source dictionary uses the same `source_protocol` object. `catalog` is `oo.source.<engine>/v1`, `profile` and `semantics` are the protocols of the measured profile and audit bytes, and `type_basis` may be empty. The bytes are not rewritten into `oo.postgres-preview.capture-check/v1`. `oo.source.pgsql/v1` calls `seal_request` and `assemble_cards`. Another `oo.source.*` dictionary is not sent to that sealer and is not retagged. A type name maps only when it is already a closed OOS type. `bytea` stays null. A source identifier such as `_id` is kept and is not renamed. Unknown grade, unknown delete, and unknown PII are not approved. `compiled_pack` stays `not_published`. A missing model bearer leaves S5 `model_blocked`.

## Still required

The fixed a370 identity service and its OIDC test IdP, customer IdP confirmation, full role permission, publication, harness, and the rest of SP07. This contract does not close those.

### Native source generation maintenance

`POST /control/v1/builds/{build_id}/maintenance` accepts trigger
`source_generation_advanced` with exactly `identity_revision`, `idempotency_key`,
`trigger`, and `catalog_sha256`. The SHA pins `catalog.json` in the project's
operator-configured sealed bundle; HTTP cannot supply a catalog or directory.
The existing bundle manifest and source protocol validate the bytes used.
Only a sealed collection with identical source/dataset identities, schema,
types, native semantics, rules, visibility and model settings may reuse the
original reviewed decisions. Generations cannot rewind or change an existing
run's data identity. Structural, semantic, classification or role changes
require a new genuine Selfbuild. The immutable model output remains unchanged.
A successful operation appends a zero-model native-maintenance receipt and
artifacts bound to the original build/decision digest and current collection,
bundle file hashes and per-table binding vector. Role Pack compilation uses the
latest accepted binding and refuses an operator bundle that has since changed
without maintenance. Neither maintenance nor a compiled Pack publishes data.
