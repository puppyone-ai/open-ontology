# Handoff

Owner of this review: session `eef831aa-dba4-4af2-a78b-541bc9e0a255`, path `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a255/SP12/trial-report-adapter-review/**` only. No product path was edited.

Verdict in `REPORT.md`: APPROVE_LIMITED for the paired trial-report adapter at `8e7925d7f4576cb6a9f4027eca66ab1671e3fd1a`. Read `FINDINGS.md` for the live vectors, the structural all-fail gate, and the refusal pointers. Read `ORIGIN.md` for recipe hashes. Read `CLEANUP.txt` for the resources removed after the inventory.

Raw proof: `linux/proof/`. Smoke, before that proof: `linux/smoke/`. Sealed sources: `source/blobs/benchmarks/trial-report-adapter/`. Contract in that blob: `CONTRACT.md`. Independent driver: `tools/drive.rs` and `tools/stub.rs`. Inventory: `SHA256SUMS` and `crate-sha256.txt`, written by `tools/sha256_manifest.rs` inside the pinned image. The second-pass log is `/tmp/oo-a255-eef831aa-dba4-4af2-a78b-541bc9e0a255/hash-again.log` and is outside the manifest. Cleanup removes that directory with the rest of this session's tmp.

The next reader can rebuild from the fixed archive without a worktree overlay:

1. `git archive 8e7925d7f4576cb6a9f4027eca66ab1671e3fd1a` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, `benchmarks/sql-runner`, `benchmarks/trial-statistics`, and `benchmarks/trial-report-adapter`.
2. Rename that archive's `rust-toolchain.toml` so rustup stays on 1.94.0.
3. Fetch only the three standalone benchmark manifests. Do not `cargo update`. The crate lock stays a subset of the root lock.
4. `cargo test --locked --offline --manifest-path benchmarks/trial-report-adapter/Cargo.toml` with `RUSTUP_TOOLCHAIN=1.94.0`. The sealed live proof needs `OO_SQL_RUNNER` and `OO_TRIAL_STATISTICS` pointed at binaries built from that same archive.

a207 remains the only writer of the root manifest and the root lock.

This review stops at the adapter crate at that commit. B-D05 acceptance, the ledger, a question bank, a quality benchmark, and a release remain open. The all-fail statistics word `passed` is the structural gate recorded in `linux/proof/independent/structural-gate.txt`. It is not a quality winner and it is not a parent acceptance.
