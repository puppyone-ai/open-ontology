# S5-D03 description provider

## Scope and verdict

This child adds a standalone Rust workspace at `tools/description-provider`. It reads one local config, performs one HTTPS OpenAI-compatible chat completion, and passes the captured response to `complete_description_candidates`. A valid completion yields review-only description candidates. A transport, provider, or budget failure keeps the structural draft and emits no candidate.

The endpoint, CA file, bearer file, origin, and residency approval come from the caller config. The process does not read an ambient proxy, a credential environment variable, or a provider key. The proof uses a local TLS mock and the existing shop fixture of hashed comment cards. It does not call a production network, import a customer row, or use PostgreSQL.

This report does not claim parent acceptance, full S5, publication, a residency certificate, or a billing certificate.

Contract commit `56710cc73cfe1663272ecaa503984e1f663428ad`. Implementation commit `aba067efe4b19bae868e15e23157ebbcf0a4969f`. The contract commit is an ancestor. The implementation parent is `77f8a5a52d1679f65fc5719180808b7236129bdc`.

## Fixed source

Fixed commit `182e2137ee261ed889b1aff5ada6e505a4a17723`. The product proof builds a selective git archive of that commit and overlays only `tools/description-provider`. The root `Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. No existing source, library, bin, manifest, or root lock was edited. a207 remains the connector manifest and root lock writer. a262 was not edited. The preoverlay run of `description_selection_order` passed on the fixed archive. That observation is not full S5 acceptance.

The provider contract was committed before the adapter.

## Recipes

Launch and report shape follow reviewed recipes. The UUID, fixed commit, owned paths, 6 GiB limit, and 1 CPU limit are this child's values. a200 commit `a587031c719a484830d78e493605f48e2056cae4` is the private PostgreSQL recipe. This unit starts no database, so that recipe was not applied.

| Origin | Commit | Path | Git blob |
|---|---|---|---|
| a204 overlay launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay-3/launch.sh` | `03c8e15c60a376ef0a757c062ebd110063a0ffe6` |
| a258 preoverlay launch | `1cd687d3f106fb9bde8c2a4a0b40cf2dddae9368` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a258/SP07/description-candidate-cli/linux/preoverlay/launch.sh` | `259abb131d0181345edec714e5cb8bd0f566dcb5` |
| a258 preoverlay inside | `1cd687d3f106fb9bde8c2a4a0b40cf2dddae9368` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a258/SP07/description-candidate-cli/linux/preoverlay/inside.sh` | `4bf98b9a1f39d60c83c23b1820faa3a041e9a055` |
| a258 proof launch | `1cd687d3f106fb9bde8c2a4a0b40cf2dddae9368` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a258/SP07/description-candidate-cli/linux/proof/launch.sh` | `ded7f91b66619ca1bea122afd8e20683256baaa5` |
| a258 proof inside | `1cd687d3f106fb9bde8c2a4a0b40cf2dddae9368` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a258/SP07/description-candidate-cli/linux/proof/inside.sh` | `c967e03f8afa3082d4c342c11e58f09ff023ec0b` |
| a199 manifest tool | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `f21989651570187fc286462349d59174a6a6b8ef` |
| a253 file hasher | `268e46972458fe53dd54412ab277b2cd09552a82` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a253/SP07/description-candidate/tools/sha256.rs` | `b69a2af13099ce4baf2d04e79c5692d9988ab67d` |

`tools/sha256.rs` and `tools/sha256_manifest.rs` are byte-identical to those blobs. `tools/lock_subset.rs` is a new Safe Rust checker for this child. It compares registry name, version, source, and checksum triples.

## Pre-edit smoke

`linux/preoverlay` on the pinned Linux arm64 Rust 1.94 image, 6 GiB, 1 CPU, private network, dropped capabilities, no pull. The archive contained `description-candidate` and did not contain `tools/description-provider`. `overlay-before` for the later proof says `provider_absent_before_overlay`.

The first offline fetch exited 101 on a cold cargo volume. An online fetch then exited 0, and the following offline fetch exited 0. `cargo update` was not invoked. The root lock hash was unchanged.

- `description_candidate` tests: 10 passed
- `description_candidate_cli` tests: 5 passed
- `description_selection_order`: 1 passed
- `cargo clippy -D warnings` on those targets: exit 0
- `smoke.exit` 0, `docker-run.exit` 0, OOMKilled false, privileged false

`rustc` reported `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Memory was 6442450944 and NanoCpus was 1000000000. Container id `e6c52af04e1cf53e44cdc6e164a1fdc82e2f99df6df5d407cd320f2cf4908cbd`.

## Decisive verification

