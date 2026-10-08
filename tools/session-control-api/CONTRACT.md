# session-control-api-v1

Frozen before implementation. SP11-W01 / B08 own-session create, event, and close API.
This crate is not SP11 phase acceptance and it does not sign a production release.

Write boundary: `tools/session-control-api/**` and
`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a387/**`.
It calls the fixed `session-store`, `session-transition`, `session-release`, and
`gateway-token-store` crates. It does not edit them.

## Deployment

| Profile | Listen | Close | `production_signed` |
|---|---|---|---|
| `localprocess` | yes | only after the checks below | always false |
| `production` | no | no | always false |

`production` returns `runtime_unavailable` or `production_driver_absent` and does not listen.
A missing `runsc` does not switch a production request into localprocess.
localprocess is an explicit dev profile. It is not a production signature.

`reducer_physical_release_certified` is reported from the reducer. This crate cannot flip that flag.
A closed localprocess receipt still reports it false.

## Authority

The operator bindings file is the only identity source.
A JSON body that contains `principal_id`, `project_id`, `role`, `claims`, `claims_hash`, or `identity` is `self_asserted_rejected`.
The cookie `oo-entry` is an opaque 256-bit value. PostgreSQL stores only its SHA-256.
Mutations require `X-OO-CSRF` equal to the token returned once by consume.
This is not control-identity and not a customer IdP. `customer_integration` is `not_claimed`.

## HTTP

Contract string `session-control-api-v1`. JSON only. Secrets are not response fields.

| operation | method and path | success |
|---|---|---|
| `consumeEntrySession` | `POST /control/v1/entry/session/consume` | 200 and `Set-Cookie` |
| `getEntryCurrent` | `GET /control/v1/entry/session/current` | 200 current principal and project |
| `createConversation` | `POST /control/v1/conversations` | 201 ready session |
| `listConversations` | `GET /control/v1/conversations` | 200 own rows only |
| `getConversation` | `GET /control/v1/conversations/{id}` | 200 own detail |
| `listEvents` | `GET /control/v1/conversations/{id}/events` | 200 bounded sequence, limit at most 50 |
| `beginTurn` | `POST /control/v1/conversations/{id}/turns` | 200 running turn |
| `cancelConversation` | `POST /control/v1/conversations/{id}/cancel` | 200 only when phase is `closed` |
| `getOutput` | `GET /control/v1/conversations/{id}/outputs/{name}` | 200 only when the current binding still matches |
| `terminateConversation` | `POST /control/v1/conversations/{id}/terminate` | admin capability; same close rule |

Status codes: 401 unauthenticated, 403 not owner or permission changed, 404 missing, 422 malformed, 409 incomplete and retryable, 503 store unavailable.
409 is never a closed session. 200 cancel requires `phase=closed`, `production_signed=false`, `deployment=localprocess`, eleven steps, S8 `actual_absence=true`, and a live recheck that the workspace and the harness pid are absent.

## Create path

Consume binds the grant to one operator binding.
Create bootstraps `session-store` and applies `begin_bind` and `complete_bind` through `session-transition`.
It takes one pool slot, holds one permit (maximum two held permits for one principal), inserts one proxy-mapping row, and issues one Gateway session token through `gateway-token-store`.
Begin turn applies `begin_turn`, writes the transcript and the output file, and starts `pg_sleep` on `oo_fixture_query`.

## Close path

Cancel calls `session_release::run` for S1, S2, S3, S4, and S8.
Before that call it revokes the Gateway session token, marks the mapping deleted, and releases the permit, so those credentials are not still live during collection.
After `session_release` returns exit 0 with phase `releasing` and those five reducer steps confirmed, this crate records:

| Step | Rule |
|---|---|
| S5 | `skipped` only when snapshot is disabled and `symlink_metadata` on the snapshot path is `NotFound`. A present snapshot is incomplete. `actual_absence` stays false. |
| S6 | `confirmed` only after `gateway-token-store` revoke and a lookup that returns `revoked`. |
| S7 | `confirmed` only when the mapping row for this session is no longer `present`. |
| S9 | `confirmed` only when the used slot path is `NotFound` and a new empty slot exists. The used slot is not returned to the pool. |
| S10 | `confirmed` only when this session holds zero permits. |
| S11 | `confirmed` only after one audit row insert. Then `complete_release`. |

`complete_release` runs only after a second observation that the workspace is `NotFound`, the harness pid is absent from `/proc`, S8 in the reducer has `actual_absence`, and S6 lookup is still `revoked`.
Anything else stays `incomplete`, `retryable=true`, phase not forced to `closed`.
A missing runtime, a failed `session_release`, or an unproven pid is that incomplete result. It is not a receipt of success.

A later process may finish after the fixture stage is durable. It reads PostgreSQL and the fsynced checkpoint. It does not invent steps that the reducer does not already show or that it cannot observe.
A crash inside `session_release` before S1 is confirmed stays incomplete, because that crate will not adopt a live pid and will not treat a missing pid as absence.

## Executable

`session-control-api install`, `issue-grant`, and `serve`.
The process accepts one subcommand and a JSON document on stdin. Passwords are fields of that document. They are not argv and not environment variables.
Exit 0 is install/issue success or a listening server. Exit 2 is a refused profile or business rule. Exit 1 is malformed input. Exit 3 is `commit_unknown` or a connection failure. Serve does not exit 0 because a session reached `releasing`.

## Out of scope

Production `runsc` certification, Valkey, object-storage TTL, control-identity OIDC, and a customer IdP.
Those gaps stay uncertified. They are not filled with a confirmed step.
