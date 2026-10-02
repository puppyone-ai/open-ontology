# Dependency license repair proposal

Session `eef831aa-dba4-4af2-a78b-541bc9e0a115`. Read-only analysis of frozen commit `99b326b18add833896128ca344d10452b0c34c30`. No Rust builder, no `cargo` / `rustc` / `cargo-deny` / `cargo-audit` execution, no container, no PostgreSQL, no root-writer edit.

This is a proposal. It does not clear the supply gate, and it does not claim that a later scan exits 0.

## Identity

| Object | Value |
|---|---|
| Frozen commit | `99b326b18add833896128ca344d10452b0c34c30` |
| Tree | `f397a2acada92905449102873e70908290e2383c` |
| Parent | `e0726071950c54383b2fe6c1831e419045b1ebe6` |
| Subject | `plan: specify independent snapshot owner acceptance` |
| `Cargo.lock` git blob | `15ae1071c7236a956547e3f1d37d627b2cbebfce` |
| `Cargo.lock` sha256 | `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f` |
| `ci/deny.toml` git blob | `367598cb3768b8c8af698041cd0f84ce6f8fb7be` |
| `ci/deny.toml` sha256 | `4320807bfe747fe339ce7bf539a1cff5604e8e0c7677760e2cbc7abc1868624b` |

The lock and deny blobs are the same blobs a110 reviewed on `3ca5357930a2c44c42d3061db5fabe577881269a`. Commits between that review and the frozen commit do not touch `Cargo.lock`, `ci/deny.toml`, `crates/oo-sdk/Cargo.toml`, or `crates/oo-connectors/Cargo.toml`. `HASHES.sha256` records the worktree HEAD observed while writing this file and whether those four paths still matched the frozen blobs.

## Current failing graph

a110 ran `cargo deny --config ci/deny.toml --locked check` on that lock with cargo-deny 0.20.2. Exit 4. Summary line: `advisories ok, bans ok, licenses FAILED, sources ok`. Diagnostic sha256 `6368b7928790e9465feac64746391b951f9d2cdccb04b7347ccaaa3f6ac87d25` (`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a110/SP03/lossless-types-review/raw/phases/deny.err`).

`ci/deny.toml` allow list is Apache-2.0, MIT, ISC, BSD-3-Clause, Unicode-3.0, Zlib, Unlicense, MIT-0. `[graph]` sets `all-features = false` and `no-default-features = false`. It sets no `targets`. `[licenses]` has no `exceptions`, `ignore`, or `clarify`.

Two rejections, both still failures under that config:

1. `tiny-keccak` 2.0.2, manifest `license = "CC0-1.0"`, lock checksum `2c9d3793400a45f954c52e73d068316d76b6f4e36977e3fcebb13a2721e80237`.
2. `webpki-root-certs` 1.0.9, manifest `license = "CDLA-Permissive-2.0"`, lock checksum `b96554aa2acc8ccdb7e1c9a58a7a68dd5d13bccc69cd124cb09406db612a1c9b`.

`bans ok` still prints `warning[duplicate]` for `num-bigint` 0.4.8 and 0.5.1. `multiple-versions = "warn"`, so the warning is not the exit. 0.5.1 is `arrow-buffer` 60.0.0. 0.4.8 is the pre-existing `fraction` / `num` / `num-rational` edge. This proposal does not touch that warning.

a110 `cargo audit --json` on the same lock exited 0 with zero vulnerabilities. That result is retained history. This session did not repeat it.

## Edge 1 — `tiny-keccak` CC0, wasm32 only

Retained feature tree `tree-tiny-all.out` (sha256 `2d74098e80af0351e1ea1fc57d5cf5c6557b594012b2cfb8e5ad3aab9f04dc7b`):

```
tiny-keccak v2.0.2
└── feature "default"
    └── const-random-macro v0.1.16
        └── feature "default"
            └── const-random v0.1.18
                └── feature "default"
                    └── ahash v0.8.12
                        └── feature "compile-time-rng"
                            └── arrow-array v60.0.0
                                └── oo-connectors v0.1.0
                                    └── feature "default" (command-line)
```

The same tree shows `ahash` feature `const-random` as an alias edge of `compile-time-rng`, and `tiny-keccak` feature `shake` on the same `const-random-macro` edge. `ahash` feature `runtime-rng` is a separate edge from the same `arrow-array`, and `ahash` features `default`, `std`, and `serde` come from `jsonschema` 0.57.0, `jsonschema-value` 0.57.0, and `referencing` 0.57.0. Those crates do not enable `compile-time-rng`.

