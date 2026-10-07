# Gateway HTTP dependency foundation

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a313`. Fixed archive `b91b67bee0f603725b2cb7cc9c284588a2699392`. This card pins Axum 0.8.9, the already-locked Tower 0.5.3, and tower-http 0.6.11 on the root workspace, and proves those crates with a separate loopback probe. Status remains UNAPPROVED. `oo-gateway` does not depend on the pins. There is no public Gateway API in this commit.

The decision record is `execution/decisions/gateway-http-dependency-baseline.md`. The pre-edit plan is `CONTRACT.md` (SHA-256 `2084027f478d354ec678fa27dd03ed693a93959fa7cb24f169e08cdf8478be40`). `CONTRACT-ADDENDUM.md` records the cargo 1.94 command change, the overlap pins, and `httpdate`.

## What changed

Root `Cargo.toml` adds `[workspace.dependencies]` for `axum =0.8.9` (`http1`, `tokio`), `tower =0.5.3` (`limit`, `make`, `timeout`, `util`), and `tower-http =0.6.11` (`limit`, `timeout`). Default features are off. `Cargo.lock` SHA-256 is still `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Git blob `2990f901ea98a7b465f462cce8742bc5c51c13aa`. `rust-toolchain.toml`, consumer manifests, child locks, specs, and `supply-chain/dependency-baseline.json` are untouched.

The probe workspace `probe/` is evidence. Its lock adds five registry crates and changes none:

| Name | Version | Checksum |
|---|---|---|
| axum | 0.8.9 | `31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90` |
| axum-core | 0.5.6 | `08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1` |
| httpdate | 1.0.3 | `df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9` |
| matchit | 0.8.4 | `47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3` |
| mime | 0.3.17 | `6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a` |

`httpdate` is required by Hyper 1.11.1 feature `http1`. It was absent from the root lock. Publication `2023-08-13T10:55:54.391658Z`, yanked false, MSRV 1.56, license `MIT OR Apache-2.0`. The project relies on Apache-2.0 for that OR. matchit keeps MIT AND BSD-3-Clause.

## Linux proof

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. rustc `1.94.0 (4a4ef493e 2026-03-02)` because `RUSTUP_TOOLCHAIN=1.94.0`. One builder, 6442450944 bytes, 1 CPU, CapDrop ALL, private network `oo-a313-eef831aa-net`, no Postgres, no host cargo.

| Run | Exit | What it shows |
|---|---|---|
| `linux/smoke/attempt-1` | fetch 1 | cargo 1.94.0 rejects `cargo fetch -p`. Manifests were not edited. |
| `linux/smoke/` | check 0 | Frozen `oo-arch-check` `cargo check --locked` before the pin edit. Manifests unchanged. |
| `linux/proof/attempt-1` | docker 1 | Offline arch-check on the edited manifest passed. `lockdiff` selfcheck failed: `jsonwebtoken` does not contain the substring `jwt`. |
| `linux/proof/attempt-2` | subset 1 | Fetch succeeded. The parser treated the lock header `version = 4` as a package version. |
| `linux/proof/attempt-3` | test 101 | Subset `added=5 changed=0 denied=0`. Three HTTP tests passed. The permit test saw `echo_started` 2 while `/hold` ran, because `ConcurrencyLimitLayer` allocates a new semaphore per route. |
| `linux/proof/attempt-4` | clippy 101 | Shared `GlobalConcurrencyLimitLayer` : 4 tests passed. `#[must_use]` on `router` failed `clippy::double_must_use` because `Router` is already `must_use`. |
| `linux/proof/` | docker 0 | Container `434367fb3bfdcd3b83f8cc1732acd6874ca96a0939d172d1ba050cb9ab803bc5`. `inside_ok=2026-10-07T10:45:40Z`. metadata 0, check 0, fetch 0, selfcheck 0, subset 0, root exact 0, licenses 0, test 0 (4 passed, 0.37s), clippy 0, lock 0. `OOMKilled=false`. |

The green loopback checks are real `127.0.0.1` HTTP/1.1: echo ignores `Authorization` and the query string, 32 bytes is kept, 33 bytes is 413, `/slow` is 408 before the 2 second sleep, and one shared permit blocks `/echo` while `/hold` is in the handler.

`linux/proof/SHA256SUMS` (1054 lines) is the in-container `sha256_manifest` of `/evidence` taken before `launch.sh` wrote the green run's `hostconfig.txt`, `tmpfs.txt`, `mounts.txt`, `docker-run.exit`, and `container-removed.txt`, and before the `inside_ok` line was appended to `console.log`. License trees under `linux/proof/licenses/` are the checksum-verified `.crate` extractions for axum, axum-core, matchit, mime, httpdate, tower, and tower-http. Attempt 4 keeps its own copy from the clippy-failure run.

Recipes are `recipes/ORIGIN.md`. Launch and report shells come from reviewed a204 / a200 / a199 files, with the command and path changes recorded there. `tools/sha256_manifest.rs` is the unmodified a204 source.

## Reviews after the foundation commit

Commit `773c461104729d6c289eae8640c7c2a28207c447` is the pin and the evidence above. Four review tasks then ran against that commit. Reports: `linux/review-1-pins/REPORT.md`, `linux/review-2-lockdiff/REPORT.md`, `linux/review-3-rerun/REPORT.md`, `linux/review-4-scope/REPORT.md`. The fresh-target container `83cb5ecac838c956bc82054f1219002d8804e3720eb7e693707e9b4eb6d51897` passed the same four loopback tests and clippy. Those reviews leave the baseline UNAPPROVED.

## Scope that stayed put

No workspace member was added. Full `oo-arch-check` tests were not run; those tests call `cargo metadata` without `--no-deps` and would fetch the whole workspace. The boundary that did run is `cargo metadata --locked --no-deps` and `cargo check --locked --offline -p oo-arch-check`. No dual-architecture build. No JWT library. No child-lock rewrite. a207's connector commit `672129aa` remains reachable.
