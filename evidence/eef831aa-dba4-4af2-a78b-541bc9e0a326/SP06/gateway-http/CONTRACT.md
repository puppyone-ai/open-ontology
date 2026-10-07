# HTTP gateway consumer contract

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a326`. Frozen unit E45 / GW-D02. Fixed product baseline `e8d297f95b32052ea587fe8ad67b6e8493a00300`. This file and `crates/oo-gateway/src/http_error.rs` are the pre-implementation contract. The router, binary, pack loader, token-store embed, and root lock edit come after this commit.

Prerequisite already read: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a319/SP00/gateway-dependencies-review/` verdict `APPROVE_LIMITED` for archive `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96` and the Axum/Tower checksums below. That approval is the dependency baseline. It is not this service, not a consumer lock, and not a release. `execution/decisions/gateway-http-dependency-baseline.md` stays `UNAPPROVED` on disk. Token storage is the accepted `gateway-token-store-v1` tree at `99ac19af42059a308ba9c418815619e5a081f407` (byte-identical in the current tree). Public HTTP, OAuth, an IdP, and external mTLS stay outside that storage review.

This card does not claim a public production Gateway, a parent goal, or a release.

## Authentication boundary

The process is a loopback HTTP service. It binds `127.0.0.1` only and refuses any other listen address. It does not terminate client TLS, does not check a client certificate, does not speak OAuth, and does not call an IdP.

`GET /health` and `GET /ready` answer without a token. Their JSON says `authentication` is `loopback-http`, and `external_mtls`, `oauth`, and `idp` are false. Readiness means the operator config and the pack catalog loaded. It does not ping PostgreSQL, so a database outage leaves readiness up.

A caller header, query string, or JSON field is never the project, role, principal, scope, or audience. Those values come from the PostgreSQL row for the SHA-256 of the bearer token. Optional `X-OO-Project`, `X-OO-Role`, and `X-OO-Scope` must match that derived tuple or the response is `FORBIDDEN` with no other pack's bytes. Absent headers mean the derived tuple alone.

Client tokens (`ooct_`) are presented as `DeclaredTransport::ExternalClient`. Session tokens (`oost_`) require `DeclaredTransport::EgressProxy`. This process does not perform that proxy, so a session token fails the accepted binding check and the HTTP status is `UNAUTHENTICATED`.

## Error schema

Application errors use one JSON object. `query_id` is omitted because this process does not mint audit ids. `oo-core::api::ErrorBody` always serializes `query_id` and has no `SQL_UNSUPPORTED`, `FORBIDDEN`, `QUERY_NOT_ALLOWED`, `NOT_FOUND`, or `METHOD_NOT_ALLOWED` variant. This card does not edit `oo-core`. The local type is `oo_gateway::http_error::ApiError`. Constructors take a code only.

```json
{"error":{"code":"SQL_UNSUPPORTED","message":"SQL execution is not served by this process.","hint":"Untrusted SQL stays unavailable until a kernel sandbox admits it.","retryable":false}}
```

| code | HTTP | retryable | when |
|---|---|---|---|
| `UNAUTHENTICATED` | 401 | false | missing, malformed, unknown, expired, revoked, stale, audience, binding, or database failure |
| `FORBIDDEN` | 403 | false | derived tuple is valid and the optional project, role, or scope header disagrees, or no pack matches |
| `QUERY_NOT_ALLOWED` | 400 | false | the request URI has a query string; the response does not echo it |
| `NOT_FOUND` | 404 | false | unknown path; the log line says `path=unknown` |
| `METHOD_NOT_ALLOWED` | 405 | false | known path, wrong method |
| `SQL_UNSUPPORTED` | 501 | false | `POST /v1/sql` |
| `INTERNAL` | 500 | true | unexpected local failure; no driver text and no bearer |

`docs/07-gateway.md` §7 also lists `TIMEOUT` 408, `RATE_LIMITED` 429 retryable, and `OVERLOADED` 503 retryable. This process handles the first as a Tower status and does not emit the other two.

`TimeoutLayer::with_status_code` answers HTTP 408. `RequestBodyLimitLayer` answers HTTP 413. Those bodies are the layer bodies, not this envelope. The global limit is one `GlobalConcurrencyLimitLayer` shared by every route. A request that waits on that permit is cut by the request deadline, so the status is 408 rather than 429. There is no per-route semaphore and no Redis counter. The breaker in `docs/07` §6 is not implemented, so 503 `OVERLOADED` is not emitted.

Store failures map to `UNAUTHENTICATED`. The client body is the static sentence. The stderr line may include the stable `StoreError::reason()` code. It must not include `StoreError`'s `Display` (that text can carry a SQLSTATE), the bearer, the password, or the query string.

## Routes

