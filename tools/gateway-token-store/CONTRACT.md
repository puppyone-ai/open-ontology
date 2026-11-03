# gateway-token-store-v1

Opaque Gateway token persistence and a process-local positive cache.
Contract `gateway-token-store-v1`. This crate is not a JWT library, not an
identity provider, and not an mTLS terminator.

Fixed parent commit for the build archive:
`25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`.
Direct registry pins are the versions already in that commit's root
`Cargo.lock`. This crate does not edit the root manifest or the root lock.
a207 remains the only writer of the connector manifest and the root lock.
No `cargo update` and no `cargo generate-lockfile`. The crate lock is a
hand subset: every registry triple (`name`, `version`, `checksum`) is copied
from the fixed root lock. The historical session-store author run that
generated a lock and then `cargo update --precise` is not repeated.

## What this crate is

A standalone workspace. It stores the SHA-256 of an opaque token and the
tuple frozen at issue. It looks that record up. It does not parse SQL for an
agent, write an audit WAL, terminate TLS client certificates, talk to an IdP,
or exchange OAuth tokens.

Two library surfaces:

- `trusted_control::TrustedControlAdapter` issues and revokes. It connects as
  the login role `oo_token_admin`. It does not accept an IdP assertion, an
  mTLS certificate, or a caller field that means "already authenticated".
- `lookup` reads. It connects as the login role `oo_token_runtime`. It does
  not issue.

A principal id, a role string, and a claims object are data the adapter
stores. They are not a login. `issue` refuses when `principal_id` is
`oo_token_admin`, `oo_token_runtime`, `oo_fixture_admin`, or the connected
`current_user`. Public JSON is not an ingress of this crate. There is no
field `authenticated`, `mtls`, or `caller_authenticated` on the issue or
lookup input.

## Token material

| Kind | Prefix | Entropy | Holder binding |
|---|---|---|---|
| Session | `oost_` | 32 bytes from the OS CSPRNG | `sandbox_id` and `session_id` required |
| Client | `ooct_` | 32 bytes from the OS CSPRNG | both ids absent |

The suffix is unpadded base64url of those 32 bytes. The plaintext exists in
the `PlainToken` returned to the adapter caller. `Debug` prints
`PlainToken(redacted)`. `Drop` zeroes the buffer through `zeroize` 1.9.0
(`alloc`, no `derive`). The plaintext is not a database parameter, not a
cache key, not an error string, and not a `println`.

The stored identity is SHA-256 over the UTF-8 bytes of the full prefixed
token, lowercase hex. The digest is the in-crate FIPS 180-4 function. Empty
string and `abc` are the known answers. No digest crate is added.

Randomness is `getrandom` `=0.2.17` with `default-features = false`. That
version and checksum are already in the fixed root lock (the `ring` edge).
The `js`, `custom`, and `rdrand` features are not enabled. `getrandom::getrandom`
fills 32 bytes. This crate does not claim a statistical test passed. Two
draws are a sample: length 32, not all zeroes, not equal to each other.
`rand` is not a direct dependency.

## Frozen tuple

Issue freezes one tuple. Lookup accepts only when every field matches the
tuple the runtime passes in, and the audience matches the gateway canonical
URL passed separately from the tuple.

| Field | Rule |
|---|---|
| `project_id`, `principal_id`, `role` | One role. Charset `[A-Za-z0-9_.:-]`, length 1..=128. A comma, space, or second role is `role_merge`. |
| `claims` | One JSON object of string values, keys sorted, max 32 keys, canonical text max 2048 bytes. Forbidden keys: `mtls`, `authenticated`, `authentication`, `auth`, `caller_authenticated`, `self_claim`, `role`, `roles`, `idp`, `groups`, `allow`. Those are `forged_auth_claim`. |
| `policy_version`, `knowledge_version` | Same charset, length 1..=64. |
| `audience` | Canonical `https` URL, no userinfo, no query, no fragment, host lowercased, default port 443 omitted, empty path stored as `/`. Compared as that exact string. |
| `sandbox_id`, `session_id` | Required for session, absent for client. Same charset as project id. |

A mismatch of policy, knowledge, project, or principal is `stale`.
A mismatch of audience is `audience`.
A mismatch of role is `role`.
A mismatch of claims is `claims`.
A mismatch of sandbox, session, or declared transport is `binding`.

`DeclaredTransport` is an enum the trusted runtime builds after its own
transport check. This crate does not perform that check.

- Session token: `EgressProxy { sandbox_id, session_id }` must equal the frozen ids. `ExternalClient` is `binding`.
- Client token: `ExternalClient` is required. `EgressProxy` is `binding`.

Claims are never read as a transport decision. `mtls_authenticated` on every receipt is false. `idp_used` is false. `auth_provider_used` is false. `pubsub_used` is false.

