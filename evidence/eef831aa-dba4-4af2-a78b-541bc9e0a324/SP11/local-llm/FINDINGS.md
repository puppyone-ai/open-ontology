# Findings

Same-session review of the frozen contract `oo.local-llm-relay/v1` and the evidence already on disk. No second builder, no new test, and no source edit. `shasum -a 256 -c linux/proof-10/source-after.sha256` from `testing/real-db/local-llm` exited 0 this turn. Reviews sit outside the development cap.

## Checks that hold

The crate is its own workspace, edition 2024, `rust-version` 1.94.0, package `oo-local-llm` 0.1.0. It is not a root workspace member. `forbid(unsafe_code)` stays. `std::env::remove_var` is not called. `reject_redirect_env` returns `config` when `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` (either case), `OPENAI_API_KEY`, `OPENAI_BASE_URL`, or `OPENAI_API_BASE` is set. Launch scripts unset those names before start. The process does not log the values.

`serve_on` accepts upstream `127.0.0.1` and, outside `cfg(test)`, port 8317 only. The production binary calls `fixed_upstream()`. `upstream_url` rejects a non-loopback address (`non_loopback_upstream_is_rejected`). `parse_listen` accepts `127.0.0.1` with a nonzero port and rejects `0.0.0.0`, a LAN address, and port 0.

The upstream client is cleartext HTTP to `http://127.0.0.1:8317` plus an allowlisted path. Redirect policy is none. A 302 fixture returns HTTP 502 to the caller and the body does not contain `location` (`redirect_is_not_followed`). The models client uses `https_only` and the caller CA. Dialogue and description-provider were not edited, so their HTTPS-only clients are unchanged.

Proof-10 on Rust 1.94.0 arm64 ran these 16 tests, 0 failed, 0.62s, then two empty harnesses for the bins:

- `allow::tests::allowlist_is_exact`
- `behavior::forwards_models_and_selects_on_the_client_side`
- `behavior::listen_is_only_ipv4_loopback`
- `behavior::non_loopback_upstream_is_rejected`
- `behavior::oversized_response_is_dropped`
- `behavior::post_chat_reaches_fixture_once`
- `behavior::redirect_is_not_followed`
- `behavior::rejects_other_paths_without_dialing` (404 with zero upstream hits, and a 80-byte body over a 32-byte cap returns 413 with zero upstream hits)
- `behavior::slow_upstream_stops_at_the_deadline`
- `behavior::third_connection_is_busy` (two holds, third response 503)
- `candidate::tests::accepts_one_stop_candidate`
- `files::tests::redirect_env_absent_is_config_ok`
- `grammar::tests::bearer_class_uses_dialogue_bounds`
- `grammar::tests::model_id_matches_dialogue_rule`
- `select::tests::selects_first_grammar_id_and_hides_the_rest`
- `upstream::tests::production_upstream_is_exact`

`cargo clippy --locked --offline --all-targets -- -D warnings` exited 0. Stderr is the checking line and `Finished dev profile [unoptimized + debuginfo] target(s) in 0.42s`.

The live host chain matches the contract's output protocols. Models: HTTP 200, id `grok-4.3`, 18 of 18 grammar matches, bearer length 6, class `below_minimum`. Chat: HTTP 426, `candidate` false. Docker with no bearer: HTTP 401, parse `fail`. Provider bodies, SQL text, and token bytes are absent from the JSON files. A scan of the evidence tree found `PRIVATE KEY` only as source and contract text, and the token path only as the path string in `host/smoke.sh`. No `*.key` file is in the owned trees. `host/ca.pem` and `host/leaf.pem` are the public certificates.

Lock pair check on proof-10 is `lock_versions=subset`. The first generate had 18 versions outside the fixed archive (`crypto-common` 0.1.7 through `zeroize` 1.9.1, including `wasm-bindgen-futures` 0.4.79). Those were pinned back to archive versions as direct exact deps. `wasm-bindgen-macro` and `wasm-bindgen-macro-support` are not direct dependencies. `sha2` 0.10.9 and `base64` 0.23.1 are pin entries. The relay source does not call them.

`is_sql_candidate` classifies one stop choice. Nothing in this crate calls `execute_trusted_select` or spawns `query-pg`.

## Residual items

The header cap of 8192 bytes is implemented as a silent close (`Fault::close`) before dial. There is no dedicated unit test for that close. Body 413, path 404, busy 503, response drop 502, redirect 502, deadline, TLS, and concurrency are tested. No further proof was added.

The host listener that produced HTTP 200, 426, and 401 was built with Homebrew rustc 1.98.1. The behavioral tests and Clippy ran on the pinned image's rustc 1.94.0. Those are different compilers. The 1.94 Linux bins from proof-11 served the Docker 401 probe. The 1.98.1 Mach-O bins served the host 200 and 426 calls. Both binary trees were deleted with cleanup.

Chat HTTP 426 is the provider status on one POST. The same relay returned HTTP 200 for `GET /v1/models` in the same smoke. The body was not stored. There was no retry and no change to follow an upgrade.

`host/smoke.exit` 1 is the missing-bin probe. `host/reach.exit` 0 is the later probe. Readers who stop at `smoke.exit` will miss `docker-models.json`.

The authorized token is shorter than dialogue's bearer rule. `DIALOGUE_BEARER_MIN` in this crate is 16, matching `tools/ontology-dialogue/src/propose.rs` `read_bearer` (`16..=256`) on the moving tree. The models client recorded the class and still completed HTTP 200. That success is the relay path. It is not a dialogue proposal.

a300 proof-8 is real on a300's evidence, and its handoff says `live_done=1`. It is not a binary this packet can run. Dialogue is absent from `e8d297f95b32052ea587fe8ad67b6e8493a00300` and from `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`. a300's cargo, target, and Postgres volumes are gone. Compiling dialogue from HEAD would use the moving tree and was not done. No approval file, proposal hash, or `query-pg` request was produced here.

`CONTRACT.md` does not mention a300 proof-8. Updating it would break the proof-10 source hash. The freeze-time paragraph remains the contract text. This file is the later seam.

The deadline path drops the socket. The contract allows close or HTTP 504. The slow-upstream test accepts either a non-200 reply or `Fault::upstream`. It does not require a 504 status line.

Cleanup removed the only keypair that matches the published leaf. Reproducing the listen requires a new CA and leaf. The public PEM files stay as evidence of the SAN that the 401 probe trusted.

## Failed-attempt causes

Proof 1 stopped on the lock subset gate before tests. Proof 2 stopped because `cargo update --precise` cannot move `js-sys` to 0.3.105 while `wasm-bindgen-futures` 0.4.79 is locked. Proof 3 is the lock that later proofs reused; the compile failed on a `const fn` match and on unsafe `remove_var`. Proof 4 failed on `slice::windows` taking `usize`. Proof 5 failed because rustls rejects a CA certificate as the end entity. Proof 6 failed because the fixture accept loop handled one connection and the hold blocked the second. Proofs 7 through 9 passed the 16 tests and failed Clippy (`collapsible_if`, `redundant_guards`, then `let_and_return`). Each of those logs is kept. Proof-10 is the first run with tests and Clippy both at exit 0.

The host offline build exited 101 because the private `CARGO_HOME` was empty. `cargo fetch --locked` then exited 0 and the rebuild exited 0. That fetch used the existing lock. It did not regenerate it.
