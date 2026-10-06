# Handoff

Owner of this delivery: session `eef831aa-dba4-4af2-a78b-541bc9e0a250`, paths `benchmarks/trial-report-adapter/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a250/SP12/trial-report-adapter/**` only.

The next reader can rebuild from the fixed archive without editing the root lock:

1. `git archive 8f192847ea7c4906c13f8419b5de2669875f7ef9` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, `benchmarks/sql-runner`, and `benchmarks/trial-statistics`.
2. Rename that archive's `rust-toolchain.toml` so rustup does not select 1.98.1.
3. Copy `benchmarks/trial-report-adapter` onto the archive. Delete the copied `Cargo.lock` if the next proof must regenerate it inside the archive. The committed lock is the attempt 3 lock.
4. `cargo test --locked --offline --manifest-path benchmarks/trial-report-adapter/Cargo.toml` with `RUSTUP_TOOLCHAIN=1.94.0`. The live proof also needs `CARGO_TARGET_DIR`, `OO_PROOF_DIR`, `OO_SQL_RUNNER`, and `OO_TRIAL_STATISTICS` pointing at the archived builds. `/tmp` in the proof container is `noexec`, so the stub binaries belong under the target directory.

Do not run `cargo update` for this crate. a207 remains the only writer of the root manifest and the root lock.

Proof to read first: `linux/impl-3/`. Smoke: `linux/smoke/`. Failed attempts stay in place. Contract: `benchmarks/trial-report-adapter/CONTRACT.md`.

Private resources for this session are the network `oo-a250-eef831aa-net` and the volumes `oo-a250-eef831aa-cargo` and `oo-a250-eef831aa-target`, plus `/tmp/oo-a250-eef831aa-dba4-4af2-a78b-541bc9e0a250`. They are removed at the end of this delivery. Do not prune and do not remove another owner's containers.

The second inventory stdout stays at `/tmp/oo-a250-eef831aa-dba4-4af2-a78b-541bc9e0a250/hash-again.log` and is not part of the hashed tree. Cleanup removes that file with the private tmp directory.

This delivery stops at the paired trial-report adapter. B-D05 acceptance, a question bank, a quality benchmark, and a release remain open.
