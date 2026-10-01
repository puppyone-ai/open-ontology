# oo-core dependency delta (SP00-W02 / SP00-W03)

This file records the crates `oo-core` added on top of the SP00-W01 baseline, including the 2026-10-01 frozen CORE corrections. Structured fields are in [core-dependency-delta.json](core-dependency-delta.json). [dependency-baseline.json](dependency-baseline.json) and [dependency-baseline.md](dependency-baseline.md) are unchanged. License copies for the crates cleared in this pass are under [licenses/core](licenses/core).

Adoption clock: 2026-09-30T23:48:21Z. Cutoff: 2026-09-23T23:48:21Z. Index publication time is the crates.io `pubtime` field. No security-fix exception was used. AGPL, BSL, ELv2, and SSPL are absent. `cargo audit` and `cargo deny` are not installed and were not run. They are not marked as run.

## Toolchain

Primary checks use Homebrew rustc 1.98.1, commit `48a229ceaefd4985c50990b14116b6d856af0985`, host `aarch64-apple-darwin`. rustup is not installed. The workspace `rust-version` stays 1.94.0. The highest declared `rust-version` among crates newly compiled into `oo-core` is serde-saphyr 1.3.0 at 1.89. chrono 0.4.45 declares 1.62.0 and chrono-tz 0.10.4 declares 1.65. The lock-wide highest remains the baseline cargo-platform 1.91. This wave did not repeat the W01 rustc 1.94.1 compile. Linux targets were not built.

## What changed in the lock

`oo-core` has fifteen direct registry dependencies. `Cargo.lock` has 119 registry packages. `cargo metadata --format-version 1 --filter-platform aarch64-apple-darwin --locked --offline` on 2026-10-01 lists 119 packages, 103 of them from the registry.

| Placement | Count | Meaning |
|---|---|---|
| host oo-core | 99 | Present in the platform-filtered metadata and reachable from `oo-core`. |
| host arch-check only | 4 | `cargo_metadata` 0.23.1, `camino` 1.2.6, `cargo-platform` 0.3.3, `semver` 1.0.28. Unchanged W01 edges. |
| other-target | 12 | In `Cargo.lock`, absent from the Apple Silicon resolve. |
| inactive optional | 4 | Locked, feature off, absent from the Apple Silicon resolve. |

The five crates added for IANA time are chrono 0.4.45, chrono-tz 0.10.4, phf 0.12.1, phf_shared 0.12.1, and siphasher 1.0.3. Checksums in the JSON match the sparse index for every locked package, including these five.

## Direct pins

| Crate | Version | Published | MSRV | License relied on | Features |
|---|---|---|---|---|---|
| serde | 1.0.229 | 2026-07-18 | 1.56 | Apache-2.0 | `derive` (baseline name and version) |
| serde_json | 1.0.151 | 2026-07-20 | 1.71 | Apache-2.0 | `arbitrary_precision` on the oo-core edge. `float_roundtrip` still comes from jsonschema. |
| schemars | 1.2.2 | 2026-07-27 | 1.74 | MIT | `derive` |
| serde-saphyr | 1.3.0 | 2026-09-16 | 1.89 | Apache-2.0 | `serialize`, `deserialize`. `include` and `include_fs` off. |
| jsonschema | 0.57.0 | 2026-09-21 | 1.85.0 | MIT | default features off |
| utoipa | 6.0.0 | 2026-09-22 | 1.88 | Apache-2.0 | `macros`, default features off |
| utoipa-gen | 6.0.0 | 2026-09-22 | 1.88 | Apache-2.0 | matched pin |
| sha2 | 0.10.9 | 2025-04-30 | none declared | Apache-2.0 | `std`, default features off. `sha2-asm` off. |
| thiserror | 2.0.21 | 2026-09-23 | 1.77 | Apache-2.0 | baseline version |
| chrono | 0.4.45 | 2026-06-04 | 1.62.0 | Apache-2.0 | `std` only. `clock`, `wasmbind`, `oldtime` off. |
| chrono-tz | 0.10.4 | 2025-07-11 | 1.65 | Apache-2.0 plus tzdb public-domain notice | `std` only. Prebuilt tzdb `2025b`. |
| encoding_rs | 0.8.41 | 2026-09-09 | 1.88 | Apache-2.0 AND BSD-3-Clause | version pin. `oo-core` source does not call it. Not a decoder. |
| siphasher | 1.0.3 | 2026-05-03 | none declared | Apache-2.0 | pin. Pulled by phf_shared. oo-core does not call it. |
| smallvec | 1.16.1 | 2026-09-11 | none declared | Apache-2.0 | pin, also used by serde-saphyr and parking_lot_core |
| zerocopy | 0.8.57 | 2026-09-08 | 1.56.0 | Apache-2.0 | `simd`, default features off, matching ahash |

