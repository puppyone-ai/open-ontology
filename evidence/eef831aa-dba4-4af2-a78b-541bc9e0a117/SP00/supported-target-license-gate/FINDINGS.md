# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a117`. Evidence under `SP00/supported-target-license-gate/`. Product write is `ci/deny.toml` only.

## F1 — Config is the two GNU Linux triples

`[graph].targets` gained `"aarch64-unknown-linux-gnu"` and `"x86_64-unknown-linux-gnu"`. Diff is four added lines (`identity/deny.toml.diff`). `all-features = false` and `no-default-features = false` stay. No license allow, exception, ignore, clarify, exclude, or exclude-dev. No CLI `--target`. Citations: `docs/11-deployment-and-operations.md` line 27, `docs/17-rust-implementation.md` line 97, spec `REQ-CORE-06`, `REQ-OPS-02`, P14, `AT-OPS-01`. wasm stays outside the supported matrix (`supply-chain/harness-dependency-delta.md` line 29).

## F2 — Lock and manifests are the pre-edit bytes

`Cargo.lock` sha256 `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f`, git blob `15ae1071c7236a956547e3f1d37d627b2cbebfce`. It still contains `tiny-keccak` 2.0.2 checksum `2c9d3793400a45f954c52e73d068316d76b6f4e36977e3fcebb13a2721e80237` and `webpki-root-certs` 1.0.9 checksum `b96554aa2acc8ccdb7e1c9a58a7a68dd5d13bccc69cd124cb09406db612a1c9b`. `oo-sdk` and `oo-connectors` manifest hashes match the pre-edit worktree file. `identity/product-diff-after.txt` is empty.

## F3 — Deny on the new config exited 0 for all four sections

Pinned cargo-deny 0.20.2, binary sha256 `44296adb80fd7d3a6ce8737e2806cca409a96757798b399f32f1b535df74884c`. Command `cargo deny --config /src/ci/deny.toml --locked check`. `raw/deny.exit` is `0`. Stdout is `advisories ok, bans ok, licenses ok, sources ok`. Stderr is 43201 bytes of `warning[duplicate]` only (11 warnings, including getrandom ×3 and num-bigint). Zero stderr matches for the CC0/CDLA crate names.

The license result is the union of the two configured triples. cargo-deny 0.20.2 applies `--filter-platform` only for a single target. Two triples take one metadata snapshot, then krates 0.21.2 `include_targets` keeps edges whose cfg matches either triple. Optional crates are kept only when a kept edge enables them. Debug output of the pruned set was not captured.

## F4 — Linux cargo-tree activation is runtime-rng and native certs

For both `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu`, before and after the edit, feature counts are `compile-time-rng` 0, `tiny-keccak` 0, `const-random` 0, `webpki-root-certs` 0, `runtime-rng` 2, `rustls-native-certs` 2, `arrow-array` 2. Tree and metadata exits are 0. ahash's normal feature list is `default,getrandom,runtime-rng,serde,std`. Inverse from `oo-connectors` shows `runtime-rng` under `arrow-array` 60.0.0. Inverse from `oo-sdk` shows `rustls-native-certs` 0.8.4 under `rustls-platform-verifier` 0.7.0. The native-certs cfg is `cfg(all(unix, not(target_os = "android"), not(target_vendor = "apple"), not(target_arch = "wasm32")))`.

`jsonschema` 0.57.0, `jsonschema-value`, and `referencing` also depend on ahash with target null and feature `serde` plus ahash's default (`std`, `runtime-rng`). That is a second ahash path. It does not select `compile-time-rng`.

Inverse stdout for `tiny-keccak` and `webpki-root-certs` is empty on both Linux triples, exit 0, with cargo's 176-byte "nothing to print" warning. Absence is the counts plus the non-empty trees, together with that warning. The warning alone is not the proof.

## F5 — The two Linux graphs are not identical to each other

x86_64 `tree-normal` adds `core_detect` 1.0.0, `multiversion` 0.9.0, `multiversion-macros` 0.9.0, and `rustversion` 1.0.23. They are already in the lock. Deny checked the union, so their licenses are inside the exit-0 result. Each architecture's own metadata and trees are byte-identical before and after the config edit (`linux/graph-digests.txt`).

