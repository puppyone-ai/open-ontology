# Supported-target license gate

Session `eef831aa-dba4-4af2-a78b-541bc9e0a117`. One configuration change: `ci/deny.toml` `[graph].targets` lists the two documented GNU Linux triples. The gate that consumed that file is pinned `cargo deny 0.20.2` with `--locked check` and no CLI `--target`.

This result licenses the union of those two triples under the existing allow list. It leaves `tiny-keccak` 2.0.2 (CC0-1.0) and `webpki-root-certs` 1.0.9 (CDLA-Permissive-2.0) in `Cargo.lock`. The unfiltered graph still fails the same allow list. That failure is the retained a110 deny scan, exit 4. This change does not approve wasm, the full lock, types, SP00, or a release.

## Identity

Work started at `a26e7d0344cd6ef2b4c40d2f33311a86f7327050`, tree `40e66a7f4e774c63e242e5d61e1762051daa5ac5`, parent `817af2888e71adcb2e2b0a6bd1f7456a16f3ee4f`. Fixed baseline `a6e002aad0aa79c4bae788a84fc62c65283e0c04` (tree `ca17b5cd44a123c613227357c61ab934560fdbbe`, parent `f7d90e2d7914c4c3b61dc3b38b83f42ad59ec649`) is an ancestor.

At report time HEAD is `3963fc2b0ece54cabe62071153918b2df815b1c7`, tree `9d4e40c009a575b73feed6233f234c04a148893e`. The two commits after the scan start are coordination only:

- `23cc9bec181c8f86ec7fe69bfddf5f0d225e2806` `execution/prompts/v01-pg-stream-wire-analysis.md` and `specs/open-ontology-development-ledger.md`
- `3963fc2b0ece54cabe62071153918b2df815b1c7` the same two paths

`HEAD:Cargo.lock` is blob `15ae1071c7236a956547e3f1d37d627b2cbebfce`. `HEAD:ci/deny.toml` is blob `367598cb3768b8c8af698041cd0f84ce6f8fb7be`. Both match the blobs at the scan parent and at a110/a115. Working tree diff outside this evidence directory is the four-line `ci/deny.toml` insertion. `identity/product-diff-after.txt` is empty. Lock, `crates/oo-sdk/Cargo.toml`, and `crates/oo-connectors/Cargo.toml` were not written.

Pre-edit worktree sha256:

| path | sha256 |
| --- | --- |
| `Cargo.lock` | `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f` |
| `ci/deny.toml` | `4320807bfe747fe339ce7bf539a1cff5604e8e0c7677760e2cbc7abc1868624b` |
| `crates/oo-sdk/Cargo.toml` | `9ce21ea310bc4bb34ee14fd7749e7b30b85114fa462eabd307e4b5f27183909d` |
| `crates/oo-connectors/Cargo.toml` | `347f21f56657900c99939d505b72d1e60e8c5a2b7c60177e0d6ff8e009f3f2cf` |
| `rust-toolchain.toml` | `7553618f800829444096beea3dce84c3d07585efda504c19adb6787369bbd9e8` |

Post-edit `ci/deny.toml` sha256 is `6483e22c78bedfbaa55d1558470a70c79552bac50377a4da802bad0a2df1f3e3`. `Cargo.lock` sha256 is unchanged. Phase 2 recorded the same deny sha in `raw/deny-toml-seen-by-phase2.sha256`.

## Documented targets

`docs/11-deployment-and-operations.md` line 27 names Linux x86_64 and aarch64 as the formal hosts, accepted separately. `docs/17-rust-implementation.md` line 97 requires verification on Linux x86_64, aarch64, and gVisor. Spec `REQ-CORE-06` (line 180), `REQ-OPS-02` (line 715), P14 (line 865), and `AT-OPS-01` (line 1181) name the same pair. `supply-chain/harness-dependency-delta.md` line 29 says wasm is age-checked and is not a supported target matrix.

