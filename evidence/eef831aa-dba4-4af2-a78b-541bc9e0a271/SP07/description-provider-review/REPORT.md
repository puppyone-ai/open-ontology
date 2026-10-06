# Independent review of the S5-D03 HTTPS description provider

## Verdict

S5-D03, the committed HTTPS description provider: **APPROVE_LIMITED**.

The verdict is for provider commit `aba067efe4b19bae868e15e23157ebbcf0a4969f` as it sits in fixed integrated archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`, tree `7e83a381c8a90c5f61f3ce73947eb7ac441381e9`. That archive already contains the provider and the repaired description library. The decisive runs are `linux/smoke-2`, before any review overlay, and `linux/proof`, with the independent TLS fixture. `linux/smoke` is a preserved harness failure and stays in this directory.

This verdict covers the frozen provider transport proved below, built against the repaired library already in that archive. It does not accept full S5. It does not accept the parent goal. It is not a release. It does not replace the a264 limited approval of S5-D01 and S5-D02, and it does not reopen that review. Acceptance counts, the ledger, the plan, and product files were not edited. a263 is the author record. It is not this approval.

## Scope

Contract read: `tools/description-provider/CONTRACT.md` at the fixed archive. Author records read: a263 `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`. Library review read: a264 `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`.

The provider is the standalone workspace `tools/description-provider`. `description-provider run --config PATH` reads one command on stdin, calls `prepare_description_request`, performs one HTTPS chat completion, and calls `complete_description_candidates`. The outcome protocol is `oo.builder.description-provider-outcome/v1`. The compiler id is `oo-builder/description-provider/v1`. `effect` on a successful candidate is `review_only`.

Contract lines 124 through 137 keep `billing_certificate` and `residency_certificate` false. `caller_residency_approval` is `explicit`. `declared_cost.amount` is `total_tokens` times `per_total_token`, exact decimal, same scale. A compiler `budget_failure` is recorded as `budget_reason=caller`. The precise reason stays on `transport.budget_reason`. The compiler statements stay `llm_transport=not_used`, `residency=not_approved`, and `cost=not_metered` after a performed call. The sibling `transport` object is where the call is recorded.

PostgreSQL was not started. No product file and no dependency file was edited. a207 remains the only writer of the connector manifest and the root lock. `cargo update` was not invoked. `linux/proof/cargo-update-invoked.txt` is `no`.

## Fixed bytes

`aba067ef` is an ancestor of `9951cda7`. `git diff --stat aba067ef 9951cda7 -- tools/description-provider` is empty. `tools/description-provider/src/lib.rs` is blob `f4fccebcb31471e028079c22f7faaef3046159e4` at both commits. The repaired library blob `crates/oo-builder/src/description_candidate.rs` is `f783ae95f1f7398da18b6c47bcc87941db49f7a3` at `9951cda7` and at `56b791368d7b5ec050aa36ac91d33bc2234bc4d2`.

`linux/smoke-2/source-identity.txt` records the archive blobs before the passing smoke. `phases/source.exit` is 0 on smoke, smoke-2, and proof. Content SHA-256 inside the proof container, from `linux/proof/source-sha256.txt`:

- provider `lib.rs` `84e0f52020f8274bf235ede35f75132baf7437f08ce289cd2b7615ec606c4e93`
- author `tests/provider.rs` `798c50c8eb6e159f5c21978f17294eff6e7cbbe08c6b304506317fdea54e2911`
- review `tests/provider_a271.rs` `6cec0432423460bc3d41d551418cfdb3c05be03af322e530dfadd67c66f18414`
- provider lock `d6b28e14bceb66512cc44acbbdce9af84754c860bef199384eb0b1662f9258cb`
- `description_candidate.rs` `c392dd9ef7dfff45ea9f6298b257e8fc4bdeca04784acbfbb696ef644f884954`
- root lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`

The producer content hash is the a264 patched hash. The provider lock was not regenerated.

## How the bytes were built