## F6 — wasm32 and all-target still contain both offending edges

wasm32 feature counts: `compile-time-rng` 1, `tiny-keccak` 4, `webpki-root-certs` 2, `rustls-native-certs` 0. Inverse stdout shows `tiny-keccak` 2.0.2 from `const-random-macro` / `const-random` / `ahash feature "compile-time-rng"` / `arrow-array` 60.0.0, and `webpki-root-certs` 1.0.9 from `rustls-platform-verifier` to `oo-sdk`. all-target counts keep `compile-time-rng`, `tiny-keccak`, `webpki-root-certs`, and `rustls-native-certs` together. Those outputs are failing-graph evidence. They are not an approval of wasm or of `--target all`.

Declared licenses: `tiny-keccak` CC0-1.0, `webpki-root-certs` CDLA-Permissive-2.0 (`raw/upstream-manifests/license-lines.txt`). Neither identifier is in the allow list.

## F7 — Linux metadata nodes still list tiny-keccak

Corrected `facts.txt` (`in_resolve` by exact package id) reports `tiny-keccak` 2.0.2 `in_resolve=yes` on both Linux filters, and ahash's resolve feature array includes `compile-time-rng`. `resolve_edge` shows `ahash` → `const-random` and `const-random-macro` → `tiny-keccak` with `target=null`, and `arrow-array` → `ahash` with both the wasm32 and the not-wasm32 cfgs. `webpki-root-certs` is absent from the Linux resolve and present on wasm32 and all-target.

`--filter-platform` did not recompute feature activation down to the cargo-tree set. `const-random` is optional and switched on by `compile-time-rng`, which the wasm32 edge requests. cargo-tree and cargo-deny drop that edge for the two GNU Linux triples. `facts.broken-id-match.txt` is the earlier scan that marked every package `in_resolve=no` because it split ids on whitespace. Do not cite those files as membership.

`rustls-platform-verifier` has a dev-dependency on `webpki-root-certs` with target null. The config does not set `exclude-dev`. Third-party dev edges are ignored by cargo-deny 0.20.2's default krates scope. The retained all-target failure is the normal wasm32 edge.

## F8 — a110 deny exit 4 still stands

Not rerun. `deny.exit` file hash `7de1555df0c2700329e815b93b32c571c3ea54dc967b89e81ab73b9972b72d1d`, contents `4`. Summary `advisories ok, bans ok, licenses FAILED, sources ok`. Same lock blob and same pre-edit deny blob as this gate. Empty targets were why both crates were checked. This gate does not delete that evidence and does not rewrite it.

## F9 — Advisory freshness is one database checkout

Advisories ok against git rev `f8dee89e1b2f2f1eaf548312df7655fe5202a302` (`2026-10-02 22:27:46 +0200`). No separate `cargo audit` was run in this session.

## F10 — Graph compare wrote a false mismatch file

`linux/graph-compare.txt` says DIFFER for metadata and tree sha256 sidecars because those files embed `before/` or `after/` in the hashed path text. The hash fields match. `linux/GRAPH_MISMATCH` exists for that reason. `Cargo.lock` matched. Production Linux graphs did not move, so the stop-and-propose condition was not met. No PostgreSQL and no product test run followed.

## F11 — Builder scope

Arm64 Rust 1.94.0 only. `RUSTUP_TOOLCHAIN=1.94.0` overrode `rust-toolchain.toml` channel `1.98.1`. x86_64 lines are target-filtered graphs from that builder, not an x86_64 compile or runtime. Host rust comm count stayed 0. One own container at a time. No PostgreSQL. Own container, network, and volumes removed. No prune. Attempt 1, attempt 2, and the first rescan failure are kept.

## F12 — What this does not sign

Byte-identical source, manifests, and lock, plus identical before/after Linux graphs, are why existing source, type, and SDK runtime proof was kept. Not rerun: a110 deny, a110 audit, PostgreSQL, Arrow gold, SDK TLS, `cargo test -p oo-connectors`, amd64 binaries, the all-workspace matrix, and `cargo deny --target all`. No fresh TLS, Arrow, runtime, or dual-arch acceptance is claimed. The change cannot sign full types, SP00, or release. Supply and release remain for independent fixed review.