serde, serde_json, and thiserror keep the baseline versions. Where a crate is MIT OR Apache-2.0, this project relies on Apache-2.0. jsonschema 0.57.0, schemars 1.2.2, fancy-regex 0.19.2, phf 0.12.1, and phf_shared 0.12.1 are MIT only. encoding_rs is `(Apache-2.0 OR MIT) AND BSD-3-Clause`. The BSD-3-Clause obligation is the WHATWG LICENSE file. foldhash 0.2.0 is Zlib. unicode-ident stays Apache-2.0 AND Unicode-3.0 from the baseline.

jsonschema's default features (`resolve-http`, `resolve-file`, `tls-aws-lc-rs`, `idna`) are off. The host resolve of jsonschema has an empty feature list. `reqwest`, `idna`, `tokio`, `hyper`, and `rustls` are not in `Cargo.lock`.

`serde_json` `arbitrary_precision` is on because the oo-core dependency requests it. The feature applies to every user of this pin in the workspace. JSON numbers used by FlexDecimal stay lexical strings. `float_roundtrip` is still on because jsonschema 0.57.0 depends on serde_json with that feature. `preserve_order` and `raw_value` stay off. Exact amounts in the models remain decimal strings. YAML numbers are read from granit-parser events, not from an f64 `serde_json::Value`.

`chrono` host features are `alloc` and `std`. `src/offset/local` is compiled only with `clock`, which is off, so this resolve does not read the host timezone and does not pull `iana-time-zone`. `chrono-tz` host feature is `std`. `phf` and `phf_shared` have an empty host feature list (`macros` off). `siphasher` host features are `default` and `std`.

ahash 0.8.12 resolves with `std` and `runtime-rng`, so getrandom 0.3.4 is on the host. `cargo tree` shows getrandom depending on cfg-if and libc. `cargo metadata` also lists the getrandom feature `wasm_js`. The platform-filtered edges do not include wasm-bindgen or js-sys.

uuid-simd 0.8.0 is a normal dependency of jsonschema 0.57.0 (`default-features = false`, features `std` and `detect`). It is in the host graph, with vsimd 0.8.0.

The `url` crate and `arrow-schema` are not dependencies. Series-60 strings are formatted by `oo-core`. That is not an arrow-schema 60 `Display` comparison.

## Versions refused for age

| Refused | Admitted |
|---|---|
| encoding_rs 0.8.42 | 0.8.41 |
| jsonschema 0.58.0 through 0.58.3 | 0.57.0, default features off |
| smallvec 1.16.2 (2026-09-25) | 1.16.1 |
| zerocopy 0.8.59 | 0.8.57, and zerocopy-derive stays 0.8.57 with the derive feature off |
| siphasher 1.0.4 (2026-09-25T08:51:05Z, checksum `33f4fe9184a62d842c9ef383018f3306d8ba224fd9d836f56d7288308847c256`) | 1.0.3 |

js-sys 0.3.106 (2026-09-25T07:13:42Z) and wasm-bindgen 0.2.129 (2026-09-25T07:13:40Z), plus wasm-bindgen-macro, wasm-bindgen-macro-support, and wasm-bindgen-shared from the same minute, are in the lock because getrandom lists them for wasm32. They are not in the Apple Silicon resolve. A wasm32 build of this lock is blocked. getrandom was not downgraded to delete them. Those young wasm crates stay unsupported.

## Other-target and inactive packages

Other-target, old enough, not compiled here: bitflags 2.13.2 and redox_syscall 0.5.18 (Redox), windows-link 0.2.1 (Windows), r-efi 5.3.0 (UEFI), wasip2 1.0.4 and wit-bindgen 0.57.1 (wasm32-wasip2), bumpalo 3.20.3 (under wasm-bindgen-macro-support).

Inactive optional, old enough, feature off: core_detect 1.0.0, multiversion 0.9.0, multiversion-macros 0.9.0 (encoding_rs `simd-accel` is off), zerocopy-derive 0.8.57 (zerocopy `derive` is off).

## License files

Copies and the retrieval record are in [licenses/core](licenses/core). Summary:

