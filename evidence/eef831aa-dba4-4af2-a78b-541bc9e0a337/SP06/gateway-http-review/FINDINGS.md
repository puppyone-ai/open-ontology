# Findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a337`. The passing proof is `linux/preoverlay-3/` unless a finding names an earlier directory.

## F1. The pack digest is declared

`pack_digest_check=declared`. The operator fixture has no `compiler` field. `oo-builder` is not invoked. File SHA-256 values and `manifest_sha256` are checked against the embedded text. `pack_digest` is accepted when it is 64 lowercase hex. Replacing it with `ab` repeated 32 times still loaded, and the HTTP metadata returned that value. Flipping one file SHA-256 to 64 zeros made the process exit 1 before `listen.addr`. The metadata response did not include file text. Role selection still used the derived project, role, policy version, and knowledge version: the auditor token did not receive the picker pack.

This is a verified operator catalog. It is not a compiler-authenticated artifact. The limited approval records that distinction.

## F2. Identity, revoke, and the closed port

A client token's derived tuple selected that role's pack. The other role's column and the same role's other policy column were absent. Forged `X-OO-Role`, `X-OO-Project`, and `X-OO-Scope` returned 403. A session token returned 401.

The revoke was committed in PostgreSQL before the warm request. That request, about one second after the first 200, stayed 200. The request at 5.2 seconds from the same start returned 401. A new process returned 401. The middle hit did not move the deadline.

With the endpoint file pointed at a closed port, the warm request stayed 200 and the request at 5.2 seconds returned 401. `/ready` stayed `database=not_probed`. A process started during the outage returned 401. Restoring the endpoint returned 200 for the auditor pack.

## F3. SQL, bounds, and the shared permit

`POST /v1/sql` returned 501 `SQL_UNSUPPORTED` with a bearer and with no bearer. The route does not call `execute_trusted_select`. HTTP sources in the archive do not name that function or `query_executor`. A missing bearer does not run SQL.

The production router left `/slow` and `/hold` unmounted. A 32-byte body returned 501. A 33-byte body returned 413 and was not echoed. A stalled TLS handshake returned 408 at 805 ms. Health, started while that request held the single global permit, returned 200 after 604 ms. `waiter_cut=0`. The timeout layer sits inside `GlobalConcurrencyLimitLayer`, so the waiter waited for the permit and was not answered with 408.

A query string returned 400 and was not echoed. Listen `0.0.0.0:9` was refused.

## F4. The allow is one private module line

`build.rs` contains one `#[allow(unused, clippy::all)]` and one `clippy::all`. The glue names `mod accepted_token_store`. `lib.rs`, `http_gateway.rs`, `pack_catalog.rs`, and `src/bin/oo-gateway.rs` do not contain `clippy::all` or an inner `allow`. `lib.rs` forbids unsafe. All 13 embedded digests matched the archive files. `oo-arch-check` printed `architecture check: pass`. The arch check does not see the `build.rs` bytes. Both gates passed.

## F5. The lock adds five triples

`added=5`, `removed=0`, `changed=0`, `denied=0`, `consumer=pass`. Hyper 1.11.1 keeps `h2` and `want` and lists `httpdate`. Tower 0.5.3 lists `tokio-util` and `tracing`. Tower-http 0.6.11 lists `http-body-util` and `tokio`. Root `Cargo.toml` is the a319 blob `b772f8fb4873885fcfac747eb24163559031bb9192ba99e8a8472571c75822bc`. No JWT, OAuth, or OIDC crate was added by this consumer diff.

## F6. Parent gaps stay open

Health and the session body set `authentication` to `loopback-http` and `external_mtls`, `oauth`, and `idp` to false. This process does not terminate public client TLS, perform an OAuth exchange, or call an IdP. Readiness does not probe PostgreSQL. Durable audit is not in this child. Those gaps stay with the parent. This limited approval does not close them.

The 42 library tests include code that later numeric commits also touch. Approving this HTTP child does not approve that numeric unit. a327's newer source writes were not built. The review used the immutable `51d08e59` archive.

## F7. Attempts before the passing proof

`linux/preoverlay/` exited 1. Cargo 1.94 `cargo fetch` rejected `-p`. The lock gate and the embed gate had already passed. `linux/preoverlay-2/` used `cargo fetch --manifest-path /src/Cargo.toml --locked`, then passed the 42 library tests, the 8 loopback tests, package Clippy, the live PostgreSQL check, and arch-check. Clippy on the private test failed with `collapsible_if` and exited 101. `linux/preoverlay-3/` first exited 3 because a host `cargo build` for another task was running. No container was created. The relaunch, after that host process exited, is the passing proof.

`review-http.stdout`, `review-http.stderr`, and `review-http.txt` in the passing proof have no bearer prefix. The gateway stderr and argv checks passed inside the test.
