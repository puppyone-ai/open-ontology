# a119 supported-target license review

Verdict: APPROVE LIMITED documented-target license gate.

Reviewed object: `341ece0ea25ae50ff054a4581e8a3389ccb2dd66`, tree `b9b8d6119ab167aeba4d6feafe031c491ae3f727`, parent `3963fc2b0ece54cabe62071153918b2df815b1c7`. The worktree was not checked out. The archive used by the builder was `git archive` of that commit. Outside `evidence/`, the parent diff is `ci/deny.toml`, 4 insertions, 0 deletions. Identifiers are in `HASHES.txt`.

This accepts the four-line `[graph].targets` list as the license gate for the documented Linux GNU pair. It does not accept the all-target lock, wasm, a release signature, a legal opinion, an advisory-freshness claim past this checkout, or a binary, dual-arch, or gVisor certification. Types and SP00 stay unsigned.

## Probe that actually ran

One linux/arm64 container, image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0` won over `rust-toolchain.toml` 1.98.1. Smoke: `uname` Linux aarch64, `rustc` host `aarch64-unknown-linux-gnu`, release `1.94.0`, cargo `1.94.0`. Private bridge, cap-drop ALL, no-new-privileges, memory and swap 8g, 4 cpus, pids 4096, own cargo and target volumes. No host network, no docker socket, no shared cargo home.

Two earlier launches died in the oracle compile and were kept.

- `linux/attempt1`: smoke identity passed, then `error[E0277]` because `Vec<Extra>` is not `Clone` (`vec![Vec::new(); n]`). Inside exit 1. Container, network, volumes, and archive removed.
- `linux/attempt2`: `error: field 0 is never read` on `Val::Num(String)` under `rustc --deny warnings`. Inside exit 1. Same cleanup.

The third launch, run `deabd858-3636-4039-9416-55f9510ef2d8`, compiled the oracle, self-test exit 0, `cargo install cargo-deny --version 0.20.2 --locked` exit 0, version `cargo-deny 0.20.2`, binary sha256 `44296adb80fd7d3a6ce8737e2806cca409a96757798b399f32f1b535df74884c`, `cargo fetch --locked` exit 0. Expected outcomes were hashed before that container started (`expected/registered.sha256` matches the oracle and controls used for the successful run). Outcomes text was not edited after registration.

## Canonical command

```
cargo deny --config /src/ci/deny.toml --locked check
```

No `--log-level` and no CLI `--target`. Config sha256 `6483e22c78bedfbaa55d1558470a70c79552bac50377a4da802bad0a2df1f3e3`. Exit 0. Stdout: `advisories ok, bans ok, licenses ok, sources ok`. Stderr has 0 `error[` lines and 11 `warning[duplicate]` lines. `multiple-versions = "warn"`, so bans stays ok. `error[rejected]` count 0.

The same config with `--log-level debug` also exited 0. The krates callback logged `gathered 259 crates`. 65 `filtered` lines. `259 + 65` equals the metadata package count (`count_match=true`). The license notes at debug mention `CC0-1.0` once, on `dunce` `CC0-1.0 OR MIT-0 OR Apache-2.0`, which deny accepts because MIT-0 and Apache-2.0 are on the allow list. That string is not a rejection.

Evaluated set, from packages minus those `filtered` lines:

- Present: `core_detect` 1.0.0, `multiversion` 0.9.0, `multiversion-macros` 0.9.0, `rustversion` 1.0.23, `arrow-array` 60.0.0, `rustls-native-certs` 0.8.4, `ahash` 0.8.12, `getrandom` 0.2.17 / 0.3.4 / 0.4.3.
- Absent: `tiny-keccak` 2.0.2, `webpki-root-certs` 1.0.9, `const-random` 0.1.18, `const-random-macro` 0.1.16.

`x86_64-unknown-linux-gnu` alone produced the same 259-crate set. `aarch64-unknown-linux-gnu` alone produced 256. The three missing names are `core_detect` 1.0.0, `multiversion` 0.9.0, and `multiversion-macros` 0.9.0. `rustversion` 1.0.23 stays on aarch64. The feature tree shows it as a build dependency of `encoding_rs` 0.8.41. `cargo tree -e normal` hides build edges, so a normal-tree absence does not mean deny skipped it. The pre-registered P2 line that aarch64 would also drop `rustversion` is wrong; the deny set is the record.

## Controls

Registered before the run. Allow list, bans, advisories, and sources match `ci/deny.toml`. Only the target list changes.

| Control | Exit | Licenses | Evaluated |
| --- | --- | --- | --- |
| Canonical two GNU triples | 0 | ok | 259, both offending crates filtered |
| aarch64 only | 0 | ok | 256, omits the three x86_64 crates |
| x86_64 only | 0 | ok | 259, equal to the canonical set |
| Two GNU triples plus wasm32 | 4 | FAILED, 2 errors | 272, includes both offending crates |
| No targets | 4 | FAILED, 2 errors | 314, includes both offending crates |

Wasm injection names `tiny-keccak` 2.0.2 `license = "CC0-1.0"` and `webpki-root-certs` 1.0.9 `license = "CDLA-Permissive-2.0"`. Advisories and sources stayed ok. Bans stayed ok with the same 11 duplicate warnings. Crates present in the wasm control and absent from the canonical set: `bumpalo` 3.20.3, `const-random` 0.1.18, `const-random-macro` 0.1.16, `crunchy` 0.2.4, `js-sys` 0.3.105, `tiny-keccak` 2.0.2, `wasm-bindgen` 0.2.128 and its macro, macro-support, and shared crates, `wasm-bindgen-futures` 0.4.78, `web-sys` 0.3.105, `webpki-root-certs` 1.0.9.

The unfiltered control adds 42 further crates (Windows, Android, Apple, Redox, Haiku, WASI, EFI, and `r-efi` twice). Bans warnings rise from 11 to 13. This re-measures the all-target shape. It does not replace the a110 files.

An aarch64-only exit 0 does not cover the documented pair. The x86_64-only package set equals the union because this lock has no aarch64-only crate. The config still lists both documented triples, and the aarch64-only control shows the three crates that disappear when `x86_64-unknown-linux-gnu` is removed.

## Why `tiny-keccak` is in the resolve and out of the check

On `--filter-platform aarch64-unknown-linux-gnu` and on `x86_64-unknown-linux-gnu`, cargo metadata still has `tiny-keccak` 2.0.2, `const-random` 0.1.18, and `const-random-macro` 0.1.16 with `in_resolve=yes`. The ahash 0.8.12 resolve feature list is `compile-time-rng`, `const-random`, `default`, `getrandom`, `runtime-rng`, `serde`, `std`. `webpki-root-certs` is absent from those filtered package lists. `core_detect` and `multiversion` are absent from the aarch64 filtered resolve and present on x86_64.

That feature list is cargo's unified resolve. It is not the activated set. From the same metadata:

- `arrow-array` 60.0.0 depends on `ahash` with `cfg(not(target_arch = "wasm32"))` and features `runtime-rng`, and again with `cfg(target_arch = "wasm32")` and features `compile-time-rng`.
- ahash features: `default = ["std", "runtime-rng"]`, `runtime-rng = ["getrandom"]`, `getrandom = ["dep:getrandom"]`, `compile-time-rng = ["const-random"]`, `const-random = ["dep:const-random"]`. `const-random` is optional, target null.
- `rustls-platform-verifier` 0.7.0 depends on `rustls-native-certs` under `cfg(all(unix, not(target_os = "android"), not(target_vendor = "apple"), not(target_arch = "wasm32")))`, and on `webpki-root-certs` under `cfg(target_arch = "wasm32")`. A second `webpki-root-certs` edge is `kind=dev`, target null. cargo-deny 0.20.2 ignores non-workspace dev edges when `exclude-dev` is false.

With both Linux triples, deny takes one metadata snapshot and does not pass `--filter-platform` (`single_target` fires only for exactly one target and no CLI target). krates then keeps a cfg edge when either triple matches. The Linux edges enable `runtime-rng` and `rustls-native-certs`. They do not enable `compile-time-rng`, so optional `const-random` and `tiny-keccak` drop. The wasm edge to `webpki-root-certs` drops. Debug logs those four names as `filtered`.

`cargo tree --target aarch64-unknown-linux-gnu` and `--target x86_64-unknown-linux-gnu`: `compile-time-rng`, `tiny-keccak`, `const-random`, and `webpki-root-certs` counts are 0; `runtime-rng` and `rustls-native-certs` counts are non-zero; inverse trees for the two offending crates print `nothing to print` and empty stdout. `--target wasm32-unknown-unknown` and `--target all` show `tiny-keccak`, `const-random`, `compile-time-rng`, and `webpki-root-certs`, and the wasm tree drops `rustls-native-certs`. Arrow 60 stays on every one of those trees.

Lock rows are unchanged: `tiny-keccak` 2.0.2 checksum `2c9d3793400a45f954c52e73d068316d76b6f4e36977e3fcebb13a2721e80237`, `webpki-root-certs` 1.0.9 checksum `b96554aa2acc8ccdb7e1c9a58a7a68dd5d13bccc69cd124cb09406db612a1c9b`. Allow list is Apache-2.0, MIT, ISC, BSD-3-Clause, Unicode-3.0, Zlib, Unlicense, MIT-0. `ignore = []`. No CC0 or CDLA exception.

## Oracle

The safe Rust oracle (`probe/graph_oracle.rs`, `#![forbid(unsafe_code)]`) walked the same metadata. Self-test passed. On every comparison with a deny debug log, `unmatched_edges=0` and the marker crates agree with deny: the Linux union keeps the x86_64 three plus `rustversion`, Arrow, and native certs, and drops both offending crates; wasm and the unfiltered walk keep both offending crates.

Each full-set diff is `oracle_only=0`, `deny_only=1`, the extra crate being `mio` 1.2.3. tokio 1.53.1 enables optional `mio` through the strong feature item `mio/os-poll` on feature `net` (`uses_default_features=false` on that dep). The oracle recorded the extra feature and did not mark the optional dep active. krates does, and the deny graph includes `mio`. The license check therefore covered `mio`. The oracle is an under-approximation of one crate, not a second license result.

## Retained parent evidence

a110 `deny.exit` is the byte `4` plus newline, sha256 `7de1555df0c2700329e815b93b32c571c3ea54dc967b89e81ab73b9972b72d1d`. `deny.out` is `advisories ok, bans ok, licenses FAILED, sources ok`. That RED stands. This review did not edit it.

a117 before and after content hashes match for metadata, feature trees, and normal trees on aarch64, x86_64, wasm32, and all-target. The hash field inside each sidecar equals the file bytes. `linux/graph-compare.txt` says DIFFER because the sidecar text embeds `/evidence/raw/before/...` versus `/evidence/raw/after/...`. `linux/GRAPH_MISMATCH` contains the word `mismatch`. Those files are a path-label mismatch. The graph bytes are the same, which is the limited reason this review did not rerun full-workspace, TLS, Arrow gold, or dual-arch product builds.

a117 `facts.broken-id-match.txt` stores the raw package id in the name column (`resolve_nodepath+file:///...` and `registry+...#name@version`) and a blank version, so name/version equality misses every interesting package. The corrected `facts.txt` uses the id and reports `tiny-keccak` `in_resolve=yes` on the Linux filter. This review parsed the new metadata with exact id equality and got the same resolve membership. The broken file is not membership evidence.

## Docs coverage

At the reviewed commit, the documented host pair is Linux x86_64 and aarch64: `docs/11-deployment-and-operations.md` line 27, `docs/17-rust-implementation.md` line 97, spec `REQ-CORE-06` line 180, `REQ-OPS-02` line 715, P14 line 865, `AT-OPS-01` line 1181. `supply-chain/harness-dependency-delta.md` line 29 says wasm is age-checked and is not a supported target matrix. The target list is that pair and no wasm triple.

## Toolchain, capacity, cleanup

Launch saw host rust comm count 0 and running rust containers 0. `eef831aa-dba4-4af2-a78b-541bc9e0a116-pg` was up and was not stopped. After the run, the a119 container, network `oo-a119-deabd858-3636-4039-9416-55f9510ef2d8-net`, both volumes, and the archive directory were removed. No `oo-a119` volume or network remains. Host config recorded `Privileged=false`, `CapDrop=["ALL"]`, `no-new-privileges`, the private network, 8589934592 memory and swap, 4000000000 nanocpus, pids 4096.

Advisory database cloned during the run: rev `f8dee89e1b2f2f1eaf548312df7655fe5202a302`, path `advisory-db-3157b0e258782691`, commit time `2026-10-02 22:27:46 +0200`. Advisories ok is against that checkout only.

Residual: the oracle misses `mio` 1.2.3; `rustversion` is shared, not x86_64-only; duplicate-version warnings remain at warn; wasm and all-target stay failing and unsigned; a116 containers were left as they were.
