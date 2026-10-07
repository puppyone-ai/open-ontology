# Gateway HTTP dependency baseline

Status: **UNAPPROVED**.
UUID: `eef831aa-dba4-4af2-a78b-541bc9e0a313`
Fixed archive: `b91b67bee0f603725b2cb7cc9c284588a2699392`
Root writer for this change: a313. a207 history stays in the repository.
Evidence: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a313/SP00/gateway-dependencies/`
Plan saved before the manifest edit: `CONTRACT.md` SHA-256 `2084027f478d354ec678fa27dd03ed693a93959fa7cb24f169e08cdf8478be40` at `2026-10-06T12:48:34Z`.

This record is the workspace pin and the evidence-only loopback probe. It is not a Gateway service, not a public API, and not a product-unit acceptance. Consumer crates keep compiling against the previous lock until a later card approves an archive and opts in.

## Rules used

- `docs/02-technology-stack.md` §1 and §6. The candidate series is Tokio 1.x / Axum 0.8.x. A release younger than 7 days does not enter the default artifact. Cargo default features are not a security configuration.
- `docs/07-gateway.md`. Gateway tokens are opaque hashes. This change adds no JWT, OIDC, or OAuth crate.
- `docs/17-rust-implementation.md` §2. HTTP routing is Axum / Tower. Body size, concurrency, and the request deadline are bounded. A Tower timeout does not cancel PostgreSQL work. This card does not implement that cancellation.
- `supply-chain/policy.md`. Exact version, official source, publication time, MSRV, features, license bytes, and checksum. `audited` stays false. No advisory scanner is claimed. `supply-chain/dependency-baseline.json` is not edited.

Age is measured from publication time to the contract stamp `2026-10-06T12:47:28Z`.

## Root manifest

`Cargo.toml` gains one `[workspace.dependencies]` table. No member uses it. The member list is unchanged, so `oo-arch-check` does not see a new crate. `Cargo.lock` is byte-identical to the fixed archive.

| File | SHA-256 |
|---|---|
| `Cargo.toml` after the pin | `b772f8fb4873885fcfac747eb24163559031bb9192ba99e8a8472571c75822bc` |
| `Cargo.lock` | `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` |

Git blob of `Cargo.lock` remains `2990f901ea98a7b465f462cce8742bc5c51c13aa`.

```toml
axum = { version = "=0.8.9", default-features = false, features = ["http1", "tokio"] }
tower = { version = "=0.5.3", default-features = false, features = ["limit", "make", "timeout", "util"] }
tower-http = { version = "=0.6.11", default-features = false, features = ["limit", "timeout"] }
```

`rust-toolchain.toml` still says `channel = "1.98.1"`. Proof and smoke set `RUSTUP_TOOLCHAIN=1.94.0`. The toolchain file is not edited.

Root lock inventory against the frozen bytes (`linux/proof/lockdiff-root.stdout`, exit 0):

```
added=0
removed=0
changed=0
exact=pass
```

## Official pins

Metadata is the crates.io version documents under `upstream/`. Every row below has `yanked=false`. Primary pages: `https://crates.io/crates/<name>/<version>`, `https://docs.rs/<name>/<version>`, download `https://static.crates.io/crates/<name>/<version>/download`. The proof hashed each `.crate` and the checksum matched the crates.io field (`linux/proof/licenses/verified.txt`).

| Crate | Version | Published (UTC) | MSRV | License field | Checksum |
|---|---|---|---|---|---|
| axum | 0.8.9 | 2026-04-14T07:55:20.258398Z | 1.80 | MIT | `31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90` |
| tower | 0.5.3 | 2026-01-12T18:43:05.227461Z | 1.64.0 | MIT | `ebe5ef63511595f1344e2d5cfa636d973292adc0eec1f0ad45fae9f0851ab1d4` |
| tower-http | 0.6.11 | 2026-05-18T12:51:03.996149Z | 1.64 | MIT | `4cfcf7e2740e6fc6d4d688b4ef00650406bb94adf4731e43c096c3a19fe40840` |
| axum-core | 0.5.6 | 2025-12-27T09:19:34.014637Z | 1.78 | MIT | `08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1` |
| matchit | 0.8.4 | 2024-07-11T07:22:03.735375Z | not declared | MIT AND BSD-3-Clause | `47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3` |
| mime | 0.3.17 | 2023-03-20T15:05:04.272177Z | not declared | MIT OR Apache-2.0 | `6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a` |
| httpdate | 1.0.3 | 2023-08-13T10:55:54.391658Z | 1.56 | MIT OR Apache-2.0 | `df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9` |

Axum 0.8.9 is the newest published Axum and is on the 0.8 line. tower 0.5.3 is the version already in the root lock. tower-http 0.7.1 exists; Axum 0.8.9 requires `^0.6.8`, so this card reuses locked 0.6.11.

License bytes were extracted from the checksum-verified `.crate` files. axum, axum-core, tower, and tower-http are MIT only. matchit keeps both MIT and BSD-3-Clause. mime and httpdate are MIT OR Apache-2.0; this project relies on Apache-2.0, the same choice the baseline already uses for dual-licensed crates.

## What the probe lock adds

The probe is its own workspace at `probe/`. It is not a workspace member. Overlap crates are exact-pinned to the frozen root lock so the resolver cannot float them. `cargo fetch` on that manifest is the only resolution this card authorized. There was no root `cargo update`.

`linux/proof/lockdiff-subset.stdout` (exit 0), as of `2026-10-06T12:47:28Z`:

