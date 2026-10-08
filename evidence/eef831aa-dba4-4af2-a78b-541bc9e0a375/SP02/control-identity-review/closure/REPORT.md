# Control identity closure

Verdict: **APPROVE_LIMITED original E54**.

The child is `tools/control-identity` at `a6f92da8a0bf872f8a30efb116c2c7b14463505b`. At this writing, `git diff a6f92da8a0bf872f8a30efb116c2c7b14463505b HEAD -- tools/control-identity` is empty (`HEAD` `95c33fca0b22b38a2e08c08bb5e2a050f0bf44ec`). This packet does not accept a customer IdP, the SP02 parent, or the full goal. Production customer IdP stays pending. The private HTTPS test issuer is the evidence issuer.

The original withhold stays in the parent `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`. This directory does not rewrite that history.

## Evidence combination

Two independent measurements, one component diff.

| Piece | Commit | Hash | What it covers |
|---|---|---|---|
| 68 cases, 0 failed, historical verdict `WITHHOLD` | `35bd6b8157cecd89f2ae085b012b4b0137562ddb` | `independent-proof.json` `f917a652b0fcec042699852ea2a3d43d109ae3c5e40a3952f184eebb8ae82438`, verdict `d880d5aac37b0bdf5ec323addf7c640da1b6abaf8982e245df918f512ba00c3b` | Every check except the split-audience failure |
| Audience recheck, verdict `FINDING_CLOSED` | `a6f92da8a0bf872f8a30efb116c2c7b14463505b` | `audience-proof.json` `5d748b03a1598c8cd7a94b4be4846f2adfa38a74065a7017ee02f8cfcd4829df`, verdict `4ab864658ddc2862ebc71db7e8fa3dd17f30943e85b64bc1988314f3baf00da1` | The one behavior the diff changes |

The 68-case files were hashed again for this closure and still match. They were not rerun.

## Why the 68 cases still apply

`git diff 35bd6b8157cecd89f2ae085b012b4b0137562ddb a6f92da8a0bf872f8a30efb116c2c7b14463505b -- tools/control-identity` is five files: `CONTRACT.md`, `src/crypto.rs`, `src/oidc.rs`, `tests/live/idp.rs`, `tests/live/main.rs`. Schema, OpenAPI, migration, `http.rs`, `store.rs`, `guard.rs`, `cookie.rs`, `config.rs`, `sql.rs`, and both lockfiles are outside that diff. The two archived `Cargo.lock` files share hash `1c078aa1ef4f877d43d693cc1aaecbf48317952f1359ce8f126ac6f9e27ddb40`. OpenAPI and `schema/` are byte-identical between the two archives. The root-registry result on the old packet remains count 225, missing 0, for those lock bytes.

The runtime change is one comparison. `claims` used to require `aud` to contain `binding.audience`. It now requires `aud` to contain `binding.client_id`. `Oidc` and `TokenBinding` no longer store `audience`. The `azp` match, issuer check, nonce check, time check, signature check, and the `IdentityClaims` return value are the same lines. `config_commit` still includes the configured audience because `config.rs` is unchanged, and the new probe loaded a file where `audience` was `configured-aud` and `client_id` was `rp-client`.

The 68-case session matrix called `write_config` with audience `rp-client`, and `client_id` is the same `rp-client`. On that configuration the old predicate and the new predicate accept and reject the same tokens. Those cases stay evidence for the new commit. The split-audience probe is the only case whose result changes. On `35bd6b81` it recorded `binding_holds=false` (`aud=rp-client` was 401, `aud=configured-aud` was 302). That result stays the historical withhold. It is not reused as a pass.

## Replacement measurement

On `a6f92da8a`, a private HTTPS issuer served code, token, and JWKS and signed RS256. Signature verification still runs before `claims`. JWKS was fetched once. Library tests on that archive: 11 passed, including the changed audience unit test and the signature-tamper assertion. Clippy `--all-targets -D warnings` exited 0. Live and cli suites were not repeated.

| Leg | HTTP | Result |
|---|---|---|
| `aud=other-aud`, `azp=rp-client` | 401 | `audience.rejected` |
| `aud=rp-client`, `azp=rp-client` | 302 | current 200, subject `fixture-admin` |
| `aud=configured-aud`, `azp=rp-client` | 401 | `audience.rejected` |
| `aud=rp-client`, `azp` absent | 302 | current 200, subject `fixture-admin` |
| `aud` `rp-client` and `extra`, `azp=rp-client` | 302 | current 200, subject `fixture-admin` |
| `aud` `rp-client` and `extra`, `azp` absent | 401 | `audience.rejected` |

That login is a real authorization-code, PKCE, state, and nonce success under the new rule. The current body carries `authority=management_session`, `data_grants=none`, `session_class=management_cookie`, and `not_issued` for owner certification, agent session, harness session, and gateway token. `projects` is empty because this probe did not consume bootstrap. Role assignment after bootstrap stays the 68-case evidence.

## Acceptance checks

Each row is one original E54 check. "Reuse" means the implementing file is outside the diff, or the check is inside `claims` but not the changed comparison, and the 68-case run used `audience=client_id`.

