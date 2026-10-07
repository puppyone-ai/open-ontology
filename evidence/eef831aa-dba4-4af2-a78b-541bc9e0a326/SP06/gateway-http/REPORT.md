# Report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a326`. Frozen unit E45 / GW-D02. This card built a loopback HTTP consumer of the accepted opaque token store. It is not a public production Gateway, a parent-goal acceptance, or a release.

Proved commit `51d08e59ddb3c92fc0b9f2958966b56e19032b7f`. Immutable archive proof: `linux/proof-3/`, container `eaf43e20d755a4ecd09b9c458a5d4790f1ee0291256ad41ab5813b21a0313850`, `inside_ok=2026-10-07T13:15:24Z`, `docker-run.exit=0`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Limits: 6442450944 bytes, 1 CPU, `CapDrop=["ALL"]`, not privileged, network `oo-a326-eef831aa-net`. OOMKilled false.

## What the process does

`oo-gateway` takes one argument, a private config directory. It binds `127.0.0.1` only. Health and readiness are unauthenticated. Readiness reports the pack catalog as loaded and does not ping PostgreSQL. Authenticated routes derive project, principal, role, scope, and audience from the SHA-256 of the bearer through the accepted token store and a runtime-role tuple read. Optional `X-OO-Project`, `X-OO-Role`, and `X-OO-Scope` must match that tuple. The metadata route returns the operator-loaded pack for that project and role: digest, public paths, sizes, and column names. It does not return file text and it does not open a caller-supplied path.

`POST /v1/sql` is `SQL_UNSUPPORTED` / 501. The HTTP module does not call `execute_trusted_select`. Session tokens fail the accepted binding check because this process is not an egress proxy. Client tokens use `DeclaredTransport::ExternalClient`. Health JSON says `authentication=loopback-http` and `external_mtls`, `oauth`, and `idp` are false. There is no external mTLS, OAuth, or IdP client on this path.

The positive tuple cache is 5 seconds. Revocation and a database outage fail closed after that deadline. A warm cache is not extended when PostgreSQL is unreachable. The runtime login is `oo_token_runtime`. Passwords and the CA are files. The access log records method, an allowlisted path, and status. Bearer tokens are not written to the log, the query string, `Debug`, or the error envelope.

## Dependency lock

a319 `APPROVE_LIMITED` covers archive `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96` and the five registry triples. `SHA256SUMS` in that review is 1404 lines. The decision file on disk stays `UNAPPROVED`.

Root lock before the consumer edit: `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Proved lock: `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. `tools/lock-consumer.rs` on the proof printed `added=5`, `removed=0`, `changed=0`, `denied=0`, `consumer=pass`. Added triples:

- axum 0.8.9 `31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90`
- axum-core 0.5.6 `08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1`
- httpdate 1.0.3 `df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9`
- matchit 0.8.4 `47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3`
- mime 0.3.17 `6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a`

Edge additions, with no removals: hyper 1.11.1 `+httpdate` (checksum `27b501faa50e7a26c3d3560ca625132f4078a17771f4810baf70475ae48cbe43`, `h2` and `want` kept); tower 0.5.3 `+tokio-util` and `+tracing`; tower-http 0.6.11 `+http-body-util` and `+tokio`. Axum features stay `http1` and `tokio`. Query, form, json, macros, original-uri, and tower-http auth stay off.

## Proof results

`linux/proof-3/` is `git archive` of `51d08e59ddb3c92fc0b9f2958966b56e19032b7f` mounted read-only. Cargo used `--locked --offline`. The proved sources are copied under `source/proof-51d08e59/`. `source/blobs/` stays the pre-code snapshot.

- Library tests: 42 passed, exit 0.
- `http_gateway_loopback`: 8 passed, exit 0. Covers the loopback boundary, query-string rejection, body limit 413, `/slow` 408, global concurrency, unknown path 404, method 405, and the production router leaving probe routes unmounted.
- `cargo clippy -p oo-gateway --all-targets -- -D warnings`: exit 0.
- Ignored live test against private PostgreSQL 17.10 (`oo-a326-pg`, `90deb43a33e8`, image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`): 1 passed in 11.25s, exit 0. Marker `ran=1` then `completed=1`. The test issues a client token, reads that role's pack metadata, denies the other role, project, and scope without the hidden column name, rejects a session token, refuses the token after revoke and cache expiry, refuses it again in a new process, denies metadata when the endpoint file points at a closed port, keeps readiness at `database=not_probed`, and accepts metadata after the endpoint is restored. Captured stdout and stderr in `linux/proof/`, `linux/proof-2/`, and `linux/proof-3/` contain no `ooct_` or `oost_` token text.

## Earlier attempts kept

| directory | container | exit | stop |
|---|---|---|---|
| `linux/smoke/` | `5dccacd0fde9` | 101 | bindgen had no libclang |
| `linux/smoke-2/` | `2d40c51c1afa` | 0 | pre-edit MSRV smoke, `2026-10-07T12:09:32Z` |
| `linux/compile/` | `9ec61a7cbce2` | 101 | lock gate passed; Clippy died on the embedded store |
| `linux/compile-2/` | `88018d4300f4` | 101 | `Cell::Decimal` missing in `query_executor_pg_live.rs` |
| `linux/compile-3/` | `79766f6b6288` | 101 | `cloned_ref_to_slice_refs` in `policy_catalog.rs` |
| `linux/compile-4/` | `fd7a49fca2c6` | 0 | overlay compile, not the immutable archive |
| `linux/proof/` | `cbc9012a150f` | 101 | runtime `CARGO_BIN_EXE_oo_gateway` absent; issue had already succeeded |
| `linux/proof-2/` | `c1770c9a12c5` | 101 | `CARGO_BIN_EXE_oo_gateway` is not a compile-time variable |

Commits on this card: `3f4a358ab150` (error schema, already landed), `0a5c77af1165` (router, embed, lock), `2f6ad2a4a8b1` (underscore binary path), `51d08e59ddb3` (hyphenated binary path, the proved tree). a327 files `reviewed_policy.rs`, `query_executor.rs`, `bin/query-pg.rs`, and `tests/a327_numeric.rs` are not in those commits.

`oo-a326-pg` (`90deb43a33e8`) was stopped and removed after this proof. `CLEANUP.txt` records that removal, the three private volumes, the private network, and the host secret directory. No prune.
