# Report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a337`. Independent review of frozen child E45 / GW-D02.

Verdict: **APPROVE_LIMITED** for commit `51d08e59ddb3c92fc0b9f2958966b56e19032b7f`. This review does not approve a parent goal, a release, or the plan line. The plan checkboxes in `specs/open-ontology-v0.1-plan.md`, `specs/open-ontology-development-ledger.md`, and `specs/open-ontology-acceptance-counting.md` were still open when this review finished. `execution/decisions/gateway-http-dependency-baseline.md` was not edited. Its header remains `UNAPPROVED`. The later a319 paragraph in that file approves the dependency baseline only and says the consumer was not accepted there.

Passing proof: `linux/preoverlay-3/`. Container `205a4c59a0811b2ab05ef1f1216a0f92639ddd82dc1b90c1794ec89c5c2494b5`. `inside_ok=2026-10-07T14:46:09Z`. `docker-run.exit=0`. PostgreSQL was accepting connections at `2026-10-07T14:45:34Z`, before the HTTP test. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `aarch64-unknown-linux-gnu`. Limits: memory `6442450944`, `NanoCpus=1000000000`, `CapDrop=["ALL"]`, privileged false, network `oo-a337-eef831aa-net`. OOMKilled false. `/tmp` was `rw,nosuid,nodev,noexec,size=1g`. The archive mount was read-only.

The workspace HEAD recorded at launch was `e7fb67e27bcffcdeec10ad84463b4854bef6eed0`. The builder did not mount that tree. `archive-commit.txt` is `51d08e59ddb3c92fc0b9f2958966b56e19032b7f`.

## Archive checks, before the review test

`linux/preoverlay-3/review-tree-delta.txt` is one line: `review_http.rs` appeared only after the archive checks. On the read-only archive:

- `cargo test --locked --offline -p oo-gateway --lib`: `test result: ok. 42 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s`. Exit 0. The 0.01s time is the cached rerun. The first compile of that same archive, in `linux/preoverlay-2/`, finished the library tests in 2m 07s with the same 42 passed line, then stopped later on the private test.
- `cargo test --locked --offline -p oo-gateway --test http_gateway_loopback`: `test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.48s`. Exit 0.
- `cargo clippy --locked --offline -p oo-gateway --all-targets -- -D warnings`: exit 0.
- `oo-arch-check`: `architecture check: pass`. Exit 0.
- Embed gate: 13 accepted store files, each pinned digest equal to the archive bytes. Exit 0.
- `scope_review`: `allow_line=1`, `clippy_all=1`, `lib_allow=0`, `executor_name=0`, `features=pass`, `http_bounds=pass`. Exit 0.

The 42 library tests execute ancestors that are already inside `51d08e59`, including policy and numeric code that a327 later edits on newer commits. This verdict does not approve that later unit.

## Lock

Root `Cargo.toml` blob from the archive: `b772f8fb4873885fcfac747eb24163559031bb9192ba99e8a8472571c75822bc`. Pre-consumer lock, parent `a40fbf1f783143a14524d6bdd70f8878ea4acf65`: `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Archive lock: `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. `build.rs`: `013cab8e17e5086e8652834e61622120386b1c8972b5213f634ededf13c77e7b`.

`lock-consumer` exit 0:

```
added=5
removed=0
changed=0
denied=0
consumer=pass
```

Added triples:

- axum 0.8.9 `31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90`
- axum-core 0.5.6 `08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1`
- httpdate 1.0.3 `df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9`
- matchit 0.8.4 `47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3`
- mime 0.3.17 `6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a`

Feature edges, sorted, matched the approved five lines and no others:

- `edge hyper 1.11.1 +httpdate`
- `edge tower 0.5.3 +tokio-util`
- `edge tower 0.5.3 +tracing`
- `edge tower-http 0.6.11 +http-body-util`
- `edge tower-http 0.6.11 +tokio`

The frozen lock package `hyper` 1.11.1 checksum `27b501faa50e7a26c3d3560ca625132f4078a17771f4810baf70475ae48cbe43` depends on `h2`, `httpdate`, and `want`. Tower 0.5.3 depends on `tokio-util` and `tracing`. Tower-http 0.6.11 depends on `http-body-util` and `tokio`. Cargo did not rewrite `Cargo.lock` after fetch, test, or clippy.

Workspace features checked in the archive root manifest are axum `=0.8.9` with `http1` and `tokio`, tower `=0.5.3` with `limit`, `make`, `timeout`, and `util`, and tower-http `=0.6.11` with `limit` and `timeout`. The gateway manifest uses `workspace = true` for those three. It does not name `gateway-token-store`.

## Independent HTTP proof