The config strings are exactly:

```toml
targets = [
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
]
```

`all-features` and `no-default-features` stay false. Advisories, bans, licenses, and sources are unchanged. No allow entry, exception, ignore, clarify, exclude, exclude-dev, or CLI `--target`.

## Builder

One arm64 container at a time, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, platform `linux/arm64`. Host config for the main run: `Privileged=false`, `CapDrop=["ALL"]`, `no-new-privileges`, memory and swap 8589934592, NanoCpus 4000000000, PidsLimit 4096, private bridge `oo-a117-…-net`. Repo mounted read-only at `/src`. Evidence mounted read-write at `/evidence`. `CARGO_HOME=/cargo-home`, `CARGO_TARGET_DIR=/target/a117`, `HOME=/work/home`, `RUSTUP_TOOLCHAIN=1.94.0`.

Smoke, `linux/rustc-vv.txt`: rustc 1.94.0 (`4a4ef493e` 2026-03-02), host `aarch64-unknown-linux-gnu`. `linux/rustup-show.txt` says the active toolchain is 1.94.0 because `RUSTUP_TOOLCHAIN` overrides the file. Workspace `rust-toolchain.toml` still says channel `1.98.1`. That file was not edited.

Host `cargo`/`rustc`/`rustup`/`rustdoc` comm count was 0 before the main run and before each rescan (`linux/builder-count-before.txt`, `builder-count-launch.txt`, `builder-count-rescan.txt`). No second builder and no PostgreSQL. Own container, network, and volumes were removed after each run. No prune. Raw `ps` listings were reduced to those counts before commit because macOS `ps` comm included unrelated host executable paths.

`cargo install cargo-deny --version 0.20.2 --locked --root /cargo-home` exited 0 inside that container. `raw/cargo-deny-version.stdout` is `cargo-deny 0.20.2`. Binary sha256 `44296adb80fd7d3a6ce8737e2806cca409a96757798b399f32f1b535df74884c`. Fetch `--locked` exited 0.

Failed attempts kept under `linux/attempt1/`, `linux/attempt2/`, and `linux/rescan.log`:

- Attempt 1: probe `E0631` (`disp` on a slice). Config not edited.
- Attempt 2: probe test exit 126, `/tmp` is noexec. Binary moved to the cargo volume. Config not edited.
- Main driver: `linux/driver.exit`, `phase1.exit`, and `phase2.exit` are 0.
- Rescan 1: unused `disp_arr`, `-D warnings`, exit 1. Container removed.
- Rescan 2 and 3: exit 0. Rescan 3 is the facts files cited below. Rescan used `--network none`, memory 2g, cpus 2, and compiled the probe to `/work/meta_facts`. It did not rerun deny, fetch, or cargo tree.

## Deny result

Command, with config path inside the container: `cargo deny --config /src/ci/deny.toml --locked check`.

| record | value |
| --- | --- |
| `raw/deny.exit` | `0` |
| `raw/deny.stdout` | `advisories ok, bans ok, licenses ok, sources ok` |
| `raw/deny.stderr` | 43201 bytes, 11 `warning[duplicate]` lines, zero matches for `rejected`, `CC0`, `CDLA`, `tiny-keccak`, `webpki-root-certs`, or `error[` |

Duplicate warnings, bans `multiple-versions = "warn"`: base64, block-buffer, cpufeatures, crypto-common, digest, getrandom (3), hashbrown, num-bigint, sha2, syn, synstructure. They do not decide the exit code.

With two configured triples, cargo-deny 0.20.2 does not pass `--filter-platform` (that flag is added only for a single target). It loads one metadata snapshot and `krates` 0.21.2 `include_targets` keeps an edge whose cfg matches at least one configured triple. An optional dependency is kept only when a kept edge enables it. This invocation did not enable debug logs, so stderr has no gathered-crate count. The license section is fresh only for advisory database checkout `f8dee89e1b2f2f1eaf548312df7655fe5202a302`, commit time `2026-10-02 22:27:46 +0200`, subject assigning RUSTSEC-2026-0320 through 0325 (wasmtime). `raw/advisory-db.log` stores that rev. a110 `cargo audit` was not rerun.