Authoritative manifests:

- `arrow-array` 60.0.0 `Cargo.toml.orig` (retained body, sha256 `798ee0be33ba28de4c891970d7c7bcad76234dc0115588bab1fceffe4c9b7a86`), dependencies, not features:

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
ahash = { version = "0.8", default-features = false, features = ["compile-time-rng"] }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
ahash = { version = "0.8", default-features = false, features = ["runtime-rng"] }
```

`ahash` is not optional. `oo-connectors` already sets `default-features = false` on `arrow-array`, `arrow-buffer`, and `arrow-schema` at `=60.0.0`. Arrow's own features are `async`, `ffi`, `force_validate`, and `pool`. None of them gate `ahash`.

- `ahash` 0.8.12, downloaded 2026-10-03 from `https://static.crates.io/crates/ahash/ahash-0.8.12.crate`, file sha256 `5a15f179cd60c4584b8a8c596927aadc462e27f2ca70c04e0071964a73ba7a75` (matches the lock):

```toml
compile-time-rng = ["const-random"]
default = ["std", "runtime-rng"]
runtime-rng = ["getrandom"]
```

`const-random` is `optional = true`.

- `const-random` 0.1.18 depends on `const-random-macro` 0.1.16 with that crate's default features. License of both is `MIT OR Apache-2.0`.
- `const-random-macro` 0.1.16 depends on `tiny-keccak` `version = "2.0.2"` with `features = ["shake"]` and does not set `default-features = false`. `tiny-keccak` `default = []` and `shake = []`, so the enabled set is `default` plus `shake`. License body in the retained crate is the CC0 1.0 waiver. crates.io API, retrieved 2026-10-03: published `2020-04-01T14:34:13.119797Z`, `yanked` false, no `rust_version`, license field `CC0-1.0`.

`oo-connectors` has no `[features]` table. The tree label `feature "default" (command-line)` is Cargo's implicit default, not a feature this crate can turn off.

Host fact already recorded by a110: on `aarch64-unknown-linux-gnu`, `cargo tree -e normal` and `cargo tree -e all` do not contain `tiny-keccak`. `cargo tree --target all` does. That matches the `not(target_arch = "wasm32")` predicate selecting `runtime-rng`.

## Edge 2 — `webpki-root-certs` CDLA, wasm32 only

Deny parents:

```
webpki-root-certs v1.0.9
└── rustls-platform-verifier v0.7.0
    ├── oo-sdk v0.1.0
    └── reqwest v0.13.5
        └── oo-sdk v0.1.0
```

`oo-sdk` selects:

```toml
reqwest = { version = "=0.13.5", default-features = false, features = ["rustls", "http2"] }
rustls-platform-verifier = { version = "=0.7.0", default-features = false }
```

`reqwest` 0.13.5, downloaded 2026-10-03 from `https://static.crates.io/crates/reqwest/reqwest-0.13.5.crate`, file sha256 `16a1cfa75cc186dd73d5818e510e042e40927bccc9c236b061cea97e1eb08029` (matches the lock):

```toml
rustls = ["__rustls-aws-lc-rs", "dep:rustls-platform-verifier", "__rustls"]
```

The `rustls-platform-verifier` dependency inside reqwest is optional and target-gated:

```toml
[target.'cfg(not(all(target_arch = "wasm32", any(target_os = "unknown", target_os = "none"))))'.dependencies.rustls-platform-verifier]
version = ">=0.6.0, <0.8.0"
optional = true
```

reqwest enables the dependency and enables no feature of `rustls-platform-verifier`. On `wasm32-unknown-unknown` that reqwest edge is off. The direct `oo-sdk` dependency has no target predicate, so it remains on every target, including wasm32.

`rustls-platform-verifier` 0.7.0, downloaded 2026-10-03 from `https://static.crates.io/crates/rustls-platform-verifier/rustls-platform-verifier-0.7.0.crate`, file sha256 `26d1e2536ce4f35f4846aa13bff16bd0ff40157cdb14cc056c7b14ba41233ba0` (matches the lock). `Cargo.toml.orig`:

```toml
[target.'cfg(all(unix, not(target_os = "android"), not(target_vendor = "apple"), not(target_arch = "wasm32")))'.dependencies]
rustls-native-certs = "0.8"
webpki = { package = "rustls-webpki", version = "0.103", default-features = false }

[target.'cfg(target_arch = "wasm32")'.dependencies]
webpki = { package = "rustls-webpki", version = "0.103", default-features = false }
webpki-root-certs = "1"
```