```
added=5
axum 0.8.9 31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90
axum-core 0.5.6 08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1
httpdate 1.0.3 df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9
matchit 0.8.4 47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3
mime 0.3.17 6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a
changed=0
denied=0
subset=pass
```

`httpdate` 1.0.3 is the extra beyond the four names in the original contract. Hyper 1.11.1's `http1` feature pulls it. The root lock's hyper entry does not list that edge, because the root hyper features do not enable it. httpdate has no further dependencies. The root lock was not edited to absorb it. `CONTRACT-ADDENDUM.md` records the pin. The denylist covers `jwt`, `jose`, `oidc`, `oauth`, `openid`, `aliri`, `paseto`, and the explicit name `jsonwebtoken`. `denied=0`.

Removed registry triples: none. Changed registry triples: none.

## Features and the loopback behavior

Axum features are `http1` and `tokio` only. `query`, `form`, `json`, `macros`, `matched-path`, `original-uri`, `tower-log`, `tracing`, `ws`, `http2`, and `multipart` stay off. Tower features are `limit`, `make`, `timeout`, and `util`. tower-http features are `limit` and `timeout`. The tower-http `auth` feature stays off.

The probe package `oo-gateway-http-baseline-probe` binds `127.0.0.1:0` and speaks raw HTTP/1.1. It uses `axum::serve`. `/slow` sits under `tower_http::timeout::TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, 150ms)` because `TimeoutLayer::new` is deprecated since tower-http 0.6.7. The handler sleeps 2 seconds. The green run returned 408 before that sleep finished. The outer body layer is `RequestBodyLimitLayer::new(32)`: 32 bytes is 200, 33 bytes is 413, and the rejected payload is not in the response body. The outer concurrency layer is `tower::limit::GlobalConcurrencyLimitLayer::new(1)`. Axum applies `Router::layer` once per route, and `ConcurrencyLimitLayer::layer` allocates a new semaphore, so a per-route limit does not cover `/hold` and `/echo` together. `GlobalConcurrencyLimitLayer` holds one `Arc<Semaphore>` across those clones. The green run held `/hold` and the queued `/echo` did not enter the handler until release. `POST /echo?role=admin&claims=all` with `Authorization: Bearer ...` and `X-OO-Claims` returned the body `ping` only.

Primary API pages used to choose those calls:

- `https://docs.rs/axum/0.8.9/axum/fn.serve.html`
- `https://docs.rs/tower/0.5.3/tower/limit/struct.GlobalConcurrencyLimitLayer.html`
- `https://docs.rs/tower-http/0.6.11/tower_http/timeout/struct.TimeoutLayer.html`
- `https://docs.rs/tower-http/0.6.11/tower_http/limit/struct.RequestBodyLimitLayer.html`

## Proof

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, rustc `1.94.0 (4a4ef493e 2026-03-02)`. One private builder, 6 GiB, 1 CPU, `CapDrop=ALL`, network `oo-a313-eef831aa-net`, no Postgres. Pre-edit smoke of frozen `oo-arch-check` is `linux/smoke/` (`check.exit` 0, manifests unchanged). Smoke attempt 1 is preserved: cargo 1.94.0 rejects `cargo fetch -p`.

Green proof container `434367fb3bfdcd3b83f8cc1732acd6874ca96a0939d172d1ba050cb9ab803bc5`, `OOMKilled=false`, `Privileged=false`, `Memory=6442450944`, `NanoCpus=1000000000`. `inside_ok=2026-10-07T10:45:40Z`. Exits 0: metadata, offline `cargo check -p oo-arch-check`, fetch, lockdiff selfcheck, subset, exact root, licenses, test, clippy `-D warnings`, lock compare. Tests: 4 passed, 0 failed, 0.37s. Preserved failures: proof attempt 1 (denylist selfcheck), attempt 2 (lock header parsed as a package version), attempt 3 (`ConcurrencyLimitLayer`, 3 passed and the permit test failed), attempt 4 (`clippy::double_must_use` on `router`).

`linux/proof/SHA256SUMS` is the Rust `sha256_manifest` inventory taken inside that container before `inside_ok` was appended to `console.log` and before `launch.sh` wrote `hostconfig.txt`, `tmpfs.txt`, `mounts.txt`, `docker-run.exit`, and `container-removed.txt`. Those five files, and any file added after the hash, are outside that inventory.

## Review tasks in this UUID

The same session ran four review tasks after commit `773c461104729d6c289eae8640c7c2a28207c447`. They do not approve the baseline for a consumer crate.

- Pins: live crates.io checksums match. Axum's newest release is still 0.8.9. matchit 0.9.2 and tower-http 0.7.1 are newer and older than 7 days, and Axum 0.8.9 does not accept them (`matchit =0.8.4`, `tower-http ^0.6.8`).
- Lock diff, from the archived commit: `added=5`, `changed=0`, `denied=0`.
- Fresh target volume `oo-a313-eef831aa-review-target`, container `83cb5ecac838c956bc82054f1219002d8804e3720eb7e693707e9b4eb6d51897`: 4 tests passed, clippy exit 0, `inside_ok=2026-10-07T10:52:12Z`.
- Scope: that commit does not touch `Cargo.lock`, `rust-toolchain.toml`, crate manifests, or `supply-chain/dependency-baseline.json`. a207 commit `672129aa` remains an ancestor.

## Later consumer card

A later approved archive may add `axum = { workspace = true }` on the consumer that actually serves HTTP, then `cargo fetch` so the root lock gains only the five added triples above. That card must show a lock diff with `changed=0` for every crate already in `16e6afda…`. Child locks under other evidence trees stay on the old baseline until their own cards say otherwise. This decision does not authorize that consumer edit.