## Expiry

Production `TrustedControlAdapter::issue` and `lookup` read `SystemTime`.
That is the production clock.

`issue_for_test` and `lookup_for_test` take a wall clock. A receipt from
those seams says `wall = injected`. A receipt from `issue` / `lookup` says
`wall = system`. An injected-clock test does not certify the production clock.

| Kind | Bound |
|---|---|
| Session | Idle deadline `issued_at + 30 min`. Absolute deadline `issued_at + 8 h`. Caller-supplied TTL is refused. |
| Client | Default `1 h` when TTL is omitted. Supplied TTL must be `1 ms` through `90 days` inclusive. |

A session token is live only while `now < idle_deadline` and `now < absolute_deadline`.
A successful database lookup of a session token sets `last_used_ms = now` and
`idle_deadline_ms = min(now + 30 min, absolute_deadline)`. A client token has
no idle column and is not slid. A cache hit does not slide and does not write.

## Cache

`LookupCache` is process-local. The key is the SHA-256 hex, never the plaintext.
Only a positive database answer is inserted. TTL is `5_000` milliseconds on
`LookupCache`'s monotonic clock (`Instant` in production). Insertion uses
checked add of that constant.

A hit rechecks the frozen tuple, the gateway audience, the declared transport,
and the wall-clock deadlines. It does not change `expires_at_mono`.
A failed refresh, a refused answer, and an unavailable database do not insert
and do not move an existing expiry. After the monotonic expiry, a database
error is `unavailable`. The old positive entry is not returned.

Revocation does not use pub/sub. A cache that was filled before a revocation
can still return that positive entry until its original monotonic expiry.
A lookup on a cache that does not contain the entry reads the database and
rejects as soon as the revocation commits. That is the fresh-lookup path.
The five-second TTL is the bound for a warm entry under this frozen
single-process cache. Pub/sub is not implemented.

## Schema `oo_token_control`

Migration file `migrations/oo_token_control_v1.sql`. The adapter hashes the
file before execution and stores the digest in `install_manifest`. The file
has no `IF NOT EXISTS`, no `CREATE OR REPLACE`, and no repair statements.
Names are not `oo_control` and not `schema_000`.

Tables: `install_manifest`, `token_record`, `revocation`.
Sequence: `revocation_seq`.
No column holds a raw token. `token_sha256` is `text` constrained to
`^[0-9a-f]{64}$` and must not match `^(oost_|ooct_)`.

`token_record` stores kind, the frozen tuple, `issued_at_ms`,
`idle_deadline_ms` (session only), `absolute_deadline_ms`, `last_used_ms`,
and `revocation_floor`.

`revocation` is append-only. The primary key is the exact scope:

| `scope` | Populated key | Other key parts |
|---|---|---|
| `token` | `token_sha256` | `principal_id` and `session_id` are empty |
| `session` | `session_id` | `principal_id` and `token_sha256` are empty |
| `principal` | `principal_id` | `session_id` and `token_sha256` are empty |
| `project` | `project_id` only | the three key parts are empty |

Empty parts are `''`, not NULL, so the primary key is exact. A second revoke
of the same key inserts nothing and returns `replay`. `seq` is not advanced
on replay.

Roles are created outside the migration. `oo_token_admin` owns the objects
because it runs the migration. `oo_token_runtime` receives `USAGE` on the
schema and `SELECT` on the three tables. It receives `UPDATE` on
`token_record.last_used_ms` and `token_record.idle_deadline_ms` only.
It receives no `INSERT`, `DELETE`, `TRUNCATE`, no table-level `UPDATE`,
no `UPDATE` on any other column, and no privilege on `revocation_seq`.
`PUBLIC` is revoked. There is no row security policy and no grant on a
business table.

## Transactions

One connection per call. `BEGIN`, then `SET LOCAL search_path TO pg_catalog`,
`lock_timeout = 10s`, `statement_timeout = 15s`, `synchronous_commit = on`.
TLS is `SslMode::Require` with the caller PEM. `pg_stat_ssl.ssl` must be true.
The connector is aws-lc-rs, an empty root store plus that PEM, hostname
verification, no client certificate, no webpki roots, no native roots.

Install, issue, and revoke take `pg_advisory_xact_lock(881275033, project_key)`
for issue and revoke, and `pg_advisory_xact_lock(881275033)` for install.
The project key is the first four SHA-256 bytes of `project_id` as `i32`.

Every call reads `install_manifest.sql_sha256` and the relation, column,
constraint, sequence, and privilege shape. A mismatch rolls back as
`install_drift`. No autofix.

Install:

