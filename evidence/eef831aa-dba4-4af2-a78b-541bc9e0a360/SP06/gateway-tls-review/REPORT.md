# Gateway TLS review

Verdict: APPROVE_LIMITED.

Reviewed commit: `97b6a9eb9caa2b6059d82bdb98b703bf5ee202d8` ("gateway: add the external TLS metadata listener").
Parent: `2c032402ee3b58801f30dbbcefddee8733e383b1`.
Review: `eef831aa-dba4-4af2-a78b-541bc9e0a360`.
Workspace HEAD when the archive was taken: `f862683945a6e5a8803e3f9450d0f51cde5a98ad`.

This approval covers the public TLS identity transport at that commit. E45, the parent gateway goal, OAuth, an IdP, token issuance, the session proxy, public SQL, and full SP06 auth remain open.

The proof is a fresh `git archive` of `97b6a9eb`. Product files were not edited. `Cargo.lock` blob `45d24ef0a15b2db24fc0b9dd02826cd1056ecae8` equals the parent. Root pins stayed `axum =0.8.9`, `rustls =0.23.45`, `tokio-rustls =0.26.5`.

## Preserved smoke, not rerun

`linux/smoke` is the first builder. It exited 101 at review Clippy, after the archive checks and the focused suites had already passed. Those passes were kept. `linux/smoke-2` reused the cargo volume `oo-a360-eef831aa-cargo` and the target volume `oo-a360-eef831aa-target`, checked the six archive hashes, and ran only the review oracle.

| Check | Exit | Result |
|---|---|---|
| `rustc` 1.94.0 | 0 | `aarch64-unknown-linux-gnu` |
| review file absent from the archive | 0 | `pre-edit.exit` |
| archive hashes match the commit | 0 | six files, `archive-match.exit` |
| `cargo check --locked -p oo-gateway` before the review overlay | 0 | |
| author Clippy `-D warnings` | 0 | finished in 4.95s |
| `http_gateway_loopback` | 0 | 8 passed in 0.47s |
| `http_gateway_tls` default | 0 | 5 passed, 1 ignored, 1.62s |
| `tls-count` | 0 | stdout contains `5 passed` and `1 ignored` |
| fixture `rustc` and `role-pack-bundle` | 0 | binaries reused by smoke-2 |
| author ignored live test | 0 | 1 passed in 6.19s, marker `completed=1` |
| review Clippy on the first oracle | 101 | `clippy::useless_format` only |
| `docker-run.exit` | 101 | container `25977a0579a44c9945fcbe042346b9d00beba35a9c3905c6f91248239a1a4d72` |

Author Clippy command, from `run/inside.sh`: `cargo clippy --locked -p oo-gateway --lib --bin oo-gateway --test http_gateway_loopback --test http_gateway_tls --test http_gateway_pg_live --test http_gateway_live -- -D warnings`.

The author live test is `verified_peer_metadata_uses_the_compiled_pack_and_the_leaf_uri`. It compiled the two-role pack and talked to private database `oo_a360`. `OO_A326_LIVE` / `http_gateway_live` was left ignored and was not run.

a350 packet check: `source/a350-sha256-summary.txt` records 402 entries, check exit 0, 402 ok, 0 bad. Five `FAILED` substrings in the check log are filenames under `linux/proof*/FAILED.txt`.

## Smoke-2, the review oracle

`linux/smoke-2/inside-ok.txt` is `2026-10-07T22:16:53Z`. `docker-run.exit` is 0. Container `0d1be1c15c56830d7df8efa9cd88824741fda405c84d83d65c3f4cbcd3dae36e` was removed. Product hashes after the tests match `linux/smoke/work-sha256.txt` (`product-unchanged.exit` 0). `Cargo.lock` was not rewritten (`lock-unchanged.exit` 0).