## Per-target trees

`cargo metadata --locked --offline --format-version 1` and `cargo tree --locked --offline` ran before the edit and again after it. Exits are 0 for metadata, `-e features`, `-e normal`, and `-e all` on all four filters, both phases. Filters:

- `--filter-platform` / `--target aarch64-unknown-linux-gnu`
- `--filter-platform` / `--target x86_64-unknown-linux-gnu`
- `--filter-platform` / `--target wasm32-unknown-unknown`
- unfiltered metadata, and `cargo tree --target all`

Content hashes match before and after (`linux/graph-digests.txt`, `linux/graph-content-match.txt`). Metadata sizes: aarch64 1306716, x86_64 1316175, wasm32 1289353, all-target 1551334. Tree line counts: aarch64 normal 638 / features 1793; x86_64 normal 643 / features 1805.

`tree-features.counts` on each Linux triple: `compile-time-rng` 0, `runtime-rng` 2, `tiny-keccak` 0, `webpki-root-certs` 0, `rustls-native-certs` 2, `const-random` 0, `const-random-macro` 0, `arrow-array` 2. wasm32: `compile-time-rng` 1, `runtime-rng` 1, `tiny-keccak` 4, `webpki-root-certs` 2, `rustls-native-certs` 0, `const-random` 5, `const-random-macro` 2. all-target keeps both sides: `compile-time-rng` 1, `runtime-rng` 2, `tiny-keccak` 4, `webpki-root-certs` 2, `rustls-native-certs` 2.

Linux `tree-normal` ahash line: `ahash v0.8.12 default,getrandom,runtime-rng,serde,std`. Inverse `-e features -i ahash@0.8.12 -p oo-connectors` exits 0 with 1162 bytes of stdout and shows `ahash feature "runtime-rng"` under `arrow-array v60.0.0` and `oo-connectors`. The same tree also shows `jsonschema` 0.57.0, `jsonschema-value`, and `referencing` enabling ahash `serde`. ahash's own default is `std` and `runtime-rng` (`upstream-manifests/ahash-0.8.12/Cargo.toml.orig`). Those edges do not name `compile-time-rng`.

Inverse `-i rustls-native-certs@0.8.4 -p oo-sdk -e features` on aarch64 exits 0, stdout 827 bytes, stderr 0: `rustls-native-certs` to `rustls-platform-verifier` to `oo-sdk` and to reqwest `rustls`. The x86_64 inverse for the same spec is the same shape (exit 0, non-empty). Declared normal edge: `cfg(all(unix, not(target_os = "android"), not(target_vendor = "apple"), not(target_arch = "wasm32")))`.

Inverse `-i tiny-keccak@2.0.2` and `-i webpki-root-certs@1.0.9` on both Linux triples exit 0, stdout 0, stderr 176 bytes:

```text
warning: nothing to print.
To find dependencies that require specific target platforms, try to use option `--target all` first, and then narrow your search scope accordingly.
```

Empty inverse stdout is not the proof of absence by itself. The proof is that exit, that warning, the zero feature counts, and the non-empty Linux trees that contain `runtime-rng` and `rustls-native-certs` and contain no `compile-time-rng`, `tiny-keccak`, or `webpki-root-certs` lines.