| Check | Source of the requirement | Evidence | Reuse reason |
|---|---|---|---|
| `iss` exact | REQ-CTL-03, frozen ID Token row | 68-case `issuer` | Issuer comparison unchanged |
| `aud` contains `client_id` for every loaded configuration | OIDC Core 1.0 §3.1.3.7, corrected contract sentence | a6f92 audience proof, all six legs `ok` | This is the diff. The old split result is history, not a pass |
| `azp` present equals `client_id`; multiple `aud` requires that `azp`; single `aud` may omit `azp` | Same clause and contract row | 68-case `bad_azp`, `multi_aud_without_azp`, `single_aud_client_id_without_azp`; a6f92 repeated the single and both multi legs on the split configuration | `azp` match arm unchanged; split legs remeasured because they sit next to the new comparison |
| State | REQ-CTL-03 | 68-case `state_rejected`; a6f92 login returned 302 | State handling is outside the diff; the new login still presented state |
| Nonce | REQ-CTL-03 | 68-case `bad_nonce`; a6f92 login returned 302 | Nonce hash check unchanged |
| Redirect allowlist | REQ-CTL-03 | 68-case `foreign_redirect_query`, `redirect_rejected` | `http.rs` outside the diff |
| PKCE and authorization-code replay | Plan SP02-W05, AT-CTL-05 replay | 68-case `pkce_rejected`, `oauth_callback_replay`; a6f92 login is a real PKCE success | PKCE code outside the diff |
| RS256 signature, `alg=none` | Frozen ID Token row | 68-case `alg_none`, `bad_signature`; a6f92 lib test rejected a tampered RS256 token; a6f92 login verified a real signature | Signature check is before `claims` and was not edited |
| Expiry, future `iat`, idle expiry, OAuth expiry | Frozen time row, session row | 68-case `token_expired`, `future_iat`, `idle_expiry`, `oauth_expiry` | Time checks and store expiry outside the diff |
| JWKS duplicate `kid`, unknown `kid`, bounded rotation | Contract JWKS paragraph | 68-case `jwks_ambiguous_one_fetch`, `jwks_unknown_kid_two_fetches`, `jwks_rotation_bounded` | JWKS cache code outside the diff. a6f92 still fetched JWKS |
| Cookie attributes | REQ-CTL-03 | 68-case wire cookies; a6f92 lib `cookie_attributes_are_host_only` | `cookie.rs` outside the diff; lib tests were rerun with the new tree |
| CSRF and Origin failures leave the session | REQ-CTL-03 | 68-case `csrf_missing_does_not_revoke`, `session_survives_bad_csrf`, `origin_rejected`, `session_survives_bad_origin` | `http.rs` outside the diff |
| Durable session, logout, restart survival, restart revocation | REQ-CTL-03, freeze durable cookie session | 68-case `restart_session_survives`, `logout`, `current_after_logout`, `restart_revocation_holds`, `login_after_revocation` | Store and HTTP logout outside the diff. New-rule session creation is the a6f92 current 200 |
| Bootstrap random, single-use, short, first initialization only, subject mismatch does not burn, no default admin password | REQ-CTL-03, AT-CTL-05 | 68-case `bootstrap_issue`, `bootstrap_subject_mismatch`, `bootstrap_mismatch_does_not_burn`, `bootstrap_superseded`, `bootstrap_expired`, `bootstrap_replay`, `bootstrap_unavailable`, `bootstrap_query_not_in_idp_redirect` | Bootstrap SQL and HTTP outside the diff |
| Roles from operator bindings; token `role` / `roles` / `groups` / `data_grants` / `authorized` do not grant management | AT-CTL-05 least privilege, freeze management binding | 68-case `admin`, `reviewer`, `steward`, `auditor`, `business_owner`, `role_claims_do_not_grant_admin`, `unbound_role_claims` | `guard.rs` outside the diff. `claims` still returns only `subject` |
| Cross-project and revision conflict | Plan stop: cross-project rejected | 68-case `project_and_revision_guard`, `revision_conflict`, `current_after_revision_restore` | Guard and store outside the diff |
| Caller JSON is not a session | Freeze: no self-asserted role | 68-case `caller_json_post_current`, `caller_json_logout`, `caller_json_library`; a6f92 lib `caller_json_is_never_a_session` | `guard.rs` outside the diff; lib test rerun |
| Envelope is management session, not data, owner, agent, harness, or gateway | Freeze | 68-case current and logout bodies; a6f92 current bodies | Response builders outside the diff |
| `config_commit`, redirect, client, secret file | Freeze issuer/client/redirect/config commit/SecretRef | 68-case install; a6f92 `split_config` and install `committed` with audience distinct from `client_id` | `config.rs` outside the diff. The new load proves a distinct audience still commits |
| Root registry subset | Review packet | 68-case `root_registry_subset` count 225, missing 0 | Child lock hash unchanged |
| Response schemas | Review packet | Both archives, OpenAPI and `schema/` identical | Those files are outside the diff |
| Table-owner `TRUNCATE` | Retained note | 68-case `table_owner_truncate_note` | Migration and `sql.rs` outside the diff. Not rerun. Not a withhold |
| Customer IdP | Plan SP02-W05 stop | Not claimed on either proof | Pending production IdP does not withhold this child. Test IdP evidence is the limited acceptance |

## What this closure did not run

No new container, database, or 68-case campaign. No product edit. The diff-affected success and rejection paths already have an independent HTTPS proof on `a6f92da8a`. Repeating logout, bootstrap, role, JWKS rotation, and restart on that commit would retest files the diff does not change.

## Limits that remain

Customer integration stays `not_claimed`. The SP02 parent stays unaccepted. The full goal stays unaccepted. The table owner can `TRUNCATE` `audit_event`. The service does not issue `TRUNCATE`. That note stays inside this limited acceptance.
