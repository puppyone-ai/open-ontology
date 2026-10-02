# SDK JSON query review

Decision: **request_changes** for the JSON-query SDK only.

| | |
|---|---|
| Delivery | `8efeb230afc6734a2e50eaca3f09fd78300a9c7d` |
| Tree | `7d97299c1681f39db6aa5252d810a3bbb771ef41` |
| Parent | `d2ea6351414303ad1926fbeb99010607db8d2833` |
| Stamp | `0b544cb066292221016899c9b5eceef84e984227` |
| Reviewer | grok-4.7, session `eef831aa-dba4-4af2-a78b-541bc9e0a084` |
| Subject | `sdk: add the bounded Gateway JSON query client` |

The stamp parent is the delivery commit. Its subject is `sdk: record the JSON query delivery commit`. `git diff --name-status` from the delivery to the stamp is only:

- `M evidence/eef831aa-dba4-4af2-a78b-541bc9e0a073/SP09/sdk-json-query/HANDOFF.md`
- `M evidence/eef831aa-dba4-4af2-a78b-541bc9e0a073/SP09/sdk-json-query/REPORT.md`

Acceptance used `git archive` of the delivery commit. The moving workspace was not the tree under test. The probe tree is that archive plus `crates/oo-sdk/tests/a084_probe.rs` (`probe/probe-tree-diff.txt`). This review did not edit source, manifests, the lock, `ci/deny.toml`, or any other evidence directory, and it did not stage or commit.

## Why request_changes

The Linux arm64 Rust 1.94 focused smoke exited 0, and the independent oracle harness exited 0. Those exits do not clear the slice. Probe 2 still returns short bearers and a short SQL string from the error accessors, retries a 9-digit `Retry-After`, and leaves a percent-encoded token form and a base-URL path in scrubbed gateway text. `cargo deny --locked` against the delivery `ci/deny.toml` exits 4 because locked `webpki-root-certs` 1.0.9 is `CDLA-Permissive-2.0`.

Full SP09-W01, AT-API-01, OpenAPI, exports, discover, describe, resolve, search, freshness, `from_sandbox_env`, query cancellation, the CLI, real Gateway, real PostgreSQL, and amd64 stay pending. They are listed so the slice boundary is visible. They are not the reason for this decision.

## What this slice is

`oo-sdk` is the production-shaped client for `POST /v1/query` JSON. It calls frozen `oo_core::api::parse_api_json` before send and on the response. It sends no SQL to Postgres, keeps no result cache, and does not read a token from a file or the environment. `oo_sdk::UNSUPPORTED` is `export`, `arrow`, `discover`, `describe`, `resolve`, `search_values`, `freshness`, `from_sandbox_env`, `query_cancellation`, `openapi_client`. The probe observed that list.

Contract: `contracts/sdk/json-query/README.md` at the delivery commit. Docs read at that commit: `docs/08-agent-interface.md`, `docs/10-security.md`, `docs/17-rust-implementation.md`, `docs/02-technology-stack.md`. Supply policy: `supply-chain/policy.md`. Deny config: `ci/deny.toml` (no `targets` key, `all-features = false`, `no-default-features = false`).

## Developer check: Linux arm64 Rust 1.94

Evidence: `linux-smoke/`. Run `7402787d-72a5-4328-ad9f-269f55dddb4c`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `pull=never`. `RUSTUP_TOOLCHAIN=1.94.0`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Container `oo-a084-msrv-7402787d-72a5-4328-ad9f-269f55dddb4c`. Network `oo-a084-7402787d-72a5-4328-ad9f-269f55dddb4c-net` (bridge, not host). Own cargo and target volumes. No socket, no HOME mount, `HOME=/tmp`, `--cap-drop ALL`, `no-new-privileges`, memory 8g, cpus 4. One labeled builder.

Commands, all `--locked`:

- `cargo fetch --manifest-path Cargo.toml` exit 0
- `cargo test --manifest-path Cargo.toml --offline -p oo-sdk` exit 0
- `cargo clippy --manifest-path Cargo.toml --offline -p oo-sdk --all-targets -- -D warnings` exit 0