wasm32 inverse `-e features -i tiny-keccak@2.0.2` exits 0, stdout 5570 bytes: `tiny-keccak` 2.0.2, `const-random-macro` 0.1.16, `const-random` 0.1.18, `ahash feature "compile-time-rng"`, `arrow-array` 60.0.0, `oo-connectors`. wasm32 `-i webpki-root-certs@1.0.9 -e features` exits 0, stdout 327 bytes: `webpki-root-certs` 1.0.9 to `rustls-platform-verifier` 0.7.0 to `oo-sdk`. wasm32 native-certs inverse is empty with the same 176-byte warning. all-target inverse stdout is 5630 bytes for tiny-keccak and 942 bytes for webpki-root-certs, and native-certs stdout is 827 bytes. all-target keeps both the CC0 chain and the CDLA chain. wasm32 is not an approved target.

Package-name sets in `tree-normal` differ between the two Linux triples. x86_64 adds `core_detect` 1.0.0, `multiversion` 0.9.0, `multiversion-macros` 0.9.0, and `rustversion` 1.0.23 (248 names on aarch64, 252 on x86_64). No aarch64-only package name. Those four crates are already in the lock. The deny check covers the union, so they are inside the license result. The two triples are not the same graph as each other. Each triple is the same graph before and after the config edit.

Upstream license lines, `Cargo.toml` and `.orig`, are in `raw/upstream-manifests/license-lines.txt`. `tiny-keccak` is CC0-1.0. `webpki-root-certs` is CDLA-Permissive-2.0. ahash, const-random, const-random-macro, reqwest, and rustls-platform-verifier are MIT OR Apache-2.0. arrow-array is Apache-2.0 AND MIT. rustls-native-certs is Apache-2.0 OR ISC OR MIT. Lock rows for the two offending crates are `identity/lock-rows-offending.txt`. Checksums there match the published crate files recorded in `raw/upstream-manifests/crate-files.sha256`.

## Metadata membership is a wider set

`facts.txt` is a second view, from `cargo metadata`, scanned by `probe/meta_facts.rs` (`#![forbid(unsafe_code)]`, rustc 1.94.0, self-test passed on the last rescan). The first scan split package ids on whitespace. Cargo 1.94 ids are `registry+…#name@version` or `path+…#version`, so every `in_resolve` value was `no`. Those outputs are `facts.broken-id-match.txt`. The cited `facts.txt` uses exact id equality and also prints `resolve_edge` rows. Before and after facts hashes match:

| filter | facts.txt sha256 |
| --- | --- |
| aarch64-unknown-linux-gnu | `1058cdda58110b0d959b8e6eb5cd749e7fd715a8380e5250c066bd6b7a3b2193` |
| x86_64-unknown-linux-gnu | `cb6a2c11c940528752ba606c9d0f532bb4889f35ff29b2507c34714779417587` |
| wasm32-unknown-unknown | `af9b485d7f5c8429f98c56433d116cd8cbb589175054d85e4c32b1312a621d7c` |
| all-target | `75d2e9ecd6239758f0ed52565f543a0fefd4e871d104c79c90f531537941a2e7` |

Resolve node counts: aarch64 269, x86_64 272, wasm32 259, all-target 324. Unknown id rows on all-target: 0.

On both Linux filters, `tiny-keccak` 2.0.2 is `in_resolve=yes` and ahash's resolve feature list is `compile-time-rng,const-random,default,getrandom,runtime-rng,serde,std`. `resolve_edge` still shows `ahash` to `const-random` with `target=null`, and `const-random-macro` to `tiny-keccak` with `target=null`. `arrow-array` still lists both ahash edges: `cfg(not(target_arch = "wasm32"))` features `runtime-rng`, and `cfg(target_arch = "wasm32")` features `compile-time-rng`. `webpki-root-certs` has no package row and no resolve node on either Linux filter. Its normal edge appears on wasm32 and all-target with `cfg(target_arch = "wasm32")`.

That Linux `in_resolve=yes` for `tiny-keccak` is cargo's unified feature set inside `--filter-platform` metadata. It is not the cargo-tree activation set and it is not the cargo-deny checked set. `const-random` is an optional ahash dependency enabled by feature `compile-time-rng`. The only normal edge that requests `compile-time-rng` is the wasm32 arrow-array edge. cargo-tree on each Linux triple activates `runtime-rng` and does not activate `compile-time-rng`. cargo-deny, walking edges whose cfg matches a configured triple, drops that wasm32 edge and therefore drops the optional `const-random` chain. The same walk drops the wasm32 `webpki-root-certs` edge and keeps `rustls-native-certs`.

