# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a115`. Subject commit `99b326b18add833896128ca344d10452b0c34c30`, tree `f397a2acada92905449102873e70908290e2383c`, parent `e0726071950c54383b2fe6c1831e419045b1ebe6`.

Verdict of this analysis: one viable config edit, unscanned. The current deny command stays a failure. No supply approval.

## F1 — Frozen graph is the a110 graph

`Cargo.lock` blob `15ae1071c7236a956547e3f1d37d627b2cbebfce`, sha256 `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f`. `ci/deny.toml` blob `367598cb3768b8c8af698041cd0f84ce6f8fb7be`, sha256 `4320807bfe747fe339ce7bf539a1cff5604e8e0c7677760e2cbc7abc1868624b`. Both match a110's review of `3ca5357930a2c44c42d3061db5fabe577881269a`. `oo-sdk` and `oo-connectors` manifests are unchanged across that range.

## F2 — CC0 edge is wasm32 `compile-time-rng`

`tiny-keccak` 2.0.2 `CC0-1.0` is reached only as:

`oo-connectors` → `arrow-array` 60.0.0 → `cfg(target_arch = "wasm32")` → `ahash` feature `compile-time-rng` → feature `const-random` → `const-random` 0.1.18 default → `const-random-macro` 0.1.16 default → `tiny-keccak` features `default` and `shake`.

`cfg(not(target_arch = "wasm32"))` selects `ahash` feature `runtime-rng` (`getrandom`) and does not select `const-random`. `jsonschema` 0.57.0 enables `ahash` `default` (`std`, `runtime-rng`) and `serde`. It does not enable `compile-time-rng`. `arrow-array` features `async`, `ffi`, `force_validate`, and `pool` do not gate `ahash`. `oo-connectors` already sets `default-features = false` and has no features of its own.

Evidence: a110 `deny.err` and `tree-tiny-all.out`; retained `arrow-array` 60.0.0 `Cargo.toml.orig`; `ahash` 0.8.12 and the const-random crates as cited in `REPORT.md`.

## F3 — CDLA edge is wasm32 roots inside the platform verifier

`webpki-root-certs` 1.0.9 `CDLA-Permissive-2.0` is a normal dependency of `rustls-platform-verifier` 0.7.0 under `cfg(target_arch = "wasm32")` only. The Linux predicate `cfg(all(unix, not(target_os = "android"), not(target_vendor = "apple"), not(target_arch = "wasm32")))` selects `rustls-native-certs` 0.8 and `rustls-webpki`, not the CDLA crate.

Parents in the all-target deny graph are the unconditional `oo-sdk` dependency (`default-features = false`, no extra features) and `reqwest` 0.13.5 feature `rustls`, which enables `dep:rustls-platform-verifier` with no extra features. reqwest's own dependency is off when `all(target_arch = "wasm32", any(target_os = "unknown", target_os = "none"))`. The direct `oo-sdk` edge is what still builds the verifier on wasm32.

`src/verification/others.rs` loads `rustls_native_certs::load_native_certs()` on the Linux predicate and `webpki_root_certs::TLS_SERVER_ROOT_CERTS` on wasm32. `verify_server_cert` takes `server_name` in both builds. An external dev-dependency on `webpki-root-certs` exists and is not the deny parent: cargo-deny 0.20.2 ignores non-workspace dev edges.

`RootTrust::Platform` is the SDK production default. `OnlyPem` replaces anchors and keeps hostname checks. Connector TLS is caller CA plus `verify-full` via `_tls-rustls-ring-no-roots`, and it does not depend on this crate.

## F4 — Empty `targets` unifies those cfgs

cargo-deny 0.20.2 calls `include_targets` only when CLI `--target` or `[graph].targets` is non-empty (`src/cargo-deny/common.rs`). The checked-in file has neither, so wasm32 and Linux dependencies are one graph. That matches a110 (host tree lacks `tiny-keccak`; `--target all` has it) and a084 (Linux arm64 tree lacks `webpki-root-certs`; `--target all` has it).