`linux/proof-7` is the passing run. The container copied the fixed archive, built `description-candidate` from that archive, overlaid the provider tree, and generated the provider lock with `cargo fetch` from a copy of the root lock. It did not run `cargo update`. `tools/lock_subset.rs` reported `registry=248 path=4 missing=0 mismatch=0`. A later offline fetch used `--locked`. The root lock hash after the run was still `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

- library tests: 2 passed
- `tests/provider.rs`: 23 passed, 0 failed, 6.14s
- `cargo clippy --locked --offline --all-targets -- -D warnings`: exit 0
- `smoke.exit` 0, `docker-run.exit` 0, OOMKilled false, privileged false

Same image, 6 GiB, 1 CPU, `CapDrop` ALL, tmpfs `/tmp` with `nosuid,nodev,noexec`. Container id `029ae9a14d0a4e7d4886f6376aec3812f2cc95a43654408648111d024fbf7a6f`.

The 23 integration tests cover an approved origin, the fixture CA, the bearer file, temperature lexeme 0, and JSON Schema output. A valid response has `effect=review_only` and matches `description-candidate complete_candidates` by JSON value. Wrong CA, redirect, HTTP 400, HTTP 500, a partial body, a malformed envelope, malformed content, an oversize body, a second chunk, a deadline, and a known secret in the body or a header emit no candidate. Table, request-byte, token, and prepare-already-budget cases are bounded. Model mismatch, sample policy, residency, temperature `0.0`, and an unknown `billing_certificate` field fail closed. The binary test sets proxy and provider environment variables on the child and still sends one request with the file bearer. A 500 is not retried.

Content SHA-256 of the committed tree, from `tools/sha256.rs`, matches `linux/proof-7/provider-Cargo.lock` for the lock (`d6b28e14bceb66512cc44acbbdce9af84754c860bef199384eb0b1662f9258cb`).

| File | Content SHA-256 | Git blob |
|---|---|---|
| `.gitignore` | `306fd52e74fca6746e12acc750f232de15e71da917756a1270d74c91f5eb7368` | `b83d22266ac8aa2f8df2edef68082c789727841d` |
| `CONTRACT.md` | `78d932548d65f312c9ec7646374c390cea9b6213c303183db04fe0a6b661c67e` | `6eb2382659f50f047101a90bf4f0a1fe6c4b5a2e` |
| `Cargo.lock` | `d6b28e14bceb66512cc44acbbdce9af84754c860bef199384eb0b1662f9258cb` | `ff14fa71781b9655d79b577f0aac80284605b432` |
| `Cargo.toml` | `48202a8f843c3eeef7a967102dbe9f83c5df2e1026e15d9c28797b417af4f4c7` | `edfebdbfa91c5cdf39a53cc7af820247784a9d0d` |
| `src/chat.rs` | `67ee17d9a0567c752b0969cd9a883952fad27831ce08f18381560057bd919393` | `dc6df53aaa0c8bc8c175e472c1636941890919b0` |
| `src/config.rs` | `b861338cec0c6067d734f8d6f4f4ab6bbbc143e22ac5c7af4c58f50107b56cdd` | `b07873fd5adccbdfacbbb09a0eb9d647d73de6c3` |
| `src/error.rs` | `643c98afdefc695f050114a9a2e30941ac27b3c596cf9c1018075f0aa18d0a9e` | `f50ef26540502535b82dc8faecc0aa70a8c2f79f` |
| `src/lib.rs` | `84e0f52020f8274bf235ede35f75132baf7437f08ce289cd2b7615ec606c4e93` | `f4fccebcb31471e028079c22f7faaef3046159e4` |
| `src/main.rs` | `b2a65427601f7891a43a32a89910de502dba85f032e022dff4adf2b2197b5c10` | `b4b33a38253578307415396edf59476935079d93` |
| `src/price.rs` | `de988d74249ddd004ac628810adbe11b2ae75d14e2644620d33cf61ba5b98f5d` | `6e778a361ca7924e3d75ac878e82c4cadd3a2779` |
| `src/transport.rs` | `d374ce9400a7f5aa0923f80128faa8ededa873958ba87f14e12b1bf3be9b09c2` | `4a117912138f1aacde41d573853313c617c220a5` |
| `tests/provider.rs` | `798c50c8eb6e159f5c21978f17294eff6e7cbbe08c6b304506317fdea54e2911` | `161db9fd39c11506b85d9d2fc005ae264d4c9908` |

Copies of those bytes are under `source/blobs/tools/description-provider/`.

## Retained failures

An initial exec of `linux/proof/launch.sh` exited 126 before the script started. It wrote no `console.log`. The same directory was then used for the first container run.

| Directory | What failed | Tests | Docker |
|---|---|---|---|
| `linux/proof` | `E0308` in `chat.rs`: `return` inside a closure | not run | 101 |
| `linux/proof-2` | `E0382` and `E0716` in the integration test | not run | 101 |
| `linux/proof-3` | `E0596`: `child` was not mutable | not run | 101 |
| `linux/proof-4` | `E0277`: `RequestView` had no `Debug` | not run | 101 |
| `linux/proof-5` | clippy `collapsible_if` in `chat.rs` | 2 + 23 passed | 101 |
| `linux/proof-6` | clippy `too_many_arguments`, `needless_question_mark`, `useless_vec` | 2 + 23 passed | 101 |
| `linux/proof-7` | passing run | 2 + 23 passed | 0 |

These directories were not overwritten. Development containers used: the preoverlay plus proof through proof-7. That is 8 of the 12 development slots. No review container was started.

## Limits

The compiler statements stay `llm_transport=not_used`, `residency=not_approved`, and `cost=not_metered`. The provider records the call on a sibling `transport` object. When the captured status is `budget_failure`, the frozen compiler stores `budget_reason=caller`. The specific reason is `transport.budget_reason`.

`declared_cost` multiplies the configured per-token price by reported total tokens. `billing_certificate` and `residency_certificate` are false. `caller_residency_approval` is the explicit config boolean.

The mock server is local. The bearer stays in a mode `0600` file. Assertions compare lengths and do not print the token or a response body. There is no cache, retry, or second endpoint in this child.

True remote provider residency, an owner for that residency, a billing certificate, and full S5 remain later gates.
