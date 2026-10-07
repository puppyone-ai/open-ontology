# Review contract

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a337`. Independent review of frozen child E45 / GW-D02 at commit `51d08e59ddb3c92fc0b9f2958966b56e19032b7f`. This file is the check list. It is not a product change and it is not a parent or release approval.

The product tree under test is `git archive` of that commit. The live workspace is not mounted. a327 may write policy and numeric sources after this commit. Those writes are outside this review. Ancestors already inside `51d08e59` stay in the archive because they are part of that tree.

## Baseline that must be read

a319 verdict `APPROVE_LIMITED` for archive `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96`. Its `SHA256SUMS` is 1404 lines. Root manifest blob `b772f8fb4873885fcfac747eb24163559031bb9192ba99e8a8472571c75822bc`. Root lock before the consumer edit `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`, which is also the lock at parent `a40fbf1f783143a14524d6bdd70f8878ea4acf65`. Decision file status stays `UNAPPROVED`.

Approved workspace features, default features off:

- axum `=0.8.9` features `http1`, `tokio`
- tower `=0.5.3` features `limit`, `make`, `timeout`, `util`
- tower-http `=0.6.11` features `limit`, `timeout`

Approved new registry triples, and no others:

- axum 0.8.9 `31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90`
- axum-core 0.5.6 `08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1`
- httpdate 1.0.3 `df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9`
- matchit 0.8.4 `47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3`
- mime 0.3.17 `6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a`

Hyper 1.11.1 must keep `h2` and `want` and add `httpdate`. Tower may add `tokio-util` and `tracing`. Tower-http may add `http-body-util` and `tokio`. No removed edge. No changed name, version, or checksum. No JWT, JOSE, OIDC, OAuth, OpenID, Aliri, Paseto, or jsonwebtoken package.

## Source checks

`oo-gateway` takes axum, tower, and tower-http from the workspace. It has no path dependency on `tools/gateway-token-store`. `oo-arch-check` on the archive must print `architecture check: pass`. `build.rs` must hash the accepted store bytes listed in the a326 contract and refuse a mismatch. The only `clippy::all` allow is the private module glue `#[allow(unused, clippy::all)]`. `lib.rs` and the HTTP sources do not carry that allow. `http_gateway.rs`, `pack_catalog.rs`, and `src/bin/oo-gateway.rs` do not name `execute_trusted_select` or `query_executor`.

## Pre-overlay, then one private database

On the read-only archive, before any review test is added: `cargo test --locked --offline -p oo-gateway --lib` reports 42 passed, `cargo test --locked --offline -p oo-gateway --test http_gateway_loopback` reports 8 passed, and `cargo clippy --locked --offline -p oo-gateway --all-targets -- -D warnings` exits 0. The private PostgreSQL 17.10 must already be accepting connections before the fresh HTTP proof. One builder, 6 GiB, 1 CPU. One database, 1 GiB, 1 CPU.

## Fresh HTTP proof

The review test is not the author's `http_gateway_live.rs`. It drives the archive binary and the accepted issue and revoke functions. It must show:

- A client token's derived project, principal, role, policy version, and knowledge version select that role's pack metadata, including file digests of the embedded text.
- A second pack with the same role and a different policy version is not returned.
- The other role's column name is absent.
- `X-OO-Role`, `X-OO-Project`, and `X-OO-Scope` cannot grant a different pack.
- A session token is `401`.
- A query string is `400` and is not echoed. `POST /v1/sql` is `501` with `SQL_UNSUPPORTED`.
- Body limit 32 is `501` and 33 is `413` on the binary. A stalled lookup hits `408`. One global permit holds the second request.
- Revoke is committed in PostgreSQL. A hit inside the original 5 second window stays `200`. A hit at 5.2 seconds from that window's start is `401`. The middle hit does not move the deadline.
- After that deadline, a closed database port is `401`, a new process is still `401`, and restoring the port is `200`. Readiness stays `database=not_probed`.
- A flipped file digest refuses startup. The operator JSON is not a compiler artifact: `pack_digest` is recorded as declared or bound, whichever the binary does.

## Verdict

`APPROVE_LIMITED` only if every check above holds and the recorded pack-digest behavior matches the source. `WITHHELD` if identity, revocation, fail-closed outage, header authority, SQL execution, the lock, the embed bytes, or the allow scope fails. Public mTLS, OAuth, an IdP, durable audit, and a readiness database probe stay outside this child either way. No parent acceptance and no release.