Results: lib 5 passed, `http_query` 18 passed, `proxy_env` 1 passed, `tls_query` 1 passed, doctests 0 passed. `docker-run.exit` 0. Inspect: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`. Phase exits `fetch`, `test`, `clippy`, `clippy-component`, and `worst` are 0.

`linux-smoke/observe/tree-linux-arm64.txt` and `tree-linux-edges.txt` select `aws-lc-rs`, `aws-lc-sys`, `rustls`, `rustls-platform-verifier`, `rustls-native-certs`, `openssl-probe`, and `zeroize`. They do not list `webpki-root-certs`, `r-efi`, `sqlx`, `native-tls`, `flate2`, `quinn`, or `ring`. `tree-all.txt` lists `webpki-root-certs v1.0.9` and both `r-efi v5.3.0` and `r-efi v6.0.0`.

This is a focused developer smoke. It is not the release matrix. amd64 was not built.

## Independent HTTP and TLS oracle

Corrected run: container `oo-a084-probe2-7402787d-72a5-4328-ad9f-269f55dddb4c`, `--network none`, same Rust 1.94 image, same cargo and target volumes, `cargo test --manifest-path Cargo.toml --locked --offline -p oo-sdk --test a084_probe -- --nocapture --test-threads=1`. `SSL_CERT_FILE=/certs/ca-b.pem`. `SSL_CERT_DIR` unset. `HTTP_PROXY`, `HTTPS_PROXY`, and `ALL_PROXY` set to `http://127.0.0.1:9`. Inspect: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=none`. Harness line: `test a084_independent_oracle ... ok`, `1 passed`, `finished in 5.61s` (`probe/console-2.log`). Report: `probe/probe-report-2.json`, 94 cases, 7 with `contract_ok` false.

The harness writes `contract_ok` and still exits 0 when that flag is false. Exit 0 is the test process. It is not an approval.

`probe/probe-report.json` has the same SHA-256 as `probe/probe-report-1.json` (`ad35f79f…`). That first run is kept. It is not the corrected oracle. On run 1, `SSL_CERT_DIR=/certs` contained the private CA, so `platform_roots_reject_private_ca` completed an HTTP hit, and `raw_response` added a second `Content-Length` on the declared-over-cap case. Run 2 fixed both probe mistakes. The six product failures below are stable across both runs.

Expected POST bodies were handwritten in the probe and compared with the bytes on the socket. Full body, 113 bytes, `body_equals_handwritten: true`:

`{"client_context":{"purpose":"answer","turn":1},"format":"json","max_rows":10,"sql":"select 1","timeout_ms":1000}`

Minimal body: `{"format":"json","sql":"select 1"}`.

### Reproduced defects

1. **Short bearer in `message()` and `hint()`.** `scrub_text` (`crates/oo-sdk/src/wire.rs:111-117`) replaces the token and `Bearer {token}` only when `token.len() >= 8`. Probe 2: length 6 and length 7, `message_has_token` true, `hint_has_token` true, `display_has_token` false, `result` `gateway`. Length 8 is stripped (`token_len_8` passed). The contract says accessors return message and hint only after the token is stripped. `docs/10-security.md:108` requires credential-pattern redaction of errors. `Debug` and `Display` of `QueryError` stay on the code, status, and retry flag (`error.rs:146-160`, `209-224`) and did omit the token.

2. **Short SQL in `message()`.** The same function replaces SQL only when `sql.len() >= 24` (`wire.rs:118-120`). Probe 2: SQL length 14, `message_has_sql` true, `display_has_sql` false. A longer SQL case passed.

3. **Percent-encoded token form survives.** Token length 8 is stripped as raw text. The gateway message still contained `Abcd%45%4612` (`message_has_percent_form` true, `message_has_token` false). `scrub_text` does not decode percent-encoding before the search.

4. **Nine-digit `Retry-After` is retried.** The contract (`README.md` retry section) says a larger `Retry-After` is not shortened and is not retried, and a missing, duplicated, or non-decimal value uses 200 ms, then 400 ms, then 800 ms. `parse_retry_after` (`client.rs:199-209`) returns `None` when the header is empty, longer than 8 characters, or contains a non-digit. `None` then takes the backoff path (`client.rs:186-196`). Probe 2 `retry_after_9_digits_not_retried`: `hits` 2, `elapsed_ms` 238, `result` `ok`. An 8-digit value `00000003` and a 3-second value each stopped at 1 hit with `result` `gateway` (`elapsed_ms` 26 and 32). A 1-second value inside `max_wait` retried once (`hits` 2, `elapsed_ms` 1043). Fractional `1.5` and an HTTP-date used backoff, which matches the contract's non-decimal rule.