`webpki-root-certs` is not behind a feature. The crate's features are `cert-logging`, `dbg`, `docsrs`, and `ffi-testing`. There is no default feature that selects the CDLA crate. `default-features = false` on the `oo-sdk` line does not remove a target dependency. The same file also lists `webpki-root-certs = "1"` under `[dev-dependencies]`. cargo-deny 0.20.2 ignores dev edges for non-workspace crates (`DepKind::Dev`, `Scope::NonWorkspace` unless `exclude-dev` is set). The scanned parent is the normal wasm32 dependency, which is the edge the diagnostic prints.

`src/verification/others.rs` in that crate loads roots as follows. For `unix`, not Android, not Apple, not wasm32, it calls `rustls_native_certs::load_native_certs()`. For `target_arch = "wasm32"`, it inserts `webpki_root_certs::TLS_SERVER_ROOT_CERTS`. `verify_server_cert` still receives `server_name`. Linux platform trust is the native-cert branch.

a084 already recorded the split on this lock: the Linux arm64 edges tree contains `rustls-native-certs` and `openssl-probe` and does not list `webpki-root-certs`; `cargo tree --target all` lists `webpki-root-certs` 1.0.9. `RootTrust::Platform` is the production default (`crates/oo-sdk/src/config.rs`). `RootTrust::OnlyPem` replaces anchors and keeps hostname verification. `build_http` calls `tls_certs_only` only for `OnlyPem` (`crates/oo-sdk/src/client.rs`).

`webpki-root-certs` 1.0.9, downloaded 2026-10-03 from `https://static.crates.io/crates/webpki-root-certs/webpki-root-certs-1.0.9.crate`, file sha256 matches the lock. crates.io API the same day: created `2026-07-18T09:30:54.919781Z`, `yanked` false, `rust_version` `1.70`, license field `CDLA-Permissive-2.0`. `rustls-platform-verifier` 0.7.0 declares `rust-version = "1.85"` and `MIT OR Apache-2.0`.

PostgreSQL source trust is a different graph. `docs/17-rust-implementation.md` line 91 requires the source connector to trust the caller CA and to use `sslmode=verify-full`. `crates/oo-connectors/Cargo.toml` enables sqlx-core `_tls-rustls-ring-no-roots` and forbids `tls-rustls-ring-webpki`, `tls-rustls-ring-native-roots`, and `tls-native-tls`. That connector does not depend on `rustls-platform-verifier` or `webpki-root-certs`.

## Why one deny command sees both

cargo-deny 0.20.2, crate `https://static.crates.io/crates/cargo-deny/cargo-deny-0.20.2.crate`, file sha256 `e528dfcbe739af7ce37a77d3d6df1b29dd6887b1c701d888820c0f16b864f737`, `src/cargo-deny/common.rs`:

- `gather_krates` calls `include_targets` only when the CLI `--target` list or `[graph].targets` is non-empty.
- An empty list leaves every target-specific dependency in the krates graph. Features enabled by different `cfg` blocks are unified. That is why `compile-time-rng` and `runtime-rng` are both active, and why `webpki-root-certs` sits beside `rustls-native-certs`.
- A string in `targets` deserializes to a triple plus an empty extra-feature vector (`src/root_cfg.rs`). Empty extra features do not add a feature and do not clear features the manifests already selected.
- `single_target` passes `--filter-platform` only when exactly one target is configured. Two configured triples still produce one metadata snapshot, then `include_targets` keeps the union of those triples.

`[graph].all-features = false` does not stop that union. Feature unification is per resolved graph, not per crate feature the workspace named. Setting `default-features = false` on `oo-sdk` or `oo-connectors` cannot cancel a feature or a cfg dependency that another edge in the same graph turns on. Both direct lines already disable default features.

## Required targets

The Rust targets this repository requires for the product are:

| Triple | Why it is required |
|---|---|
| `aarch64-unknown-linux-gnu` | `docs/17-rust-implementation.md` §7, line 97: verify on Linux x86_64 and aarch64, and on gVisor. `docs/11-deployment-and-operations.md` §1.1, line 27: the formal host is Linux x86_64/aarch64, accepted separately. Spec `REQ-CORE-06` (line 180), `REQ-OPS-02` (line 715), P14 (line 865), `AT-OPS-01` (line 1181), release line 1339. The builders already used for this lock are `aarch64-unknown-linux-gnu`. gVisor is a runtime, not a separate Rust triple. |
| `x86_64-unknown-linux-gnu` | The same sections name x86_64 beside aarch64. The plan (`specs/open-ontology-v0.1-plan.md` line 58) keeps the full two-architecture matrix at the integration node. Naming the triple in deny config is the license-graph coverage for that documented target. It is not an amd64 product sign-off. |