`cargo test --test review_http -- --ignored --test-threads=1` on the private copy: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.67s`. Exit 0. Review Clippy on that test exited 0. Marker `ran=1` then `completed=1`. `review-http.txt`:

```
executor_name=0
operator_pack=not_compiler
gateway=up
health=200
ready=not_probed
probe_routes=404
picker_metadata=200
revoke=committed
warm_hit=200
revoked_after_deadline=401
restart_revoked=401
auditor_metadata=200
outage_warm=200
outage_after_deadline=401
outage_restart=401
restored=200
forged_header=403
session_binding=401
query=400
sql=501
sql_without_bearer=501
secret_channels=clean
tamper_file_sha=refused
pack_digest_check=declared
foreign_listen=refused
body_limit=501/413
stall=408 elapsed_ms=805
concurrency_held=1 health=200 wait_ms=604
waiter_cut=0
completed=1
```

Stdout, stderr, and `review-http.txt` contain no `ooct_` or `oost_` text. `bearer-log.exit=0`. The gateway child was started with an empty environment plus `PATH`. Stderr and `/proc/<pid>/cmdline` did not contain the runtime password.

The picker token returned its own project, principal, role, policy `pv1`, knowledge `kv1`, and the picker pack. The same role at policy `pv2` was not returned. The auditor column name was absent. Forged `X-OO-Role`, `X-OO-Project`, and `X-OO-Scope` each returned 403 `FORBIDDEN`. A session token returned 401 `UNAUTHENTICATED`. A query string on `/health` returned 400 `QUERY_NOT_ALLOWED` and the marker was not echoed. `POST /v1/sql` returned 501 `SQL_UNSUPPORTED` with a bearer and again with no bearer. The body `select 1` was not echoed. `/slow` and `/hold` were 404 on the production router.

Revoke was committed in PostgreSQL, then a request about 1 second after the first 200 stayed 200, and a request at 5.2 seconds from that same start returned 401. A new process still returned 401 for the revoked token. After a live auditor response, the endpoint file was pointed at `127.0.0.1:1`. The warm request stayed 200. The request at 5.2 seconds returned 401. `/ready` stayed `database=not_probed`. A restarted process during that outage returned 401. Restoring the endpoint returned 200 with the auditor column and without the picker column.

A pack whose table-file SHA-256 was 64 zeros exited 1 and did not write `listen.addr`. Listen `0.0.0.0:9` exited 1 and did not write `listen.addr`. On a bounds process, 32 bytes to `/v1/sql` returned 501 and 33 bytes returned 413. The 33-byte payload was not echoed. A metadata request against a TCP listener that accepts and does not speak TLS returned 408 at 805 ms. A health request started 200 ms later returned 200 after 604 ms. One global permit held it. The waiter was not cut.

## Operator pack

The fixture artifact has protocol `oo.builder.role-pack/v1`, embedded file text, and no `compiler` field. Its `pack_digest` equals `manifest_sha256` because the fixture writes it that way. The loader checks `pack_digest` as 64 lowercase hex. It checks each file SHA-256, byte length, and `manifest_sha256` against the embedded text. A substituted `pack_digest` of `ab` repeated 32 times, with the file hashes left valid, started and was returned in the metadata JSON. That value was not equal to `manifest_sha256`. The response did not include file text. An auditor token against a catalog that contained only the picker pack returned 403 and did not include the picker column.

This child loads an operator JSON directory. It does not call `oo-builder`. The returned `pack_digest` is the declared field. It is not the compiler digest over canonical file records.

## Database

Private PostgreSQL 17.10, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, arm64. Container `oo-a337-pg` id `ed9e2bd1db73047df9c4cd09b1da58918c81ba18ffa74b40657fdae8311fc9ce`. Memory 1 GiB, 1 CPU, pids 256, no host port, privileged false, `CapDrop` ALL plus `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, `SETGID`. Database `oo_a337`. SAN `DNS:oo-a337-pg`. The builder fixture held `ca.crt`, `token-admin.pass`, and `token-runtime.pass`. It did not hold a key or `server.crt`. Roles `oo_token_admin` and `oo_token_runtime` were created. `oo-a327-pg` and `oo-a334-pg` were left running.

## Earlier attempts kept

| directory | container | exit | stop |
|---|---|---|---|
| `linux/preoverlay/` | `c6234a3c8066` | 1 | `cargo fetch` rejected `-p` |
| `linux/preoverlay-2/` | `366542ce47db` | 101 | archive tests passed; private-test Clippy `collapsible_if` |
| `linux/preoverlay-3/` first start | none | 3 | host `cargo` was already running; no container |
| `linux/preoverlay-3/` | `205a4c59a081` | 0 | this report |

`linux/preoverlay-2/review_http.failed.rs` is the test source from the Clippy failure. The passing test collapses that `if`. Product files were not edited.