- Schema absent: execute the embedded SQL, insert the hash, verify the shape, commit. Disposition `installed`.
- Schema present, hash and shape match: commit without executing the SQL. Disposition `replay`.
- Hash, manifest, relation, column, constraint, sequence, or grant drift: roll back. `install_drift`.
- `GRANT` fails closed when `oo_token_runtime` does not already exist.
- A commit I/O error with no SQLSTATE is `commit_unknown`.

Issue, under the project lock, after the shape check:

- Session id that already has a `session` revocation: insert nothing, `scope_closed`.
- Otherwise read `revocation_floor = max(seq)` over project, principal, and, for a session token, that session id.
- Draw the token only after that check. Insert one row. Commit. Return the plaintext once.
- A later revocation gets a greater `seq`. Lookup treats the row as revoked when any covering revocation has `seq > revocation_floor`, or a `token` revocation exists for the hash.
- A principal or project revocation does not permanently forbid a later admin issue. The new row's floor includes that `seq`, so the new token stays live and the older rows stay revoked. That later issue is a new grant, not a resurrection of the revoked material.
- Replaying the same hash is not an API. Each issue draws new bytes. A revoked row is never updated back to active.

Concurrent issue and revoke on one project take the same advisory lock, so
one commits before the other starts its write.

- Issue commits first, then a covering revoke: the new `seq` is greater than the row's floor. Lookup is `revoked`.
- Session revoke commits first, then issue of that same session id: `scope_closed` and no row.
- Those two orders are also forced by `issue_for_test` / `revoke_for_test`, which wait inside the open transaction after the write and before commit. `issue` and `revoke` do not wait.

Lookup, as `oo_token_runtime`:

- Cache hit within the monotonic TTL: no database round trip, expiry unchanged.
- Otherwise one transaction. Missing hash is `not_found` and is not cached.
- Covering revocation or a token-hash revocation is `revoked` and is not cached.
- Wall clock past a deadline is `expired` and is not cached.
- Tuple, audience, or transport mismatch is the refusal above and is not cached.
- A positive session row slides idle in that same transaction, then the positive answer is cached.
- Connect or query failure after a cache miss or an expired cache entry is `unavailable`. The previous expiry is left where it was.

## Out of scope

Public HTTP authentication, IdP, OAuth, PKCE, refresh rotation, RFC 8693
token exchange, mTLS handshake, Redis or any pub/sub, audit WAL, agent SQL,
session-lease certification, and cancellation of an active query. This
contract does not close SP06-W01, GW-D01's parent, or a release.

## v0.2 lifecycle extension (D9)

The preceding sections describe the immutable, checksum-pinned v1 reader.
The v0.2 service-account entry point is `src/lifecycle.rs`, compiled directly
by `oo-gateway::http_gateway::token_lifecycle`. It uses the Gateway's already
locked dependencies, not a second copy of the v1 crate. The standalone v1
library and session issuance remain unchanged. New CLI and HTTP client-token
issuance use `Control`, not the legacy one-hour issuance default.

### Install and storage

`Control::install` first calls the v1 installer (including its exact schema,
privilege and checksum verification). It then applies
`migrations/oo_token_control_v2.sql` transactionally under the installation
advisory lock. The new schema is **oo_token_lifecycle**, not an alteration of
v1: the pinned reader rejects additional columns or tables in its schema.
The v1 file SHA-256 remains
`501d52c0f1f8de46470fc3192fca7de3d59dfd1a3b990dc70daef97341b85597`.
The extension records its own digest; exact replay verifies it and does not
execute DDL again. Apply through `install`, not by blindly rerunning SQL.
An interrupted extension transaction rolls back; the installed v1 remains
usable and a subsequent install can retry. This works on empty and v1 DBs.

- `policy`: per-project max lifetime and default TTL both default to 90 days;
  max lifetime must be positive and at most 365 days. Default TTL cannot
  exceed it. Self-renew defaults disabled, with a 14-day window; overlap is
  7 days; enrollment expiry is 24 hours. Policy replacement is audited.
- `token`: first issuance, rotation predecessor, overlap end, issuer and
  purpose; v1 rows are backfilled without changing their original deadlines.
  Legacy rows issued after migration use their v1 issuance time as fallback.
- `enrollment`: SHA-256 of the complete `OO-XXXX-XXXX` code, frozen grant,
  expiry, redeemed time and trusted redemption origin. Codes contain 40
  random bits (eight uniform Crockford base32 symbols); never plaintext in DB.
- `audit`: exactly one row for each dispatched operation, including list,
  replay and semantic refusal. Operation changes and the success audit commit
  together; a savepoint rolls back refused changes before recording refusal.
  Hashes, actor and old/new deadlines only; no token or code values. Malformed
  HTTP, unauthenticated requests and unavailable-DB failures cannot promise a
  database audit row. A commit acknowledgement failure is `commit_unknown`;
  do not automatically retry secret-producing operations.