| method and path | auth | body |
|---|---|---|
| `GET /health` | none | process up, loopback boundary flags, `sql_execution=unsupported` |
| `GET /ready` | none | catalog loaded, `database=not_probed` |
| `GET /v1/session/current` | bearer | server-derived project, principal, role, policy, knowledge, audience, scope; no token and no hash |
| `GET /v1/pack/metadata` | bearer | metadata of the one pack selected by derived project, role, policy version, and knowledge version |
| `POST /v1/sql` | bearer checked only to apply the body limit consistently; the handler does not read a caller tuple | `SQL_UNSUPPORTED` |

The SQL handler does not call `execute_trusted_select`, `query_executor`, or any dynamic SQL. The HTTP module source must not contain those names. `POST /v1/sql` exists so the route is explicitly unsupported. It is not a bypass around the kernel-sandbox block.

The production body limit is 1 MiB (`docs/17` §2). Loopback tests build a router whose limit is 32 bytes: 32 bytes on `POST /v1/sql` reaches the 501 envelope, 33 bytes is 413, and the extra payload is not in the response. The production request deadline is 5000 ms. Tests mount a short deadline on `/slow` and leave `/hold` outside that short deadline, matching the reviewed probe. The production router does not mount `/hold` or `/slow`.

Unknown paths and query strings are rejected before the bearer is used. Logs are one stderr line per response: method, known path or `path=unknown`, status. No `Authorization` value, no query string, and no debug dump of the token.

## Token lookup

`oo-arch-check` rejects a path dependency on `tools/gateway-token-store` (`R_DEP_UNRESOLVED` unless that crate becomes a workspace member, which would be `R_MEMBER_UNKNOWN`). This card does not edit `oo-arch-check`. `build.rs` compiles the accepted sources into `oo-gateway` after checking SHA-256:

| file | SHA-256 |
|---|---|
| `src/cache.rs` | `23ec81bba03fe71b28c026798966f5f748a5d8932916648962ad4fc3881f4fc1` |
| `src/canon.rs` | `dd395153171400551517f18c61b6955d86c9418d85b3d5e8d8f905918ff57730` |
| `src/error.rs` | `6bd1f3a033381135d070c485d5fdfd56735c44dd01e0a19225f51b0a15fa7ed5` |
| `src/lib.rs` | `6cac5d52aed4f82c6895744ee73fb2ebd6dd07732e753e9506086c8e1eaf14e5` |
| `src/live.rs` | `b58e56f84309de4cbd7d5d59f4f377c2f06d2f7d1ef603517c63482aeb6529b3` (hashed, not compiled) |
| `src/model.rs` | `9dc3a64f158f23bbe92b924e5af3934dbdf98a41c3730d2fce34c5b641755de1` |
| `src/sha256.rs` | `eac61aa8d9d0eebe935be0d438b6877b40845656601c4c5a8180edd97963f037` |
| `src/sql.rs` | `46e36bef797c85a64c0d1714eeccc4a3897432d51597aea3c9eb5b7e02956fba` |
| `src/store.rs` | `2cf8e9ead5843d7c8a8da8b415aaea6182b39ba457153741530d0c4cf14c7827` |
| `src/tls.rs` | `1300d55b8067d619f4dbfd812937d3ee60185d4ed563e071d46eb483396c5ec7` |
| `src/token.rs` | `a68cb481d97b34f898ff95fb055f0bfec4447f094d6d07878580c46cad71b8e3` |
| `src/trusted_control.rs` | `d8d9eacf3642931d01a06a71387e365f0c593d3ee02cc43d09c42f8cb903c757` |
| `migrations/oo_token_control_v1.sql` | `501d52c0f1f8de46470fc3192fca7de3d59dfd1a3b990dc70daef97341b85597` |

The rewrite maps `crate::` to the embedded module path, drops the inner `#![forbid(unsafe_code)]` because the gateway crate already forbids unsafe code, and points the migration `include_str!` at `CARGO_MANIFEST_DIR`. `live.rs` is not compiled. Direct dependencies added for that embed, both already present as registry triples, are `getrandom` `=0.2.17` default-features false (checksum `ff2abc00be7fca6ebc474524697ae276ad847ad0a6b3faa4bcb027e9a4614ad0`) and `zeroize` `=1.9.0` default-features false, feature `alloc` (checksum `e13c156562582aa81c60cb29407084cdb54c4164760106ab78e6c5b0858cf64e`).

The HTTP runtime logs in as `oo_token_runtime`. Startup refuses any other runtime user. It selects the frozen row with the fixed statement below (parameter `$1` only) and then calls the accepted `lookup` with `DeclaredTransport::ExternalClient`, the configured audience, and that derived `CurrentTuple`. `lookup_for_test` is not used on the request path. The positive cache inside the accepted store is 5000 ms of monotonic time. A second process-local tuple cache uses the same bound, keyed by the token hash, and is never consulted unless a later `lookup` succeeds. A failure does not extend either cache. After the deadline, a closed database is `StoreError::Unavailable` and the HTTP status is 401. Revocation is visible on the next lookup after that deadline. A warm hit may stay positive until the original deadline. That is the accepted store rule.