`all-features = false` and `no-default-features = false` do not split target cfgs. Extra `default-features = false` on a workspace crate cannot cancel a feature unified in from another cfg.

## F5 — Documented hosts are the two GNU Linux triples

Required: `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu`.

Citations: `docs/17-rust-implementation.md` §7 line 97; `docs/11-deployment-and-operations.md` §1.1 line 27; `docs/02-technology-stack.md` §6 line 189 (MSRV ≥ 1.94, workspace `rust-version = "1.94.0"`); spec `REQ-CORE-06`, `REQ-OPS-02`, P14, `AT-OPS-01`. gVisor is the runtime on those Linux hosts, not another triple.

Wasm is explicitly not a supported target matrix (`supply-chain/harness-dependency-delta.md` line 29). macOS, Windows, Android, and musl are not acceptance triples in those sections. `docs/10-security.md` §11 line 155 tells CI to run `cargo deny` and to refuse AGPL/BSL/ELv2/SSPL. It does not name wasm32 as a shipped platform.

## F6 — Viable edit is `[graph].targets` only

```toml
targets = [
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
]
```

String entries, no per-target `features`, `all-features` left false, `no-default-features` left false. Both triples are `not(target_arch = "wasm32")`, so their feature union does not enable `compile-time-rng` or `webpki-root-certs`. Linux keeps `runtime-rng` and `rustls-native-certs`.

Write path: `ci/deny.toml` only. No manifest, lock, vendor, source, policy, spec, or plan edit.

The edit is not executed and not scanned. Expected disappearance of the two crates is a predicate argument. The exit code of the future command is unknown. `num-bigint` 0.4.8 / 0.5.1 stays a warning under current bans policy if both remain on the Linux graph, which they do: 0.5.1 is `arrow-buffer` on every target.

`Cargo.lock` will still list both failing crates. `--target all` and a wasm32 tree must still show them after the edit. That is the check that the lock was not rewritten to hide them.

## F7 — Rejected

- Allow-list, exception, ignore, clarify, or dropping the deny command.
- `exclude-dev`, `all-features = true`, or repeating `default-features = false`.
- Leaving series 60, or patching `arrow-array` / `const-random-macro`. crates.io on 2026-10-03: `arrow-array` max version is 60.0.0.
- Replacing `tiny-keccak`: the macro requires 2.0.2, and that crate line is CC0.
- `rustls-platform-verifier` 0.7.1: published `2026-09-24T20:39:00.557360Z`, crate sha256 `1167586491e2b18b8bfbb293e8180ec17c201c4f076d7cb3070ca964e7598f98`, and its wasm32 dependency is still `webpki-root-certs` 1. The `oo-sdk` pin comment already refuses 0.7.1. Age, yank, and MSRV of a new 0.7.1 closure were not collected because the bump does not fix the edge.
- Removing platform trust, switching the SDK to `OnlyPem` only, or switching reqwest to native-tls.
- A target list that drops either documented Linux triple, or that adds wasm32.

## F8 — Behavior the edit is required to keep

Arrow 60 `RecordBatch`, decimal coefficient behavior, and `oo-core` canonical hashes stay on the current crate versions and features. Connector caller-CA `verify-full` stays. SDK `RootTrust::Platform` stays on `rustls-native-certs` for the two Linux triples. Hostname verification stays in `verify_server_cert`. MSRV stays 1.94.0. No new crate is pinned, so no new 168-hour or yank condition is opened by this proposal.

## F9 — If target policy is refused

No feature or version change inside the current pins removes either crate from an empty-targets deny graph while keeping Arrow 60 and Linux platform trust. The gate stays the a110 exit 4. This document is not a substitute green result.

## F10 — Unsigned on purpose

No runtime test, no new deny run, no audit run, no amd64 build, no claim that `cargo test -p oo-connectors` without `OO_PG_FIXTURE` is a pass. a110 recorded that command as exit 101, missing fixture, and it stays that.
