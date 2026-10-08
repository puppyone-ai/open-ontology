# FINDINGS

Bound to the green run in `linux/build-1/console.log` and `linux/proof/private-pg.json`. A green test does not accept SP08.

## CSRF and the frozen identity sentences

`CONTRACT.md` says every request forwards only the session cookie to exactly `/control/v1/identity/current`, and that a mutation's `X-OO-CSRF` equals the current envelope `csrf_token`. Identity `Store::current` stores a new CSRF hash on every GET and returns the new plaintext. The previous token is then dead. Comparing the caller's header with the token returned by that same GET cannot succeed.

The mutation path therefore does two calls, and the contract file was not edited:

1. The caller's `Origin` must be one of `deployment_origins`. A mismatch is `403 origin.rejected` and does not call identity.
2. The API posts `/control/v1/auth/bootstrap/consume` with `Origin` set to the configured identity origin, the caller's cookie, the caller's `X-OO-CSRF`, and a fixed unknown 43-character bootstrap token. Identity checks origin and the stored CSRF hash before it classifies the token. A `bootstrap.*` code means those checks matched and the stored hash is unchanged. Any other refusal is `403 csrf.rejected`.
3. The API then GETs `/control/v1/identity/current` and requires `identity_revision` to equal `revision`. That GET rotates the CSRF for the next mutation.

GETs still call only `current`. The live proof recorded `origin.rejected`, `csrf.rejected`, and `revision.conflict` before any operation row, then succeeded on later mutations that refreshed `current` immediately before the POST. A client must GET identity `/current` after any product GET and use that new token on the next mutation.

`test_issuer` did not become customer approval. The proof field `customer_approval` is `not_issued`. `customer_gate_verified` stayed false.

## Gates that do not publish

Omitted G7 returns `refused` / `not_ready` and leaves the pointer null.

A fence with `sessions_terminated=true` is stored by the producer as `fabricated_session` and the plan reason includes `stale_fence`. The operation reason in the proof is `not_ready`. The pointer stays null.

Production scope, with a `test_issuer` customer row and no owner or runsc store, returns `refused` / `production_blocked` / `not_issued`. The pointer does not move on an empty database or while `ver-n` is active. The live test required `authenticated_owner_missing`, `runsc_missing`, `customer_not_production_admissible`, `fence_not_production_admissible`, and `replica_not_production_admissible`. Unmet production conditions do not auto-publish.

A legal private test-scope activation is `certificate_class=test_scope_not_production`. The four production flags and `full_sp08_accepted` stay false. Rollback success is `not_issued`.

## Rollback permission

Publish may widen columns. Rollback may not. `ver-w` rolled back to `ver-n`. After a later narrow `ver-n2`, rollback was `409 compat.refused` detail `permission` and wrote no operation. The check runs before the producer call. The subset rule is the sorted visibility columns, the same role, the same policy, knowledge, and catalog versions, and `review_admitted` on both sides. An empty restored base is allowed only when the active version is `review_admitted`.

## Restart and scope

A running row with no candidate becomes `resume_uncertain` on the next serve and does not call recover. A queued publish inserted while serve is down completes after restart. The idempotency replay of a finished publish still returns 202 after the pointer has moved. A different body for the same key conflicts. A missing operation and another project's current both return `403 forbidden` detail `scope`.

The reader role can read `publication_status`. `oo_release_api_admin` cannot (`42501`). API audit updates return `42501`.

## What stays false

There is no real owner store, no replica production evidence, and no runsc store in this proof. Those production gates stay failed. There is no PITR proof and no customer IdP. The bootstrap login is an explicit test fixture. `apps/web` was not edited, so the release admin pages remain unbound.