```sql
SELECT token_kind, project_id, principal_id, role_name, claims_canonical, policy_version, knowledge_version, audience_url, sandbox_id, session_id, issued_at_ms, idle_deadline_ms, absolute_deadline_ms, revocation_floor FROM oo_token_control.token_record WHERE token_sha256 = $1
```

The runtime connection uses `SslMode::Require`, the caller CA PEM, `rustls_connector`, hostname verification, and `pg_stat_ssl`. `application_name` is `oo-gateway-http`. On each tuple-cache miss the process re-reads the endpoint file, so a test can point the next miss at a closed local port without publishing a second listen API. The password and CA are read at startup and zeroized when the process drops them. Cache hits do not re-read the endpoint and do not touch PostgreSQL.

Issue and revoke in the live test use `TrustedControlAdapter` with the admin role. Those credentials are test inputs. They are not in the HTTP process config.

## Pack metadata

The catalog is loaded at startup from an operator directory. Direct children only, names matching `^[a-z0-9][a-z0-9_-]{0,62}\.json$`, no recursion, and no join of a database string onto a path. Each artifact is protocol `oo.builder.role-pack/v1` with a manifest of protocol `oo.builder.role-pack-manifest/v1`. `manifest_sha256` must equal the SHA-256 of the manifest JSON text. `pack_digest`, when present, must be 64 hex characters. This card does not re-implement the compiler digest. Two artifacts with the same project, role, policy version, and knowledge version refuse startup.

The response lists `pack_digest`, public file paths, file sizes, file digests, and column names parsed from `` | `name` | `` rows in `tables/*.md`. It does not return file text. The live fixture plants `picker_note` only in the picker pack and `auditor_phone` only in the auditor pack. A wrong role, project, or scope receives `FORBIDDEN` and neither marker. The fixtures follow the compiler protocol and are not a second compiler. There is no path dependency on `oo-builder`.

## Dependency edit

On `oo-gateway` only, set `axum`, `tower`, and `tower-http` to `workspace = true`. Leave the axum `json`, `query`, `form`, and macros features off. Parse and emit JSON with the existing `serde_json` pin. Leave tower-http `auth` off. Then a targeted lock update of axum 0.8.9, not `cargo update` of the workspace and not `cargo generate-lockfile`.

The root lock may add only these registry triples. Existing name, version, and checksum pins stay:

- axum 0.8.9 `31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90`
- axum-core 0.5.6 `08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1`
- httpdate 1.0.3 `df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9`
- matchit 0.8.4 `47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3`
- mime 0.3.17 `6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a`

Workspace features already pinned: axum `=0.8.9` default-features false, features `http1` and `tokio`; tower `=0.5.3` default-features false, features `limit`, `make`, `timeout`, `util`; tower-http `=0.6.11` default-features false, features `limit` and `timeout`. Frozen lock before the edit is SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. The hyper dependency array must keep `h2` and `want` and add `httpdate`. The diff is printed with the reviewed `lockdiff.rs` (SHA-256 `58041b895fd19549bf0da35439f9e201217c6b429c9c6a4cf4ecac629c050f08`). Any other new registry triple, or any changed checksum, is reverted.

Denylist, zero added: jwt, jose, oidc, oauth, openid, aliri, paseto, jsonwebtoken.

## Operator bootstrap

One argv: the config directory. The password file and the CA file are one-segment names inside that directory. The bound address is written to `listen.addr` in that directory. The runtime user field is exactly `oo_token_runtime`.

## Proof required after the implementation commit

Pre-edit smoke uses `git archive` of `e8d297f` with no product overlay. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, 6 GiB, 1 CPU. The checks are `cargo check --locked -p oo-arch-check` and `cargo check --locked -p oo-gateway`. Attempt `linux/smoke` is preserved: rustc 1.94.0, overlay absent, `oo-arch-check` exit 0, `oo-gateway` exit 101 because `pg_query` bindgen found no libclang (`OOMKilled=false`). Attempt `linux/smoke-2` keeps that failure in place and extracts the a200 Debian libclang set onto the private cargo volume before the same two checks. The archive remains `e8d297f`.

After implementation, one builder and one private PostgreSQL 17.10 (`docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`). Focused tests plus Clippy `-D warnings` on a git archive of the implementation commit. The live test is ignored unless `OO_A326_LIVE=1` and is run with `--ignored --test-threads=1`. It spawns `CARGO_BIN_EXE_oo-gateway`. Minimum results: issue a client token, HTTP 200 with the matching pack metadata; wrong role, scope, and project denied with no hidden marker; revoke, wait at least 5200 ms, then 401; a new process still returns 401 for that token; after a fresh 200, a closed database port after the cache deadline returns 401 and restoring the port returns 200 for a token that is still valid; body, deadline, and the single shared permit; restart leaves the cache empty. Stderr and the child command line must not contain the password or a bearer. Loopback body, time, and concurrency tests are not ignored.

One Rust builder per task, global cap 10. The PostgreSQL container is the one private database. No privileged mode, host network, host socket, host home, shared cargo cache, or published ports.