Not required, and not proposed as deny targets:

- wasm32. `supply-chain/harness-dependency-delta.md` line 29: wasm packages are age-checked, and wasm is not a supported target matrix. No spec or docs section requires a wasm artifact.
- macOS and Windows. `docs/11` §1.1 says Docker Desktop on those systems is not gVisor and must not silently count as production. `docs/17` §7 says a successful macOS compile is not evidence that Linux deployment works. Earlier Apple Silicon supply notes are host observations.
- Android, musl, Redox, UEFI. Not named as acceptance targets. Omitting musl does not hide these two failures: a Linux musl triple is still `not(target_arch = "wasm32")`, so both crates stay out. A later musl requirement needs its own tree.

MSRV is workspace `rust-version = "1.94.0"` (`Cargo.toml` `[workspace.package]`). `docs/02-technology-stack.md` §6, line 189, sets the first MSRV at 1.94.0 because of SQLx 0.9. `arrow-array` 60.0.0 declares `1.88`. `rustls-platform-verifier` 0.7.0 declares `1.85`. Both are below 1.94.

## Viable candidate

Change only `ci/deny.toml` `[graph]`, keeping `all-features` and `no-default-features` as they are:

```toml
[graph]
all-features = false
no-default-features = false
targets = [
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
]
```

Use the string form. Do not attach a `features` array. Do not pass CLI `--target` in place of the checked-in list: the gate command is `cargo deny --config ci/deny.toml --locked check`, and the help text says CLI `--target` overrides the config.

Effect, derived from the predicates above and not from a new scan:

- For both triples, `target_arch = "wasm32"` is false. `arrow-array` selects `ahash` feature `runtime-rng` only. `compile-time-rng`, `const-random`, `const-random-macro`, and `tiny-keccak` leave the checked graph.
- The same predicate selects `rustls-native-certs` inside `rustls-platform-verifier` and does not select `webpki-root-certs`. The direct `oo-sdk` dependency and reqwest's `rustls` feature stay. Linux `RootTrust::Platform` still loads native CAs. `verify_server_cert` still takes `server_name`. `OnlyPem` and the connector's caller-CA path are untouched.
- The two triples unify with each other. Neither enables the wasm32 blocks, so the union does not reintroduce either crate. Apple, Windows, and Android crates drop out of the checked graph because those targets are not in the list. a110's license errors did not name those crates. Their licenses were inside the allow list when the all-target scan printed them. Dropping them is a consequence of the documented host set.

This is a target-policy edit with a documented support set. It is the minimal edit that removes both failing edges from the graph cargo-deny 0.20.2 actually checks. No product manifest, lock, vendor patch, or Arrow/TLS source change is required. `Cargo.lock` will still contain both crates, because Cargo records target-specific dependencies. A wasm32 build of this lock would still compile them and would still fail this allow list. Wasm stays unsupported.

The configured command on the current file remains a failure until that edit exists and a builder runs it. This analysis does not predict the exit code. Advisories, source allow-list, and the `num-bigint` warning still have to be read from that run.

## Rejected options