5. **Base-URL path remains after origin replacement.** `endpoint_origin` (`wire.rs:131-138`) replaces `scheme://authority` when that origin is at least 8 characters. Probe 2 `base_url_path_remainder`: `origin_left` false, `path_left` true. The mount path is still a locator in the accessor text. `Debug`/`Display` of the error omit the message.

6. **`HeaderValue::set_sensitive` is absent.** `BearerToken::new` (`token.rs:19-32`) builds `Bearer {token}` and never calls `set_sensitive(true)`. `Debug` and `Display` of `BearerToken` are `BearerToken([redacted])`. Probe 2 `debug_redacts_token`: token debug is that string, and client debug does not contain the token. This review did not capture an SDK log line that printed the header. The missing sensitive flag is a source gap. The reproduced leak is the accessor path above.

### Controls that held on probe 2

- Door rejects duplicate keys, unknown fields, high-precision numbers, and NUL in SQL before a client call (`client_not_called` true).
- Typed `max_rows` 0 and 2001, and `timeout_ms` 0 and 120001, are `local.request_rejected` with `accepts` 0 (`typed_rejection_opened_no_socket`).
- Eighteen endpoint strings were rejected with `accepts` 0 and the expected `EndpointError` kind: raw `..`, `.`, empty segment, a base that already ends in `/v1/query`, percent-encoded `%2e%2e` and userinfo as `Characters` (the parser rejects `%` and `@` before URL parse), query, fragment, backslash, `::ffff:127.0.0.1`, `10.0.0.1`, `0.0.0.0`, production `http://127.0.0.1`, `ftp`. `http://127.0.0.2` and `http://[::1]` / expanded `::1` completed a loopback request.
- Authorization on the canonical request matched `Bearer` plus the probe token. Header names were `content-type`, `accept`, `authorization`, `user-agent`, `host`, `content-length`. `accept-encoding` was empty and the client did not advertise gzip. A gzip body was `protocol.rejected`.
- Redirect: origin hit 1, `protocol.redirect`, target accepts 0.
- Proxy listener on `127.0.0.1:9` saw `proxy_accepts` 0 while the proxy environment variables were set.
- Success requires the JSON content type. `charset=utf-16`, an extra parameter, HTML, a 200 that carries an error envelope, and a status that disagrees with `ErrorCode` are protocol failures. Nullable cells pass. Numeric cells are rejected. Row width, row count, and the row budget are `protocol.rejected`. A duplicate response key is `protocol.rejected`.
- Framing: duplicate `Content-Length`, and `Content-Length` plus chunked, are `protocol.conflicting_framing`. A declared length above the cap is `protocol.declared_too_large` with 1 hit. A chunked body over the cap is `protocol.body_too_large`.
- Retry subset: only a complete retryable 429 `RATE_LIMITED` or 503 `OVERLOADED` whose status matches. Cap is 3 extra attempts (`hits` 4, authorization stable, bodies equal, `elapsed_ms` 30 on the immediate-success follow-ups). `RetryPolicy::none` is 1 hit. Unauthenticated, wrong 429 code, `retryable: false`, `AUDIT_UNAVAILABLE`, `INTERNAL`, `TIMEOUT`, and a partial 429 are 1 hit. The overall deadline bounded a wait (`elapsed_ms` 1036, `hits` 2, `result` `gateway`). A fresh client uses its own token.
- TLS: `RootTrust::OnlyPem` with CA A completed HTTP against the localhost certificate while `SSL_CERT_FILE` was CA B (`http_hits` 1). OnlyPem rejected CA B's certificate, the wrong hostname, and the expired certificate with `http_hits` 0 and `transport.connect`. An empty PEM bundle is rejected. With `SSL_CERT_DIR` unset, platform roots rejected the private CA (`http_hits` 0, `transport.connect`). `openssl-probe` and `rustls-native-certs` are on the Linux graph because `rustls-platform-verifier` probes the process store. That platform path is separate from `RootTrust::OnlyPem`, and the Linux edges tree does not select `sqlx`.
- Slow headers: `transport.timeout`, display says the client stopped, `display_contains_cancel` false, `backend_cancel` `pending_real_gateway_pg`. Dropping the future closed the client read (`server_read_after_abort` `eof`). A client timeout or a dropped future is pending real Gateway and PostgreSQL cancellation (`docs/17-rust-implementation.md:36`).