Each builder used `git archive 9951cda7` mounted read-only, copied it, and moved `rust-toolchain.toml` aside. `RUSTUP_TOOLCHAIN=1.94.0`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `cargo 1.94.0 (85eff7c80 2026-01-15)`. Debian 13.

Each builder had 6 GiB (`Memory=6442450944`), 1 CPU (`NanoCpus=1000000000`), `--cap-drop ALL`, `--security-opt no-new-privileges`, `--pull never`, private bridge `oo-a271-eef831aa-net` (`internal=false`), `HOME=/tmp`. No docker socket. No host home. No PostgreSQL. Recipe origins are in `ORIGIN.md`.

Workspace HEAD recorded in `repo-head.txt` for smoke, smoke-2, and proof is `eb9fdf46313d8a1d3cc4e25eb1fa09be28e3842e`. The archive stayed `9951cda7`.

## Preserved harness failure

`linux/smoke` ran before the review overlay existed. `docker-run.exit` 1. `smoke.exit` 1. `phases/fetch.exit` 1. Container `7e301b88036ddd00eec43acca5da98071fe0eff4818e14c62c63861e02d68c63` removed. OOMKilled false. Privileged false. NetworkMode `oo-a271-eef831aa-net`. Capacity `own=0 global=0 host_cargo=0`.

`linux/smoke/fetch.log` is:

```
error: unexpected argument '-p' found
```

The harness passed `-p` to `cargo fetch`. `cargo fetch` has no `-p`. Source identity, the absent overlay, and the toolchain-aside tree diff had already exited 0. No provider test ran. This is a review harness mistake. It is not a provider defect. The directory was left as it finished.

## Smoke-2

`linux/smoke-2` is the same archive with the fetch command corrected to `cargo fetch --manifest-path Cargo.toml --locked` for the root workspace and a separate locked fetch for the provider manifest. The overlay was still absent (`overlay-absent.txt` is `overlay_absent`). `docker-run.exit` 0. `smoke.exit` 0. Container `f90aebde9885c47d50d7203e3d713a92088a91ea9265f4f1285e360045e12c24` removed. OOMKilled false. Privileged false. Same memory, CPU, capability, and network limits. Capacity `own=0 global=0 host_cargo=0`.

Offline root fetch exited 101 (`no matching package named chrono-tz` on the volume left by the failed smoke). The following locked fetch exited 0. Provider locked fetch exited 0. `description-candidate` built. The existing provider tests passed: 2 library, 23 integration, finished in 6.11s. Empty binary and doctest targets reported 0. That is the author's 25 tests. Clippy was not run.

Lock subset: `registry=248 path=4 missing=0 mismatch=0`. Direct exact pins in the provider manifest: 14. The tool walks every registry name, version, source, and checksum. Path packages are allowed only for `description-provider`, `oo-builder`, `oo-core`, and `oo-jobs`. A copy of the provider lock with the first checksum replaced by 64 zeros was rejected: `registry=248 path=4 missing=0 mismatch=1`, stderr `mismatch ahash 0.8.12`. The tampered copy was removed. Both locks after the run matched the archive. Tree diff is only `rust-toolchain.toml` moved aside.

## Proof

`linux/proof` copied `overlay/provider_a271.rs` into the writable tree as `tools/description-provider/tests/provider_a271.rs`. The archive mount stayed read-only and does not contain that file. `docker-run.exit` 0. `proof.exit` 0. Container `dfb25ef45420d29a0996d7479acfec178f67627fa4f040cff69097ae87981dbd` removed. OOMKilled false. Privileged false. Memory 6442450944. NanoCpus 1000000000. NetworkMode `oo-a271-eef831aa-net`. Capacity `own=0 global=1 host_cargo=0`. `containers-before.txt` names `oo-a274-chain` and `oo-a274-pg`. Those were not stopped.

Fetch was offline and exited 0. Focused tests on the same container:

- library: 2 passed
- author `provider`: 23 passed in 6.07s
- `provider_a271`: 7 passed in 2.48s

The seven tests are `valid_review_only_matches_both_compiled_clis`, `redirect_is_not_followed`, `partial_body_stays_incomplete`, `malformed_envelope_emits_no_candidate`, `deadline_is_a_timeout`, `secret_echo_admits_no_candidate`, and `usage_budget_is_declared_not_a_certificate`.