`rustls-platform-verifier` also declares a dev dependency on `webpki-root-certs` with `target=null`. This config does not set `exclude-dev`. cargo-deny 0.20.2 still ignores dev edges of non-workspace crates (`krates` dev scope `NonWorkspace` when `exclude-dev` is false). The all-target license failure a110 recorded is the normal wasm32 edge, which remains in the all-target tree.

A reviewer who reads only `in_resolve=yes` will mis-classify the Linux graph. Read `resolve_edge` targets together with `tree-features.counts` and the inverse stdout.

## Retained all-target failure

a110 deny was not rerun. Bound files, hashes in `identity/a110-deny.sha256`:

- `deny.err` `6368b7928790e9465feac64746391b951f9d2cdccb04b7347ccaaa3f6ac87d25`
- `deny.out` `738307e513dcfbf7f3cc0178af0fa13f078b69f9cf7d06ff985ce4a6d2c385b0`
- `deny.exit` file contains `4`

`deny.out` text is `advisories ok, bans ok, licenses FAILED, sources ok`. Same lock blob `15ae1071c7236a956547e3f1d37d627b2cbebfce` and same pre-edit deny blob `367598cb3768b8c8af698041cd0f84ce6f8fb7be`. Empty `[graph].targets` is why that scan saw both wasm32 edges. This gate's all-target trees are the current picture of that failing graph. They are labeled failing evidence. They are not an approval.

## What was not rerun

Source, both product manifests, and `Cargo.lock` are byte-identical, and each Linux cargo graph is byte-identical before and after the config edit. Existing source, type, and SDK runtime proof is retained on that basis.

Not rerun, and not claimed as fresh evidence:

- a110 `cargo deny` scan and a110 `cargo audit`
- PostgreSQL, Arrow live gold, and any connector fixture
- SDK TLS runtime (`RootTrust::Platform`, PEM-only, wrong CA, hostname, proxy, retry)
- `cargo test -p oo-connectors` (the known missing `OO_PG_FIXTURE` path was not executed)
- an x86_64 rustc, an x86_64 product binary, or a dual-architecture runtime matrix
- an all-workspace test matrix
- `cargo deny` with `--target all` or with any other CLI target
- a second deny after the facts rescan

x86_64 coverage in this directory is `cargo metadata --filter-platform` and `cargo tree --target` produced on the arm64 builder, plus the deny union of both triples. It is dependency-graph coverage. It is not dual-arch acceptance. gVisor was not run. No product behavior change was authorized or made.

## Residuals

- Supported-target license ok is not all-lock approval. Both offending crates remain in the lock with their checksums.
- wasm32 and `--target all` still show the CC0 and CDLA edges. Those trees stay in this evidence as failing graphs.
- Linux metadata resolve nodes still name `tiny-keccak` because cargo feature unification leaves the optional edge in the filtered metadata. cargo-tree and the deny license section are the activation evidence.
- Deny evaluated the union of the two triples in one invocation. It did not print its pruned crate list.
- Advisory ok is this database rev and this invocation.
- Duplicate-version warnings remain at warn.
- `linux/GRAPH_MISMATCH` is the sidecar-path compare, explained in `linux/graph-content-match.txt`.
- The first facts files are wrong on id match and are preserved as `facts.broken-id-match.txt`.
- Two coordination commits landed after the scan parent. They do not change lock, manifests, or deny bytes. This commit's parent is whatever HEAD is at commit time; the blobs above are the binding.
- This target-policy change cannot sign full types, SP00, or release, and it does not erase the a110 CC0/CDLA failure. Supply and release stay subject to independent fixed review.
