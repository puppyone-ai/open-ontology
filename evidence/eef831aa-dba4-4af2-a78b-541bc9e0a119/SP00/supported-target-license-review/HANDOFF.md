# a119 handoff

Verdict: APPROVE LIMITED documented-target license gate.

Object: `341ece0ea25ae50ff054a4581e8a3389ccb2dd66`
Tree: `b9b8d6119ab167aeba4d6feafe031c491ae3f727`
Parent: `3963fc2b0ece54cabe62071153918b2df815b1c7`

The earlier a119 actor reached outer exit 0 on the probe and exited before these reports and the evidence commit. This handoff is that completion. The raw exits were not rerun.

## Use this for

The `[graph].targets` pair `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu` in `ci/deny.toml` at the object above. Exact `cargo deny --config ci/deny.toml --locked check` exited 0 on advisories, bans, licenses, and sources. The debug graph has 259 crates, includes `core_detect` 1.0.0, `multiversion` 0.9.0, `multiversion-macros` 0.9.0, `rustversion` 1.0.23, `arrow-array` 60.0.0, and `rustls-native-certs` 0.8.4, and excludes `tiny-keccak` 2.0.2 and `webpki-root-certs` 1.0.9. Dropping `x86_64-unknown-linux-gnu` drops the first three of those. Adding wasm or clearing targets fails licenses on the CC0 and CDLA crates. Policy other than the target list is unchanged. Lock and the two product manifests match the parent.

## Leave unsigned

Wasm, the empty-target graph, and the a110 all-target RED (exit 4, `licenses FAILED`). Types and SP00. Any all-lock, legal, or release signoff. Product binaries, dual-arch images, and gVisor. Advisory results beyond db `f8dee89e1b2f2f1eaf548312df7655fe5202a302` fetched in this run (`2026-10-02 22:27:46 +0200`).

## Residuals the next reader should keep

`rustversion` is an `encoding_rs` build dependency on aarch64, so it is in both Linux deny sets. The x86_64-only names are `core_detect`, `multiversion`, and `multiversion-macros`. The x86_64-only control's crate set equals the two-target set because no crate in this lock is aarch64-only.

`tiny-keccak` `in_resolve=yes` on Linux filtered metadata is cargo feature unification. The checked graph drops it with `const-random` because `compile-time-rng` is the wasm arrow edge.

The Rust oracle under-keeps `mio` 1.2.3. Deny kept it. Marker crates match. Do not treat an oracle diff exit 1 as a license failure.

a117 `GRAPH_MISMATCH` and `graph-compare.txt` DIFFER lines are sidecar paths. Content hashes match before and after. `facts.broken-id-match.txt` is a bad id split. Attempt 1 and attempt 2 compile failures stay under `linux/attempt1` and `linux/attempt2`.

## Cleanup

a119 container, network, cargo volume, target volume, and archive directory are removed. Host rust comm count was 0 at launch. a116 `eef831aa-dba4-4af2-a78b-541bc9e0a116-pg` was up at launch and was not touched. No a119 builder remains. Coordinator owns checklists. No push.