All operations use the v1 project advisory lock, a TLS-verified
`oo_token_admin` connection and fixed parameterized SQL. Runtime credentials
cannot read enrollment grants or mutate lifecycle policy/audit. Revocation
and overlap shortening update the same v1 records consumed by the unchanged
five-second positive cache. No pub/sub or immediate warm-cache revocation is
claimed. A previously cached deadline can survive its original <=5s TTL;
new reads enforce `now < deadline`, including the exact overlap boundary.

`renew` never returns or changes the token value, never resurrects expired
or revoked material, and requires a strictly later deadline no later than
`first_issued_at_ms + max_lifetime_ms`. Self-renew verifies token and Gateway
canonical audience directly against live state, requires the policy flag and
`deadline - now <= self_renew_window_ms`, and clamps the requested extension
to the lifetime ceiling. An already maximal deadline cannot be extended.

`rotate` preserves first issuance and the frozen tuple. New expiry is bounded
by the same lifetime ceiling. Old expiry is the earlier of its existing
expiry and `now + rotate_overlap_ms`; zero overlap immediately invalidates
it. Rotated predecessors cannot be renewed or rotated again. An explicit
new admin issue is a new grant, unlike renewal/rotation.

### Admin and public HTTP

Operator config may add `token_admin_password_file` (one filename, mode 0600)
and `token_admin_workload` (an exact trusted certificate workload URI).
Lifecycle HTTP is disabled without this configuration and requires TLS.
This opt-in permits ordinary verified-server TLS clients without client
certificates; it does not make their requests mTLS-authenticated. Existing
mTLS-only deployments without the opt-in retain their handshake policy.

`POST /internal/v1/tokens` accepts an `operation` discriminator and requires
an actual verified client certificate matching `token_admin_workload`.
A bearer token or certificate-looking header is not admin authority. Other
`/internal/**` routes remain denied. Accepted request shapes:

```json
{"operation":"issue","grant":{"project_id":"demo","principal_id":"alice","role":"reader","claims":{},"policy_version":"v1","knowledge_version":"v1","audience_url":"https://gateway.example/","ttl_ms":3600000,"purpose":"automation"}}
{"operation":"list","project":"demo"}
{"operation":"renew","project":"demo","hash":"<full SHA-256>","deadline_ms":1800000000000}
{"operation":"rotate","project":"demo","hash":"<full SHA-256>"}
{"operation":"revoke","project":"demo","hash":"<full SHA-256>"}
{"operation":"enroll","grant":{"project_id":"demo","principal_id":"alice","role":"reader","policy_version":"v1","knowledge_version":"v1","audience_url":"https://gateway.example/"}}
{"operation":"policy","project":"demo","policy":{"max_lifetime_ms":7776000000,"default_ttl_ms":7776000000,"self_renew_enabled":false,"self_renew_window_ms":1209600000,"rotate_overlap_ms":604800000,"enroll_ttl_ms":86400000}}
```

`POST /v1/auth/enroll {"code":"OO-XXXX-XXXX"}` atomically consumes the code
and returns `token`, `token_sha256`, role, audience, first issuance and expiry.
Audience comes from server configuration, not request JSON. A wrong-audience
attempt does not consume the code. Simultaneous redemptions have one winner.
A global process-local budget permits 10 enrollment attempts per minute;
restart resets that budget. Distributed deployments must enforce a shared
edge limit as well. There is no device approval flow on this card.

`POST /v1/auth/renew` accepts an empty body or `{}` and the bearer token;
it returns deadline metadata, never the token. Responses use
`Cache-Control: no-store`. Token/code values occur only once in a secret-
producing response. Errors expose stable reason codes, not database errors.
List returns hash prefixes, role, expiry, first issuance, last-use observation
and overlap; it never returns plaintext. The retained v1 client's last-use
column is not a per-request activity counter (cache hits do not write).

### CLI

Existing `issue-token install ENDPOINT PASSWORD_FILE CA`, `issue ... GRANT
OUTPUT`, and `revoke ... PROJECT HASH` argument order remains supported.
The issue grant retains `kind=client` and optional `client_ttl_ms`; it also
accepts `purpose`. Client TTL now defaults to 90 days.

New commands: `list|renew|policy ENDPOINT PASSWORD_FILE CA REQUEST_JSON` and
`rotate|enroll ENDPOINT PASSWORD_FILE CA REQUEST_JSON OUTPUT`. Request JSON
is the admin request without `operation`. Plaintext goes only to a newly
created 0600 output file; stdout contains metadata. Existing output files
are never replaced. Failure to write a file after issuance does not roll
back a committed credential: revoke it by its audited hash and issue again.
Passwords, enrollment codes and bearer values are never argv arguments.
