# FINDINGS

Bound to `linux/build-1/console.log` and `linux/proof/private-pg.json` from the run whose `git-head.txt` is `18b0fcde61ea269cc27e2dc34a81c07ba413a30e`. A green test does not accept SP08. The earlier consume-probe campaign is archived at `linux/prior-fbd046761/` and is not the binding this packet describes.

## CSRF binding

Identity `a6f92da8a0bf872f8a30efb116c2c7b14463505b` `Store::current` mints a new opaque token on every `GET /control/v1/identity/current`, stores `sha256` of that token in `csrf_hash` via `SLIDE_SESSION`, and returns the new plaintext. The caller's previous `X-OO-CSRF` cannot equal `csrf_token` in that same response.

That commit's HTTP routes are login, callback, current, logout, and bootstrap consume. Logout compares the header with the stored hash and then revokes the session and audits `logout`. Consume compares the hash and, for an unknown bootstrap token, commits a separate audit row `bootstrap_rejected`. `CONTRACT.md` and `src/identity.rs` both refuse those two calls.

The binding this service implements:

1. `Origin` must be one configured deployment origin. The check is local. A mismatch is `403 origin.rejected` and does not call identity. The live test held the admin `csrf_hash` unchanged across that refusal.
2. `X-OO-CSRF` must be one 43-character token of ASCII letters, digits, `_`, and `-`. Missing or ill-shaped is `403 csrf.rejected` and does not call identity. The live test held `csrf_hash` unchanged for `short-token` and for a missing header. Proof steps: `csrf-shape`, `csrf-missing`.
3. The service then sends only the session cookie to `GET /control/v1/identity/current` and requires `identity_revision` to equal `revision`.
4. The service does not compare the header with the returned `csrf_token`.

A 43-character token that is not the stored token is not `csrf.rejected`. Proof step `csrf-unverified` posted that token on rollback of a missing operation, received `403 forbidden` detail `scope`, added no operation row, and observed `csrf_hash` change because `current` ran. After publish, rollback, pack read, and restart, identity audit `bootstrap_rejected` was 0 and `bootstrap_consumed` was 1, the fixture login. Proof step `identity-audit`.

`test_issuer` did not become customer approval. The proof field `customer_approval` is `not_issued`. `customer_gate_verified` stayed false.

## Interface gap for the identity owner

402 is the only writer of `tools/control-identity`. This card does not modify that tree. The host worktree differed from `a6f92` while the proof ran (`linux/build-1/identity.host.diffstat`). The compiler read `git archive a6f92da8a0bf872f8a30efb116c2c7b14463505b tools/control-identity`, 28 files, each compared with `git cat-file` (`linux/build-1/compiled-pins.txt`).

The missing operation is `verifyManagementSession`, `POST /control/v1/identity/verify`:

- Inputs: `__Host-oo-session`, `Origin` equal to the identity `https_origin`, `X-OO-CSRF`, and `revision`.
- Success returns the capability envelope `getIdentityCurrent` already returns, and echoes the presented token.
- The transaction does not update `csrf_hash`, does not slide idle expiry, does not insert `audit_event`, does not consume a bootstrap token, and does not revoke the session.
- `401 unauthenticated`, `403 csrf.rejected`, `403 origin.rejected`, and `409 revision.conflict` commit nothing.

This service binds the stored-hash compare to that route when a later identity commit pins it. Until that pin exists, a well-formed token that does not match the stored hash is not rejected.

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