### Observations that do not drive the decision

- **JSON null `max_rows`.** `bounded_option` (`crates/oo-core/src/api.rs:833-845`) maps JSON null to `None`. Probe 2 `door_null_max_rows`: `door_rejected` false, `max_rows_is_none` true, `client_not_called` true. The encoder still omits nulls (`wire.rs:36-43`). The comment there says the frozen schema rejects null for those integers and objects. The door accepts null as the omitted default. The client did not open a socket for that document. In-memory 0 and 2001 still fail in `encode_request` before connect.
- **`127.255.255.255`.** The endpoint treats `127.0.0.0/8` as loopback, so this address is not `NotLoopback`. The probe then reported `transport.connect` and `hits` 0. The connect failure is this container's result. It is not evidence that the SDK rejected a loopback address.
- **Run 1 platform and declared-length rows.** Those two `contract_ok: false` rows are probe setup, corrected on run 2. They are not product defects.
- **Author Clippy narrative.** a073 `REPORT.md` says an earlier locked clippy failed on `dead_code` in the shared test helper and on `clippy::manual_is_multiple_of` (`bytes.len() % 4`), and that the raw log was not archived. The a073 evidence tree has `host/cargo-clippy-2.log` ending `EXIT:0`, plus pre-smoke and linux-post `clippy.exit` files. There is no `cargo-clippy-1.log`. `host/cargo-test-1.log` and `host/cargo-test-2.log` end `EXIT:101`. `host/cargo-test-3.log` ends `EXIT:0`. This review's clippy on the delivery archive exited 0. That does not recreate the missing failure log, and this review does not invent its text. The current helper allows `dead_code`, and `tls_query.rs` uses `bytes.len() & 3 != 0`.

## Supply

Registry comparison of the delivery lock against the parent, started `2026-10-02T01:54:03.891529Z`, 168h cutoff `2026-09-25T01:54:03.891529Z`. Evidence: `supply/summary.json`, `supply/added-versions.json`.

| Check | Result |
|---|---|
| Delivery registry versions | 250 |
| Parent registry versions | 119 |
| Added | 131 |
| Removed | 0 |
| Checksum mismatches | 0 |
| Registry failures | 0 |
| Yanked | 0 |
| Published inside the 168h window | 0 |
| Declared `rust-version` above 1.94.0 | 0 of 101 present |
| No declared `rust-version` | 30 |

The thirty crates with no `rust-version` field are recorded in `summary.json` (`cfg_aliases` 0.2.2 through `zerovec-derive` 0.11.6, including `dunce` 1.0.5, `getrandom` 0.2.17, and `yoke-derive` 0.8.2). Absence of the field is not proof that the crate is above the floor, and it is not proof that it is below the floor. No published `rust-version` on the added set is above 1.94.0.

`added_selected_linux` includes `aws-lc-rs` 1.18.1, `aws-lc-sys` 0.45.0, `openssl-probe` 0.2.1, and `rustls-native-certs` 0.8.4. `added_only_target_all` includes `r-efi` 6.0.0 and `webpki-root-certs` 1.0.9. This review made no root dependency change.

### cargo-audit and cargo-deny

Isolated tools volume, `CARGO_HOME=/tools`, image and toolchain as the smoke, network bridge. First container `oo-a084-tools-…` used `bash -lc`, lost `/usr/local/cargo/bin`, and exited 4 with `command not found` (`supply/tools-console-1.log`, `tools-docker-1.exit`). That run did not audit the lock.

Second container `oo-a084-tools2-7402787d-72a5-4328-ad9f-269f55dddb4c` installed `cargo-audit` 0.22.2 and `cargo-deny` 0.20.2 with `--locked` and printed those versions. Install exits are 0. Commands: `cargo-audit audit --json` and `cargo deny --format json --config /deny/deny.toml --locked check`, config mounted read-only from the delivery `ci/deny.toml`.

