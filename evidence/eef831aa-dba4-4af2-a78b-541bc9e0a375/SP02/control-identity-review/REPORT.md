# Control identity review E54

Verdict: **WITHHOLD original E54**.

Clause: OIDC Core 1.0 §3.1.3.7. The ID Token `aud` claim must contain the relying party's `client_id`. This build accepts `aud` when it equals the configured audience and rejects `aud` when it equals `client_id` and those two strings differ.

This packet does not accept a customer IdP, the SP02 parent, or the full goal.

## What was measured

Product tree `tools/control-identity` matches commit `35bd6b8157cecd89f2ae085b012b4b0137562ddb` (28 files, 0 differences). `CONTRACT.md`, `openapi/control-identity.openapi.yaml`, and `schema/` match contract commit `089b00d7f70da1bc42f7e201dfa070748c651d5b` (9 files, 0 differences). `CONTRACT.md` sha256 is `35f9c4a1fccf08b695848c6e6291f191a392cf8ecd079490f80855a773035732`.

The archive was mounted read-only. `archive-before.sha256`, `archive-after.sha256`, and `archive-after-rerun.sha256` are identical (`archive-immutable.exit` 0, `archive-immutable-rerun.exit` 0). Product bytes were not edited.

Linux arm64 smoke, before the independent test was copied into the work tree: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, `smoke.exit` 0. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Builder limits: 6 GiB, 1 CPU, `cap-drop ALL`, `no-new-privileges`, private network, no host port. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, 1 GiB, 1 CPU, no host port. `init-roles.txt` is `ok`.

Author tests on that tree, locked and offline: library 11 passed, `usage_is_silent` 1 passed, `live_identity_boundaries` 1 passed. `cargo-test-author.exit` 0. Author Clippy `--all-targets -D warnings`: `cargo-clippy-author.exit` 0.

Independent oracle, final pass: 68 cases, 0 failed, `cargo-test-independent.exit` 0, `cargo-clippy-independent.exit` 0, `rerun-independent.exit` 0. The first full `run-proof.sh` exited 1 because the oracle did not compile. Those logs are in `linux/raw/failed-attempt-1/`. The final verdict file is `linux/raw/independent-proof.verdict` and contains `WITHHOLD`.

## Audience measurement

Split configuration: `audience` `configured-aud`, `client_id` `rp-client`. The issuer signed real RS256 ID Tokens. JWKS served the RSA public key.

| Token `aud` | `azp` | HTTP | Code |
|---|---|---|---|
| `other-aud` | `rp-client` | 401 | `audience.rejected` |
| `rp-client` (the `client_id`) | `rp-client` | 401 | `audience.rejected` |
| `configured-aud` | `rp-client` | 302 | session issued |

`GET /control/v1/identity/current` after the configured-audience login returned 200 for subject `fixture-admin`. `binding_holds` is false. The same binary, with `audience` set equal to `rp-client`, rejected `other-aud` (`bound_arbitrary_aud`, 401 `audience.rejected`) and accepted a single audience of `rp-client` with no `azp` (302).

Frozen contract row, ID Token rules: "`iss` exact, `aud` contains the configured audience, multiple audiences require `azp` equal to `client_id`, a present `azp` equals `client_id`". The code follows that sentence. It does not follow OIDC Core 1.0 §3.1.3.7 when the configured audience and the `client_id` differ.

## Checks that passed on this build

Login, current, logout, and bootstrap consume were driven over HTTPS against the private PostgreSQL 17.10 database. Cookies observed on the wire were `__Host-oo-login` and `__Host-oo-session` with `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/`, and no `Domain`. CSRF failure and a foreign `Origin` returned `csrf.rejected` and `origin.rejected` and left the session usable. Bootstrap query material was absent from the IdP redirect and from the final redirect. Subject mismatch left the token `issued`. Supersession, expiry, replay, and a second issue after initialization returned `bootstrap.rejected`, `bootstrap.expired`, `bootstrap.replay` (409), and `bootstrap.unavailable` (exit 2).

PKCE verifier tamper, state tamper, and callback replay returned `pkce.rejected` or `state.rejected`. Foreign `redirect_uri` returned `malformed`. `return_to=//evil` returned `redirect.rejected`. `alg=none`, a flipped RS256 signature, a foreign `iss`, a bad `azp`, multiple audiences without `azp`, a bad nonce (400 `nonce.rejected`), expiry, a future `iat`, and an IdP `access_denied` error were rejected with the codes recorded in `independent-proof.json`.

JWKS: duplicate `kid` produced one fetch and `kid.ambiguous`; an unknown `kid` produced two fetches and `kid.unknown`; rotation from key 1 to key 2 produced two fetches and HTTP 302. A session survived process restart. Logout remained revoked after restart. Idle expiry and OAuth expiry were applied by timestamp updates and returned 401.

`Store::authenticate` plus `authorize` allowed `project.admin` on project `alpha` for the admin binding, returned `forbidden` for project `beta`, and returned `revision.conflict` for `revision + 1`. An HTTP current after a SQL revision bump returned 409 `revision.conflict`. Restoring the row to revision 7 made current return 200, and logout then returned 200 `revoked` and cleared the session cookie.

Role logins for admin, reviewer, steward, auditor, and business_owner matched the frozen capability matrix. An auditor ID Token carrying `role`, `roles`, `groups`, `data_grants`, and `authorized` still presented project `delta` role `auditor`. `authorize` of `project.admin` for that session returned `forbidden`. An unbound subject carrying the same claims received an empty `projects` array. Envelope constants on current, logout, and error bodies stayed `authority=management_session`, `data_grants=none`, and `not_issued` for owner certification, agent session, harness session, and gateway token. `session_class` on current was `management_cookie`.

Caller JSON: `POST /control/v1/identity/current` with `{"authorized":true,"role":"admin"}` returned 405 `method.not_allowed` and set no session cookie. `POST /control/v1/auth/logout` with `{"authorized":true}` returned 400 `malformed` because logout requires an empty body, and set no session cookie. `VerifiedSession::try_from_caller_json` returned `unauthenticated`.

Root `Cargo.lock` contains every registry `name`/`version`/`checksum` triple from the child lock. The child registry count is 225 and `missing` is 0.

Archived actual bodies are under `responses/actual/`. Copied schemas and the OpenAPI file are under `responses/schema/`. `csrf_token` values in the archived current bodies are the literal `REDACTED`.

## Retained limitation

`pg_get_userbyid` reports `audit_event` owner `oo_identity_admin`. `pg_get_triggerdef` is `BEFORE DELETE OR UPDATE` and does not mention `TRUNCATE`. The table owner can `TRUNCATE`. This review recorded that note and did not run `TRUNCATE` or any other database campaign. The service SQL does not issue `TRUNCATE`.

## Resources

The PostgreSQL recipe reuses `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/` from commit `a587031c719a484830d78e493605f48e2056cae4`. `pg-wrap.sh` sha256 `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f` is unchanged. `up.sh` origin sha256 `96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45`; this copy changes resource names only. Owned containers, volumes, the network, and `/tmp/oo-a375-eef831aa-dba4-4af2-a78b-541bc9e0a375` were removed after the proof. See `linux/raw/owned-cleanup.txt`.