`valid_review_only_matches_both_compiled_clis` builds the shop catalog, prepares a request, and compares three documents: the provider library outcome, `CARGO_BIN_EXE_description-provider run --config`, and `OO_A271_DESCRIPTION_CANDIDATE complete_candidates`. The provider CLI and the library outcome are equal. The compiler CLI document equals `candidate`. Stderr on the provider CLI is empty and the exit is 0. Proxy variables `HTTPS_PROXY`, `HTTP_PROXY`, and `ALL_PROXY` point at `http://127.0.0.1:9`, and `OPENAI_BASE_URL` points at `https://example.invalid/v1`. One POST goes to `/v1/chat/completions` on the local mock. The raw body contains the temperature lexeme `"temperature":0` and does not contain `"temperature":0.`. The response format is `json_schema`, name `oo_builder_description_response_v1`, `strict` true, protocol const `oo.builder.description-response/v1`. The bearer check compares header bytes by length and does not print the token. The token is absent from argv and from stdout.

On that success, `transport.status` is `complete`, `http_status` is 200, `finish_reason` is `stop`, and usage is 11 prompt, 7 completion, 18 total. `declared_cost` is USD at the configured per-token price `0.000002`. The amount equals that price times 18, computed in integer units at the price scale (`0.000036`). `billing_certificate` and `residency_certificate` are false. `caller_residency_approval` is `explicit`. Candidate statements stay `not_used`, `not_approved`, and `not_metered`. `effect` is `review_only`. Dataset names are `public.customer` then `public.purchase`, in that order, equal to `selected_datasets` and to the prepared request table order. Structural draft hashes equal `compile_captured_pg_catalog` of this archive. The shop schema hash checked by the fixture is `bcb019d92270bf12092a3f47697b25719cc087a1ac4b7c56a10cf6dd00294c4c`.

The other six tests, each with one mock hit:

- redirect: `provider_failure` / `redirect`, `http_status` 302, `declared_cost` null, no candidates, statement `llm_transport=not_used`
- partial close: `incomplete` / `partial`, candidate outcome `incomplete`, no candidates, `declared_cost` null
- HTTP 200 body `not-json`: `provider_failure` / `malformed_envelope`, no candidates
- hold past a 2000 ms deadline: `provider_failure` / `timeout`, `declared_cost` null, statement `cost=not_metered`
- bearer echo on HTTP 200: `provider_failure` / `secret_echo`, usage 18, declared amount present, both certificates false, no candidates, statement `llm_transport=not_used`
- valid 18-token body with `max_total_tokens` 10: transport `budget_failure` / `tokens`, candidate `budget_failure` / `caller`, usage and declared amount present, both certificates false, statements `not_metered`, `not_used`, and `not_approved`, no candidates

Clippy component log is `info: downloading component clippy`, exit 0. Command:

```
cargo clippy --manifest-path tools/description-provider/Cargo.toml --locked --offline --all-targets -- -D warnings
```

`clippy.exit` 0. The log has no `warning:` line and ends `Finished dev profile [unoptimized + debuginfo] target(s) in 1m 07s`. Lock subset again `registry=248 path=4 missing=0 mismatch=0`, direct pins 14. Both locks unchanged. Tree diff is the review test plus `rust-toolchain.toml` moved aside.

## Cleanup

Own volumes `oo-a271-eef831aa-cargo` and `oo-a271-eef831aa-target`, network `oo-a271-eef831aa-net`, and `/tmp/oo-a271-eef831aa-dba4-4af2-a78b-541bc9e0a271` were removed after the proof. No prune and no foreign cleanup. See `CLEANUP.txt`.

`SHA256SUMS` is rewritten by `linux/manifest/finalize.sh` after this report, `FINDINGS.md`, `HANDOFF.md`, and `CLEANUP.txt` exist. The tool is `tools/sha256_manifest.rs`. The compile console in `linux/manifest/` is the earlier run of that tool, before these four files existed.