- `audit.exit` 0. `supply/cargo-audit.json`: database advisory-count 1279, last-updated `2026-10-01T22:25:27+02:00`, lockfile dependency-count 267, `vulnerabilities.found` false, `count` 0, `list` empty.
- `deny.exit` 4. Inspect `ExitCode=4`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=bridge`. Stdout redirect `supply/cargo-deny.json` is 0 bytes. The JSON diagnostics were captured on the container log and copied to `supply/cargo-deny.stderr.ndjson` (8 lines).

Deny summary: advisories errors 0, bans warnings 6, licenses errors 1, sources errors 0. The license error is `webpki-root-certs` 1.0.9, code `rejected`, span `CDLA-Permissive-2.0`, message `failed to satisfy license requirements`, parents `rustls-platform-verifier` 0.7.0 from `oo-sdk` 0.1.0 and from `reqwest` 0.13.5. The six warnings are duplicate versions of `base64`, `getrandom`, `r-efi`, `syn`, `synstructure`, and `windows-sys`. `multiple-versions` is warn in the existing config. This review did not add an allow entry and did not edit `ci/deny.toml`.

The Linux arm64 graph used by the default product artifact does not select `webpki-root-certs` 1.0.9. The locked target-all graph does, and the checked-in deny config evaluates that graph. CDLA-Permissive-2.0 stays outside the allow list. The linux/host selection does not close this gate. Audit count 0 does not admit the supply chain. OSV coverage of the RustSec database is one scan.

### r-efi

`r-efi` 6.0.0 and 5.3.0 publish `MIT OR Apache-2.0 OR LGPL-2.1-or-later`, `rust-version` 1.68. The crate body has `AUTHORS` and `README.md`. The license-file walk found `AUTHORS` and no `LICENSE`, `LICENCE`, `COPYING`, or `NOTICE`. `r-efi` 6.0.0 is on the target-all graph and absent from the Linux arm64 edges tree. `cargo-deny` accepts the crate because one `OR` alternative is on the allow list, and reports only the duplicate-version warning.

`supply-chain/policy.md` is stricter. A dual license needs `dual_license_choice` and LICENSE evidence that covers every declared license. LGPL needs `weak_copyleft_review`. `LGPL-2.1-or-later` is also the `+` or-later form the policy fails closed. `AUTHORS` is not a license locator (`policy.md` §2: basename `LICENSE`, `LICENCE`, `COPYING`, or `NOTICE`). This review does not weaken `ci/deny.toml` and does not treat deny's `OR` success as policy admission. No exception record was added.

### aws-lc and the platform store

`aws-lc-sys` 0.45.0 license text is the conjunction `ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0)`, `rust-version` 1.71.0. `aws-lc-rs` 1.18.1 is `ISC AND (Apache-2.0 OR ISC)`, `rust-version` 1.71.0. Fetched bodies include `LICENSE` and `aws-lc/LICENSE`. The license prose keeps ISC on older BoringSSL-derived code and Apache-2.0 on newer AWS-LC and BoringSSL code. Each SPDX token in the expression is on the deny allow list. An `AND` keeps every obligation (`policy.md`). There is no separate reviewed-exception document. The bodies were read from the registry checkout recorded in `supply/crate-bodies.txt`.

The smoke target volume contains `/target/debug/build/aws-lc-sys-a105f2b1660f99a0/out/libaws_lc_0_45_0_crypto.a` (13,465,754 bytes) and 371 `.o` files. Build-script output is 283,788 bytes, stderr 0 bytes, `cmake` count 0, `Building with: CC` once, symbol prefix `aws_lc_0_45_0`, `cargo:rustc-link-lib=static=aws_lc_0_45_0_crypto`, target `aarch64-unknown-linux-gnu` (`supply/aws-lc-build.txt`). cmake was not the compiler used for that object set.

`dunce` 1.0.5, selected on Linux through the aws-lc build, is `CC0-1.0 OR MIT-0 OR Apache-2.0` and has no `rust-version` line. `CC0-1.0` is not on the deny allow list. Deny did not emit a `dunce` license error, which is consistent with deny accepting one allowed `OR` alternative (`MIT-0` or `Apache-2.0`). Policy still wants a `dual_license_choice` and evidence for every declared license. Same rule as r-efi, with no LGPL token. Recorded here so the `OR` mismatch is explicit. No policy change.

`rustls-platform-verifier` 0.7.0 is `MIT OR Apache-2.0`, `rust-version` 1.85. On Linux it pulls `openssl-probe` 0.2.1 (`MIT OR Apache-2.0`) and `rustls-native-certs` 0.8.4 (`Apache-2.0 OR ISC OR MIT`). Those crates read `SSL_CERT_FILE` and `SSL_CERT_DIR`. Probe 2 shows OnlyPem still verifying with the caller PEM while `SSL_CERT_FILE` pointed at a different CA, and platform mode failing closed against that private CA when `SSL_CERT_DIR` was unset. SDK Gateway trust and a private-CA-only Postgres source graph are different graphs. This Linux edges tree does not select `sqlx`.

## a073 record, preserved

a073 reports two host test failures and a green rerun. This review re-read the files and did not rewrite them.

- `host/cargo-test-1.log` ends `EXIT:101` (`--lib`).
- `host/cargo-test-2.log` ends `EXIT:101` (`--test http_query`).
- `host/cargo-test-3.log` ends `EXIT:0`.
- `host/cargo-clippy-2.log` ends `EXIT:0`.
- The earlier clippy failure log a073 describes is not in that evidence tree.

## Release gates still open

Passed as developer checks: focused Linux arm64 fetch, `oo-sdk` tests, and clippy `-D warnings`; registry checksum, yanked, and 168h checks on the 131 added versions; `cargo-audit` vulnerability count 0.

Still open:

- `cargo-deny` license error for `webpki-root-certs` 1.0.9 `CDLA-Permissive-2.0` on the locked target-all graph (exit 4).
- Policy admission for `r-efi` 6.0.0 `MIT OR Apache-2.0 OR LGPL-2.1-or-later` (no license-file locator, no `weak_copyleft_review`).
- `dunce` 1.0.5 three-way `OR` including `CC0-1.0`, same deny-versus-policy gap, no LGPL.
- aws-lc conjunctive obligations are real and the native archive was compiled; a conjunction record that lists each duty is not in this review's evidence.
- Thirty crates publish no `rust-version`.
- Audit 0 is one advisory database, not a full supply-chain admission.
- amd64, real Gateway, real PostgreSQL, OpenAPI, exports, discover, cancellation, CLI, and SP09-W01 / AT-API-01.

## Repair, for the author

Strip every accepted bearer and the request SQL in `scrub_text`, including percent-encoded forms of the token, with no length floor. Keep `Debug` and `Display` free of message, hint, token, SQL, and URL. Treat an all-digit `Retry-After` whose parsed duration exceeds `max_wait` as not retried, including values longer than 8 digits. Replace the whole base URL the client used, path included, or omit it. Call `set_sensitive(true)` on the authorization header value. Leave `ci/deny.toml` unchanged. The CDLA crate has to leave the graph that this deny config checks, or the release gate stays failed. Do not add a CDLA allow. An r-efi policy record has to name one admissible license, point at real LICENSE evidence for every declared license, and include `weak_copyleft_review` if LGPL stays in force; otherwise the crate leaves the locked graph. This review does not apply those edits.

## Cleanup

Removed after `review.md` and `findings.json` were written. Each `docker rm`, `docker network rm`, and `docker volume rm` exited 0. `rm -rf` of the temp directory exited 0.

- Containers: `oo-a084-msrv-7402787d-72a5-4328-ad9f-269f55dddb4c`, `oo-a084-probe-…`, `oo-a084-probe2-…`, `oo-a084-tools-…`, `oo-a084-tools2-…`
- Network: `oo-a084-7402787d-72a5-4328-ad9f-269f55dddb4c-net`
- Volumes: `…-cargo`, `…-target`, `…-tools`
- Temp: `/tmp/oo-a084-7402787d-72a5-4328-ad9f-269f55dddb4c`, including the probe private keys

A follow-up listing shows no container, volume, or network with that UUID, and the temp directory is gone. `oo-a085-pg1710-…` and `oo-a080-msrv-f669cc94` were still running and were left in place. No `docker prune`. No git add, commit, or history change. `git diff --cached` is empty. The evidence directory is untracked. The workspace also has unrelated dirty files under `crates/oo-builder`, `crates/xtask`, and `Cargo.lock`; this review did not modify them.
