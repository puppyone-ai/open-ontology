# E48-D1/1 TLS identity transport

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a350`. Frozen before the Rust edit. Fixed product baseline `99e93d0f88eb95d8b3834c534dc8ae90d62753ec`. The gateway files at that commit are the files this card edits. This contract does not accept SP06, public identity, OAuth, an IdP, token issuance, a session proxy, or the parent plan.

The listener uses the crates already linked by `oo-gateway`: `rustls` `=0.23.45` and `tokio-rustls` `=0.26.5`, served by the public `axum` `0.8.9` `Listener` API. No root manifest edit, no lock edit, no new registry triple, and no feature change on `hyper`.

## Transport

`config.json` may contain `transport`.

| value | listener | bind |
|---|---|---|
| absent | plaintext development loopback, the historical operator file | IPv4 `127.0.0.1` only |
| `loopback` | the same plaintext listener, spelled on purpose | IPv4 `127.0.0.1` only |
| `tls` | rustls server, client certificates required | any `SocketAddr`, including `0.0.0.0` |

Any other string is `transport`. A loopback file that contains a TLS key name is `config`. A TLS file that omits a required TLS name is `config`.

Loopback keeps the previous allowed names: `listen`, `audience`, `runtime_user`, `password_file`, `ca_file`, `endpoint_file`, `pack_dir`, `body_limit`, `request_timeout_ms`, `global_concurrency`, plus `transport` when it is present. TLS adds `tls_key_file`, `tls_chain_file`, `client_ca_file`, and `handshake_timeout_ms`, all required.

`runtime_user` stays `oo_token_runtime`. File names are one path segment. The PostgreSQL `ca_file` is the server CA for the token lookup. It is not the client trust anchor.

## Material checked before bind

`load_operator_dir` builds the rustls `ServerConfig` before `serve` binds. A failure returns one static reason. The process does not listen and does not write `listen.addr`.

| reason | when |
|---|---|
| `tls_key` | missing, empty, no `PRIVATE KEY` PEM, or the PEM is not a PKCS#8, PKCS#1, or SEC1 key |
| `tls_chain` | missing, empty, contains `PRIVATE KEY`, or yields no certificate |
| `client_ca` | missing, empty, contains `PRIVATE KEY`, or is not exactly one certificate |
| `tls` | the leaf public key does not match the private key, or rustls rejects the server config |
| `handshake_timeout` | missing or outside `1..=30000` milliseconds |
| `listen` | loopback address is not IPv4 `127.0.0.1`, or the address does not parse |

The client verifier is `WebPkiClientVerifier` over that one CA. `allow_unauthenticated` is not used. `webpki-roots` and the native root store are not used. An anonymous client, or a certificate that does not chain to `client_ca_file`, fails the handshake. That connection gets no HTTP response.

`handshake_timeout_ms` bounds one handshake. When it fires, or the handshake fails, the TCP connection is dropped and the listener accepts again. The bound is not the HTTP request deadline.

## Peer workload

After rustls accepts the chain, the leaf must contain exactly one subjectAltName, and that name must be a URI:

`urn:oo:workload:` plus 1 to 128 characters from `[A-Za-z0-9_.:-]`

Any other SAN, a second URI, or a missing URI drops the connection before HTTP. The stored peer identity is that full URI. It is read from the verified leaf. It is not read from a header.

The URI is not a role, not a principal, and not an authorized issuer. This process does not construct `DeclaredTransport::EgressProxy` from it. Token lookup stays `DeclaredTransport::ExternalClient`. An `oost_` session token still fails the accepted binding check. HTTP status `401`. A certificate whose URI is `urn:oo:workload:egress-proxy` does not change that. `POST /internal/v1/session-tokens` is not implemented.

## Headers

These headers are never a certificate and never a workload. On either transport, a request that carries one of them is `403` `FORBIDDEN`. The body does not echo the header value.

- `x-forwarded-client-cert`
- `x-client-cert`
- `ssl-client-cert`
- `x-ssl-client-cert`
- `x-ssl-client-s-dn`
- `x-forwarded-tls-client-cert`
- `x-oo-workload`
- `x-oo-peer`

`X-OO-Project`, `X-OO-Role`, and `X-OO-Scope` keep the previous rule: absent is acceptable, a mismatch with the token tuple is `403`, and they do not select the pack.

## Responses

Loopback JSON keeps `authentication` = `loopback-http` and `external_mtls` = false. It does not add `peer_workload`.

A TLS response from `/health`, `/ready`, `/v1/session/current`, and `/v1/pack/metadata` adds:

| field | value |
|---|---|
| `authentication` | `verified-certificate` |
| `external_mtls` | true |
| `oauth` | false |
| `idp` | false |
| `peer_workload` | the URI from the leaf |
| `peer_source` | `verified-certificate` |

`/health` and `/ready` still do not require a bearer. They do require the client certificate, because the handshake does. `/ready` still reports `database` = `not_probed`. `oauth` and `idp` stay false. This process does not issue tokens, does not call an IdP, and does not perform an OAuth exchange.

`/internal` and any path under it returns `403` `FORBIDDEN` on both transports. The log path is the static word `internal`. The suffix is not logged.

`POST /v1/sql` stays `501` `SQL_UNSUPPORTED`. The handler does not call the executor. The source of this module must not name it.

The error envelope is unchanged. Proxy-header and `/internal` refusals use the existing `FORBIDDEN` code. Store failures stay `401`. The stderr line may include `StoreError::reason()`. It must not include the bearer, the password, a private key, or the query string.

## Lookup and bounds

The runtime statement, the `oo_token_runtime` login, `SslMode::Require`, the PostgreSQL CA, `DeclaredTransport::ExternalClient`, the configured audience, and both 5000 ms positive caches stay as they are. A warm hit may remain positive until the original deadline. The next lookup after that deadline sees a revocation. A closed database after that deadline is `401`. A wrong audience, an expired client token, a malformed bearer, and an unknown hash are `401`.

`body_limit`, `request_timeout_ms`, and `global_concurrency` stay on the same Tower layers. `0` is still `limits`. The production body ceiling documented for operators remains 1 MiB when the operator sets that value. The global permit is still one layer for every route. A request that waits on it is cut by the request deadline (`408`), not `429`.

## Process

`oo-gateway` still takes one argument, the config directory. `listen.addr` is written only after `bind` returns a socket of the accepted family. Loopback still refuses a bound address that is not IPv4 `127.0.0.1`.

## Proof this card owes

Linux arm64 Rust 1.94 `cargo check --locked` of the fixed tree before the edit. After the edit, focused tests and Clippy `-D warnings` on a locked archive, then one private PostgreSQL 17.10:

- loopback tests still pass, including a config with no `transport` key
- invalid TLS material exits before `listen.addr`
- plaintext bytes on the TLS port are not an HTTP 200
- a client certificate from another CA, a missing client certificate, and a leaf without the workload URI do not receive metadata
- a valid `ooct_` token over HTTPS returns the compiler digest for two roles, and the other role's column is absent
- `peer_workload` equals the URI in the verified leaf, including when a second certificate changes only that URI and not the role
- forged `X-OO-*` and the proxy certificate headers are `403` and do not return the other role's column or the header text
- no bearer, a wrong audience, an expired client token, a revoked token after the 5000 ms deadline, and an `oost_` token are `401`
- `/internal/v1/session-tokens` is `403`
- `POST /v1/sql` is `501`
- a handshake that does not finish inside `handshake_timeout_ms` is dropped, and a later handshake still completes
- body limit `413` still applies on the TLS listener

No production credential, no customer call, and no committed private key.