| Crate | Relied on | Where the text is |
|---|---|---|
| chrono 0.4.45 | Apache-2.0 | `LICENSE.txt` in the crate and at commit `170338250e836976a211e64728ec956e45e78a39`. |
| chrono-tz 0.10.4 | Apache-2.0, plus public-domain tzdb notice | Crate path `LICENSE` is a symlink to the repo-root `LICENSE` at commit `e15d1f308a1ac2fdf3f1bf19c325dc202d418081`. `tz/LICENSE` matches eggert/tz commit `7e1145bfdb9630c127841dc8ce808a937a300938`. |
| phf 0.12.1, phf_shared 0.12.1 | MIT | Crate `LICENSE` matches the repo-root file at commit `03a930696231da01005e762425841f91587b3e04`. The crate path is a symlink. |
| siphasher 1.0.3 | Apache-2.0 | `COPYING` is in the crate at commit `451f67d73a772cba325728109bbfa247750ed076`. `LICENSE-APACHE` and `LICENSE-MIT` are HTTP 404 there. The Apache-2.0 body was fetched from the URL `COPYING` names and is not a blob in that commit. |
| jsonschema-regex 0.57.0, jsonschema-value 0.57.0 | MIT | Tarballs omit `LICENSE`. Root `LICENSE` and `workspace.package.license = "MIT"` at commit `8a7870f1400e9dcddfe8cfb57e550fbe4013f79a` cover `crates/jsonschema-regex` and `crates/jsonschema-value`. |
| uuid-simd 0.8.0, vsimd 0.8.0 | MIT | Tarballs omit `LICENSE`. Each README points at `../../LICENSE`. Root `LICENSE` at commit `d74c030d9dc4f3cae02146d1f497ff62726ef09a` covers both paths. |
| r-efi 5.3.0 | Apache-2.0 | Other-target. `AUTHORS` is in the crate at commit `97b55bed1c2c91dcbf787674849f05337ff80b33`. There is no `LICENSE` file. The Apache-2.0 body was fetched from the URL `AUTHORS` names and is not a blob in that commit. LGPL-2.1-or-later is not selected; its full text is not in the commit and is not treated as cleared. |

encoding_rs keeps the license files already recorded for 0.8.41. The pin does not admit a new decoder.

## Build scripts and unsafe scope

Host build scripts that run for this resolve are cfg and rustc version probes (`println!`, autocfg, version_check, rustc_version), plus chrono-tz's `build.rs`. That script does not call `chrono-tz-build` unless `case-insensitive` or `filter-by-regex` is on. Neither feature is on. No host C compilation was observed.

wasm-bindgen-shared 0.2.129 `build.rs` runs `git rev-parse` and prints a rustc env var. The substring `bindgen` is why an audit heuristic called it native. It does not compile C, and it is not in the host graph.

wit-bindgen 0.57.1 returns immediately when the target family is not wasm. On wasm32 it copies the shipped static archive `src/rt/libwit_bindgen_cabi.a` and emits `cargo:rustc-link-lib=static`. That path was not executed.

chrono `unsafe` under `src/offset/local` is behind `feature = "clock"` and is not compiled here. The always-compiled chrono sources still contain `unsafe` `Send` impls and `NonZeroI32::new_unchecked`. chrono-tz, phf, and phf_shared have no `unsafe` in `src/`. siphasher 1.0.3 uses `unsafe` byte loads in `src/sip.rs`, `src/sip128.rs`, and `src/common.rs`. No proc-macro is enabled on phf. serde, thiserror, utoipa, and schemars still bring their existing proc-macros; this pass did not add another one.

## Checks

The `commands` array in the JSON is the pre-correction run. It does not cover this tree. The corrected-tree commands on 2026-10-01, Homebrew rustc 1.98.1, `aarch64-apple-darwin`, all exited 0:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`
- `cargo test --workspace --locked --offline` (oo-arch-check 43, oo-core lib 8, canonical_golden 8, fixtures_pg_mysql 5, frozen_corrections 19, schema_contract 7)
- `cargo run --locked --offline -p oo-arch-check` (`architecture check: pass`)
- `cargo run --locked --offline -p oo-arch-check -- --demonstrate-negative` (mutated `oo-gateway -> oo-pipeline` failed with 4 violations; the baseline was restored)

The architecture commands ran before the last test assertion (America/Chicago 2026-10-01 09:12:03). That assertion does not change library code or dependencies. `cargo audit` and `cargo deny` were not run. Logs are under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a016/SP00/SP00-W02/commands` and the same files under `SP00-W03/commands`.