| Check | Exit | Result |
|---|---|---|
| reused work tree and pack binaries | 0 | |
| Clippy component | 0 | absent, then `rustup component add clippy` |
| `cargo clippy --locked -p oo-gateway --test http_gateway_tls_review -- -D warnings` | 0 | finished in 0.30s |
| review tests, default | 0 | 3 passed, 1 ignored, 1.48s |
| `verified_leaf_does_not_select_the_token_tuple` | 0 | 1 passed in 11.14s, marker `completed=1` |

`review-summary.txt` records the oracle lines: complete Content-Length accepted, truncated / missing length / unterminated rejected as `read`, tail at or beyond the declared length accepted, empty length 0 accepted, bad key refused before `listen.addr`, directory-only argv, forged header 403, internal 403, body limit 413, anonymous / other CA / DNS-only refused, handshake dropped then a later health 200, plaintext not HTTP 200, current tuple taken from the store, leaf URI taken from the verified certificate, forged tuple headers 403, wrong audience 401, `oost_` session token 401, internal issuance 403, revoke 401 after the original 5000 ms deadline, closed database 401 after that deadline.

The default author TLS test, which this review ran once in smoke and did not rerun, already checked `/health` and `/ready` with the client certificate and no bearer, `/ready` `database=not_probed`, the verified workload URI, a second certificate changing only that URI, forged proxy headers, `POST /internal/v1/session-tokens` 403, SQL at 32 bytes 501, and one extra byte 413. Loopback covered the 408 deadline, the single concurrency permit, and the executor-name ban.

## Platform

Smoke container `25977a0579a44c9945fcbe042346b9d00beba35a9c3905c6f91248239a1a4d72`. Smoke-2 container `0d1be1c15c56830d7df8efa9cd88824741fda405c84d83d65c3f4cbcd3dae36e`.

- Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. No pull.
- `--cap-drop ALL`, `no-new-privileges`, `Privileged=false`, memory `6442450944`, `NanoCpus=1000000000`, `OOMKilled=false`, `pids-limit 4096`.
- Network `oo-a360-eef831aa-net`. `HOME=/tmp`. Private cargo and target volumes. `/tmp` tmpfs `rw,nosuid,nodev,noexec,size=1g`.
- Smoke-2 mounts: review source, prior smoke evidence, pg fixture, and `inside-continue.sh` were read-only. Evidence was read-write. No host network, host socket, or host home.
- Capacity before each builder: `own=0 global=0 host_cargo=0`.

PostgreSQL for the author live test was `oo-a360-pg` from `pg/up.sh` (`pg/up.exit` 0), then removed by that launcher. Smoke-2 started a new `oo-a360-pg` because that removal had deleted it. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, arm64. Memory 1g, 1 CPU, `Privileged=false`, `CapDrop ["ALL"]` plus `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. Published ports `{}`. Database `oo_a360`. SAN `DNS:oo-a360-pg`. `roles.txt` ends in `roles_ready`. Container id `1192f2128aee9de4e7fe3f8c3914fa169fbe5fe6ec3a101df17c97627bc37691`.

`invocation.txt` in both attempts is a summary line. Passwords stayed in files under `/tmp/oo-a360-eef831aa-dba4-4af2-a78b-541bc9e0a360/secrets`. The builder fixture had no private key and no `server.crt`.

## Failed attempt kept

`linux/smoke` review Clippy exit 101. The diagnostic is one `clippy::useless_format` at the overlaid `http_gateway_tls_review.rs:146`, a `format!` with no placeholders. Author Clippy had already exited 0. The oracle literal is now a byte string. Smoke-2 also requires the stalled handshake to close with EOF or a connection reset inside 400 ms. No `#[allow]`. Product files were not edited.

## Out of this approval

E45, the parent gateway goal, OAuth, an IdP, token issuance, the session proxy, public SQL, a readiness database probe beyond `not_probed`, durable audit, and runsc remain open. `POST /internal/v1/session-tokens` stayed 403. An `oost_` token stayed 401. `POST /v1/sql` stayed 501.
