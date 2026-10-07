# oo.control-identity/v1

SP02-W05 / REQ-CTL-03 / AT-CTL-05 / E54-D1/1. Schema `oo_identity_control`. This file was frozen before the service, the migration, and the tests.

This crate is the management-identity producer for B01. It is not SP02 acceptance, not a customer IdP integration, not an agent session, not a harness session, and not a public Gateway token issuer. `customer_integration` is `not_claimed` on every response. `data_grants` is `none`. `owner_certification` is `not_issued`. `agent_session`, `harness_session`, and `gateway_token` are `not_issued`. `authority` is `management_session`.

The configured bootstrap binding is an explicit test fixture. It is not customer-owner evidence.

## Surface

These are the only HTTP operations. Anything else is `not_found` or `method.not_allowed`. There is no review, publish, grant, or secret-read route.

| operationId | Method and path | Success |
|---|---|---|
| `startAuthorizationCodeLogin` | `GET /control/v1/auth/login` | 302 to the pinned authorization endpoint |
| `completeAuthorizationCodeCallback` | `GET /control/v1/auth/callback` | 302 to the same-origin return path |
| `getIdentityCurrent` | `GET /control/v1/identity/current` | 200 current envelope |
| `logoutManagementSession` | `POST /control/v1/auth/logout` | 200 after a durable revoke |
| `consumeBootstrapToken` | `POST /control/v1/auth/bootstrap/consume` | 200 after a one-time consume |

OpenAPI and the JSON schemas in this directory are the machine contract. Error bodies use `schema/error.schema.json`.

## Browser

The service speaks HTTPS. `https_origin` is the exact origin, with no path.

| Cookie | Purpose | Attributes |
|---|---|---|
| `__Host-oo-login` | One login attempt. Holds the 256-bit handle and the PKCE verifier. | `Secure`; `HttpOnly`; `SameSite=Lax`; `Path=/`; no `Domain` |
| `__Host-oo-session` | Server session. 256-bit opaque value. | same attributes |

`SameSite=Lax` is required so the cross-site top-level GET from the IdP still presents the login cookie. `SameSite=Strict` would drop that cookie. The session cookie is separate from the login cookie.

PostgreSQL stores SHA-256 hex of the login handle, the state, and the nonce. It stores the PKCE S256 challenge, not the verifier. The verifier exists only in the login cookie until the callback clears that cookie. The session table stores only the session hash. Idle and absolute expiry and revocation are columns on that row.

`getIdentityCurrent` returns `csrf_token` in the JSON body. Cookie-authenticated mutations (`logoutManagementSession`, `consumeBootstrapToken`) require that token in `X-OO-CSRF` and require `Origin` to equal `https_origin`. The CSRF token is not a cookie and is not written to browser storage by this service. A missing or wrong token or origin does not revoke the session.

`return_to` is an optional login query. It must be a same-origin path: one leading `/`, no `//`, no `\`, no `?`, no `#`, no `..`. The callback success `Location` is `https_origin` plus that path and has no query. Unknown login query keys are `malformed`, including `redirect_uri`.

A login query `bootstrap_token` is hashed into the OIDC transaction and is not copied into the IdP redirect or the final redirect.

## OIDC

Authorization Code with `code_challenge_method=S256` only. The code, state, and nonce are single use. The OIDC transaction TTL is the configured `oauth_ttl_seconds`.

Discovery is `GET {issuer}/.well-known/openid-configuration`. `issuer` in the document must equal the configured issuer string. `authorization_endpoint`, `token_endpoint`, and `jwks_uri` must be HTTPS, must have the same scheme, host, and port as the issuer, and must not carry userinfo, a query, or a fragment. The service does not follow redirects. Each outbound call has a five-second deadline and a 65536-byte cap. TLS uses the configured IdP CA file only (`tls_certs_only`). There is no certificate bypass and no plaintext issuer.

The token request is `application/x-www-form-urlencoded` with `grant_type=authorization_code`, `code`, `redirect_uri`, `client_id`, `code_verifier`, and `client_secret`. The secret is read from `client_secret_file`. Verifier, secret, state, nonce, code, and tokens are not argv, environment, logs, or Git.

ID Token rules, checked before a session row is inserted:

