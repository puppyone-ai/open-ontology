# a119 findings

Reviewed `341ece0ea25ae50ff054a4581e8a3389ccb2dd66` tree `b9b8d6119ab167aeba4d6feafe031c491ae3f727` parent `3963fc2b0ece54cabe62071153918b2df815b1c7`.

## F1. The policy diff is the target list

Parent to reviewed commit, outside `evidence/`, is `ci/deny.toml` +4/−0. The new lines are:

```
targets = [
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
]
```

`all-features = false` and `no-default-features = false` stay. Allow list, `confidence-threshold = 0.8`, `ignore = []`, bans, and the crates.io source rule are unchanged. No license exception, clarify, or `exclude-dev`. Lock blob `15ae1071c7236a956547e3f1d37d627b2cbebfce` is identical at the parent. oo-sdk blob `d04a641f013f55e0f306a29e60b42d091c896e87` and oo-connectors blob `e8e3254da87a29ce90c485e8ad6566dbd4654f43` are identical at the parent. CC0 and CDLA rows remain in the lock.

## F2. The canonical gate checked both arches' active crates and passed

Exact command `cargo deny --config /src/ci/deny.toml --locked check` exited 0. Stdout: `advisories ok, bans ok, licenses ok, sources ok`. Stderr has no `error[`. Eleven duplicate-version warnings match `multiple-versions = "warn"`.

Debug `gathered 259 crates`, 65 filtered, count match true. The kept set includes `core_detect` 1.0.0, `multiversion` 0.9.0, `multiversion-macros` 0.9.0, `rustversion` 1.0.23, `arrow-array` 60.0.0, and `rustls-native-certs` 0.8.4. It excludes `tiny-keccak` 2.0.2, `webpki-root-certs` 1.0.9, `const-random` 0.1.18, and `const-random-macro` 0.1.16. Tool binary sha256 `44296adb80fd7d3a6ce8737e2806cca409a96757798b399f32f1b535df74884c`. Advisory db `f8dee89e1b2f2f1eaf548312df7655fe5202a302`.

## F3. Three crates are x86_64-only; `rustversion` is not

Canonical minus aarch64-only deny set:

- `core_detect` 1.0.0
- `multiversion` 0.9.0
- `multiversion-macros` 0.9.0

x86_64-only minus canonical is empty, and canonical minus x86_64-only is empty. `rustversion` 1.0.23 is in the aarch64 deny set. The aarch64 feature tree shows it under `encoding_rs` build-dependencies. A normal-edge tree that omits it is the wrong absence test. P2 expected aarch64 to drop `rustversion`. The deny log does not.

## F4. Missing a triple, adding wasm, and clearing targets are visible

aarch64-only exit 0 with licenses ok omits the three x86_64 crates. That pass is not dual-arch coverage.

Adding `wasm32-unknown-unknown` exited 4. `licenses FAILED: 2 errors`. The errors are `tiny-keccak` `CC0-1.0` and `webpki-root-certs` `CDLA-Permissive-2.0`. Both crates are in the 272-crate evaluated set, with the wasm-bindgen family, `js-sys`, `web-sys`, `bumpalo`, `crunchy`, and `const-random`.

Clearing targets exited 4 with the same two license errors on a 314-crate set, 42 crates beyond the wasm control, and 13 duplicate warnings. a110 remains the bound all-target RED: exit file contents `4`, summary `licenses FAILED`.

## F5. Resolve membership and the checked graph are different facts

Linux `--filter-platform` metadata: `tiny-keccak` 2.0.2 `in_resolve=yes`, ahash resolve features include `compile-time-rng` and `const-random`. The activated Linux edges do not. `arrow-array` 60 selects `runtime-rng` on `cfg(not(target_arch = "wasm32"))` and `compile-time-rng` on wasm32. `const-random` is optional and target-null, enabled only by `compile-time-rng`. `webpki-root-certs` normal edge is wasm32; its dev edge is target-null and is ignored for a non-workspace crate. Linux trees keep Arrow 60, `runtime-rng`, and `rustls-native-certs`. Wasm and `--target all` show the CC0 and CDLA crates.

`dunce` 1.0.5 is `CC0-1.0 OR MIT-0 OR Apache-2.0`. Deny accepts it on the canonical run because MIT-0 and Apache-2.0 are allowed. The passing debug log contains that one `CC0-1.0` string. Rejection count is 0.

## F6. Oracle marker agreement, one missed crate

The independent oracle agrees with deny on the marker crates for the canonical union, aarch64-only, x86_64-only, wasm injection, and the unfiltered graph. `unmatched_edges=0`. Every diff is deny-only `mio` 1.2.3. tokio feature `net` lists `mio/os-poll` with `uses_default_features=false`. The oracle did not treat that strong item as activating optional `mio`. Deny kept `mio`, so the license section included it. The oracle is not the license oracle of record.

## F7. a117 `GRAPH_MISMATCH` is a sidecar path label

Recomputed before and after sha256 of `metadata.json`, `tree-features.stdout`, and `tree-normal.stdout` match on all four filters, and they match the hash field inside the sidecars. The sidecar body also contains `/evidence/raw/before` or `/evidence/raw/after`, so a whole-file compare prints DIFFER. `linux/GRAPH_MISMATCH` is the single word `mismatch`. Graph bytes were unchanged across the config edit.

## F8. The first meta_facts id matcher is not membership

`facts.broken-id-match.txt` writes the raw id into the name field and leaves version empty. Corrected `facts.txt` and this review's parser, using exact id equality, both show `tiny-keccak` in the Linux filtered resolve. The broken scan's `in_resolve=no` lines are discarded.

## F9. Compile failures kept

Attempt 1: `E0277` `Vec<Extra>: Clone`, inside exit 1, after a good 1.94 aarch64 smoke. Attempt 2: dead_code on unread JSON number, inside exit 1. The successful oracle is the third source, hashed in `expected/registered.sha256`.

## F10. What this approval does not say

No all-lock license approval, no legal opinion, no release signature, no product binary, no dual-arch image, no gVisor run, no new cargo-audit, no claim that the advisory db is newer than the checkout `f8dee89e` of `2026-10-02 22:27:46 +0200`. Wasm and the empty-target graph stay unsigned. Types and SP00 are not accepted by this exit 0.