| Option | Why it is not the repair |
|---|---|
| Add `CC0-1.0` or `CDLA-Permissive-2.0` to `allow` | The task forbids allow-list expansion. `docs/02` §1 is a separate OSI/weak-copyleft rule. This proposal does not decide that question and does not treat it as permission to edit the allow list. |
| `exceptions`, `ignore`, `clarify`, or deleting the deny invocation | Forbidden by the task and by the current file, which has none of those escapes. |
| `exclude-dev = true` | External dev edges are already ignored. The failing edges are normal target dependencies. Workspace dev edges include the Arrow tests. |
| `all-features = true`, or another `default-features = false` on `oo-sdk` / `oo-connectors` | Already false where it can be set. `ahash` and `webpki-root-certs` are not selected by those default features. Unification would still see the wasm32 cfg while `targets` is empty. |
| Drop `arrow-array` or move off series 60 | `docs/03` §4, line 102, requires an Arrow `RecordBatch` stream. `docs/04` §3, line 51, requires one Arrow batch shape for every path. `TypedBatch::record_batch` returns `arrow_array::RecordBatch` from the pinned 60.0.0 crates. crates.io API on 2026-10-03 reports `arrow-array` max version `60.0.0`, created `2026-09-15T15:30:02.038482Z`, not yanked. No newer 60.x exists to diff. A downgrade leaves the approved series. |
| Patch or vendor `arrow-array`, `ahash`, or `const-random-macro` | The wasm32 line is upstream. A patch would be a new vendored fork. The task forbids silent vendoring. Linux builds do not need that fork to keep `runtime-rng`. |
| Replace `tiny-keccak` 2.0.2 | `const-random-macro` requires `=2.0.2` in its manifest. The crate's license field is CC0 for that line. No other version satisfies the requirement without editing the macro. |
| Bump `rustls-platform-verifier` to 0.7.1 | crates.io API on 2026-10-03: 0.7.1 created `2026-09-24T20:39:00.557360Z`, not yanked, max version 0.7.1. Downloaded crate sha256 `1167586491e2b18b8bfbb293e8180ec17c201c4f076d7cb3070ca964e7598f98`. Its `Cargo.toml.orig` still has `webpki-root-certs = "1"` under `cfg(target_arch = "wasm32")` and under dev-dependencies. The bump does not remove the all-target CDLA edge. `oo-sdk` also records that 0.7.1 was refused in the original review. Transitive 168-hour age, yank status, and MSRV of a 0.7.1 lock are not verified here because the bump is not proposed. |
| Remove `rustls-platform-verifier` or switch production trust to `OnlyPem` only | `RootTrust::Platform` is the production default. Linux trust is `rustls-native-certs`, which is the crate the wasm32 list does not use. Removing the verifier would drop required platform trust. reqwest feature `rustls` would still depend on it on Linux. |
| Switch reqwest off `rustls` onto native-tls | Changes the TLS stack and the host/CA path a084 reviewed. The CDLA crate is not on the Linux edge. |
| A target list that omits `x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu` | Both are documented. Omitting one would shrink the checked graph below the support set. |
| A target list that adds wasm32, or any list chosen because it happens to exclude a failing crate while dropping a documented host | That is the concealment case. The list above is the two documented GNU Linux triples, and both failing crates are outside those triples by upstream `cfg`. |

## Preservation

No intended change to Arrow 60 conversion or canonical bytes. `crates/oo-connectors/src/types/arrow_batch.rs` builds `RecordBatch`, `Decimal128Array`, and `Decimal256Array` from `arrow-array` / `arrow-buffer` / `arrow-schema` 60.0.0. Canonical hashes go through `oo-core`, not through Arrow buffers. `ahash` is an internal arrow-array dependency. On the two Linux triples it stays `runtime-rng`.

No intended change to MSRV 1.94, to connector `verify-full` caller-CA trust, or to SDK hostname checks and `RootTrust::Platform`.

No new pin, so this proposal has no new 168-hour clock, yank check, or license-body obligation. The crates that remain on the Linux graph are already in the lock. A later implementation that still wants a fresh age walk can reuse a110 `licenses/provenance-version.tsv` for the Arrow additions and must not treat this document as that walk.

## Later validation, not done here

A builder slot that is not a112 or a114, after a root-writer handoff limited to `ci/deny.toml`:

1. Record the new deny file sha256. Run `cargo deny --config ci/deny.toml --locked check` with the pinned cargo-deny 0.20.2 on Linux Rust 1.94. Keep the raw stderr and the exit. Read advisories, bans, licenses, and sources separately.
2. Show `cargo tree` for `tiny-keccak@2.0.2` and `webpki-root-certs@1.0.9` at `--target aarch64-unknown-linux-gnu`, `--target x86_64-unknown-linux-gnu`, and `--target all` / wasm32. The Linux trees should omit them. The all-target or wasm32 tree should still show them. That pair is the proof the lock was not laundered.
3. Focused product checks only where behavior could move. It should not move. PostgreSQL: the live Arrow gold a110 ran (`independent_live_gold` / `record_batch` / canonical hash), with a real fixture. `cargo test -p oo-connectors` without `OO_PG_FIXTURE` exiting 101 at `live_source_catalog` stays a missing-fixture result. SDK: `RootTrust::Platform` and `OnlyPem`, wrong CA, wrong hostname, proxy disabled, retry header. Run those if the slot's scope includes the SDK. They confirm the manifests did not move.
4. Do not run an all-workspace matrix for this file. amd64 product tests stay at the integration node unless the x86_64 tree contains a license failure the arm64 tree does not.
5. Independent fixed review. This session cannot approve the edit.

If a reviewer refuses any `[graph].targets` change, there is no remaining repair inside the current manifests. The wasm32 dependencies are unconditional for that architecture, series 60 has no newer crate, and 0.7.1 has the same CDLA edge. The gate stays unsigned. The failing command stays the command on the unmodified `ci/deny.toml`.