| Check | Rejection |
|---|---|
| Exactly three JWT segments, base64url without padding | `token.rejected` |
| Header keys are only `alg`, `kid`, and optional `typ=JWT`. No `jwk`, `jku`, `x5u`, `crit`, or other header | `token.rejected` or `alg.rejected` |
| `alg` is exactly `RS256`. `none`, `HS*`, `PS*`, `ES*`, and any other alg are rejected before verification | `alg.rejected` |
| `kid` nonempty, unique among registered JWKs, `kty=RSA`, `use` absent or `sig`, `alg` absent or `RS256`, no private RSA fields, exponent exactly 65537, modulus 2048..=8192 bits | `kid.unknown`, `kid.ambiguous`, or `jwks.rejected` |
| Signature over the ASCII signing input using `ring::signature::RsaPublicKeyComponents` and `RSA_PKCS1_2048_8192_SHA256` | `token.rejected` |
| `iss` exact, `aud` contains the configured audience, multiple audiences require `azp` equal to `client_id`, a present `azp` equals `client_id` | `issuer.rejected` or `audience.rejected` |
| `exp`, `nbf`, and `iat` are integers. `exp > iat`, `exp - iat <= 900`, skew is `clock_skew_seconds` | `token.expired` or `token.rejected` |
| `nonce` hashes to the stored nonce hash. `sub` is 1..=256 non-space ASCII | `nonce.rejected` or `token.rejected` |

Role, group, and realm claims in the ID Token are ignored. They never become a management role.

JWKS cache maximum age is 60 seconds. An unknown `kid` forces at most one more fetch, then fails. A failed fetch does not extend the old cache. Duplicate `kid` values fail without a refresh.

## Session and roles

A session row binds `subject`, `issuer`, `config_commit`, and `revision`, plus idle and absolute expiry. Restarting the process does not drop the row. Logout sets `revoked_at` and inserts one audit event in the same transaction. If that commit fails, the outcome is `commit.unknown` and the session stays usable. A second logout of a revoked session is `unauthenticated`.

`getIdentityCurrent` lists projects from the operator config bindings for that subject and issuer. Before bootstrap consume, `install_state` is `uninitialized` and `projects` is empty. After consume, each binding contributes one role. One subject has at most one role in one project.

| Role | Capabilities |
|---|---|
| `admin` | `binding.read`, `identity.current`, `project.admin`, `session.revoke` |
| `steward` | `identity.current`, `ontology.draft` |
| `reviewer` | `identity.current`, `review.decide` |
| `auditor` | `identity.current`, `audit.read` |
| `business_owner` | `business.record`, `identity.current` |

No role's set contains another role's set. None of these capabilities is an `oo_r_` data grant or an owner certification. The names `review.decide` and `ontology.draft` exist so a later review API can call the guard. This crate does not serve that API.

`VerifiedSession` is built only from the session cookie and the database. `VerifiedSession::try_from_caller_json` always fails, including on `{"authorized": true}`. `authorize(session, project_id, capability, expected_revision)` checks the project binding, the capability, `install_state=initialized`, and the revision. A stale revision is `revision.conflict` and does not slide the idle timer.

`config_commit` is SHA-256 of the canonical JSON defined in `schema/config.schema.json`. The file's `config_commit` must equal the recomputed value. Secrets are not part of the preimage.

## Bootstrap

`control-identity bootstrap-issue` writes one 256-bit token to the `--out` file, mode `0600`, and prints a JSON object that does not contain the token. The database stores only the hash. TTL is `bootstrap_ttl_seconds`. A later issue supersedes an unused token. Issue after initialization is `bootstrap.unavailable`.

Consume requires a verified OIDC session whose subject and issuer equal the frozen bootstrap binding, a live token, and an uninitialized install. Expiry, replay, supersession, and a subject mismatch are audited. A mismatch does not burn the token. The winner of two concurrent consumes and the initialization flag commit in one transaction; the loser reads the consumed row and audits `bootstrap.replay`. There is no default admin password and no self-asserted role.

The login-query token and `POST /control/v1/auth/bootstrap/consume` share that consume transaction. The POST body is `{"token":"..."}` and uses the CSRF and Origin rules.

## PostgreSQL

`migrations/oo_identity_control_v1.sql` runs once inside one transaction. The manifest stores that file's SHA-256. The same hash replays. A different hash is install drift and is not repaired. The script does not `GRANT` data roles and does not create a customer principal. Audit rows are insert-only: a trigger rejects `UPDATE` and `DELETE`.

The login role is `oo_identity_admin`, a member of `NOLOGIN` `oo_identity_runtime`. The password and the PostgreSQL CA come from files named by the config. TLS is required. `synchronous_commit` is on for these transactions.

## Commands

```text
control-identity install --config CONFIG
control-identity bootstrap-issue --config CONFIG --out FILE
control-identity serve --config CONFIG
```

| Condition | Exit |
|---|---|
| malformed argv or config | 1 |
| refused business rule | 2 |
| committed or listening | 0 |
| `commit.unknown` | 3 |

Stdout is one JSON object and contains no secret, verifier, token, cookie, or password.

## Dependencies

Direct pins already in the repository root lock. No new crate, no version bump, no root manifest edit, no root lock edit, no `cargo update`. `rust-version` is 1.94.0. Safe Rust only. RS256 verification and test-IdP signing use `ring` 0.17.14. There is no JWT crate.

## Still required

Customer IdP confirmation, the rest of the control API, data-role grants, owner certification, agent and harness sessions, Gateway tokens, and independent review. A green test does not accept SP02-W05 or E54.
