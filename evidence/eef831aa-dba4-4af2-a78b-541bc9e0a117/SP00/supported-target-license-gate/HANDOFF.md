# Handoff

Independent fixed review of session `eef831aa-dba4-4af2-a78b-541bc9e0a117`. The implementer does not approve supply or release.

## What landed

`ci/deny.toml` `[graph].targets` is the two strings `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu`. Every other deny field is unchanged. Evidence for the run is this directory. No root or subcrate manifest, lock, vendor, source, supply policy, docs, spec, plan, or ledger write. No new dependency or pin.

## Check these artifacts

- `identity/deny.toml.diff` is four added lines. Post-edit sha256 `6483e22c78bedfbaa55d1558470a70c79552bac50377a4da802bad0a2df1f3e3`. Pre-edit blob `367598cb3768b8c8af698041cd0f84ce6f8fb7be`.
- `Cargo.lock` sha256 `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f`, blob `15ae1071c7236a956547e3f1d37d627b2cbebfce`. Rows in `identity/lock-rows-offending.txt` still name `tiny-keccak` 2.0.2 CC0-1.0 and `webpki-root-certs` 1.0.9 CDLA-Permissive-2.0.
- `raw/deny.exit` is `0`. `raw/deny.stdout` is `advisories ok, bans ok, licenses ok, sources ok`. Stderr is duplicate warnings only. Tool is cargo-deny 0.20.2, binary sha256 `44296adb80fd7d3a6ce8737e2806cca409a96757798b399f32f1b535df74884c`. Command had `--config` and `--locked` and no `--target`.
- Advisory database rev `f8dee89e1b2f2f1eaf548312df7655fe5202a302` (`raw/advisory-db.log`). Advisories ok is only as fresh as that checkout and that invocation.
- `linux/graph-digests.txt` hashes match before and after. `linux/GRAPH_MISMATCH` is a sidecar-path compare; read `linux/graph-content-match.txt` before treating it as a graph change.
- Linux `tree-features.counts` are zero for `compile-time-rng`, `tiny-keccak`, and `webpki-root-certs`, and non-zero for `runtime-rng` and `rustls-native-certs`. wasm32 and all-target counts keep the offending names. Inverse stdout for those crates is empty on Linux (with the "nothing to print" stderr) and non-empty on wasm32 and all-target. Keep the all-target files visible as the failing graph.
- Cite `facts.txt`, not `facts.broken-id-match.txt`. Linux `in_resolve=yes` for `tiny-keccak` is unified metadata membership. Pair it with `resolve_edge` cfgs and with cargo-tree. `webpki-root-certs` has no Linux resolve node.
- a110 `deny.exit` remains `4`, `licenses FAILED`, same lock and pre-edit deny blob (`identity/a110-deny.sha256`). That scan was not repeated.

## Scope the reviewer should hold

The license exit covers crates reachable for at least one of the two configured triples after cfg filtering. It does not cover wasm32, other cfgs, or every package in the lock. x86_64 rows were resolved on the arm64 Rust 1.94.0 builder (`RUSTUP_TOOLCHAIN` overrode channel `1.98.1`). They are graph coverage. They are not an x86_64 binary, a gVisor run, or `AT-OPS-01` runtime acceptance. x86_64 additionally resolves `core_detect`, `multiversion`, `multiversion-macros`, and `rustversion`; those crates were already locked and are inside the union deny checked.

`jsonschema` 0.57.0 enables ahash `serde` plus default (`runtime-rng`). That path is present on Linux and does not enable `compile-time-rng`.

## Explicitly not rerun

a110 deny, a110 cargo audit, PostgreSQL, Arrow live gold, SDK TLS runtime, `cargo test -p oo-connectors`, amd64 product binaries, the all-workspace matrix, and `cargo deny --target all`. Existing source, type, and SDK proof is retained because source, manifests, lock, and the Linux cargo graphs did not change. Do not describe those retained runs as fresh results of this session.

## Review limits

This commit cannot sign full types, SP00, or release. It does not clear the a110 CC0 or CDLA findings. A later reviewer who wants the deny pruned-crate list needs a new invocation with debug logging; this stderr does not contain it. A later reviewer who wants machine execution on x86_64 needs an integration-node builder. This session's builder slot is released after own-container cleanup. Slot A a116 snapshot review was left in place. No foreign containers were pruned.

## Commit boundary

Local commit only, paths `ci/deny.toml` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a117/SP00/supported-target-license-gate/`. No `git add .`, no push, no PR, no release tag.
