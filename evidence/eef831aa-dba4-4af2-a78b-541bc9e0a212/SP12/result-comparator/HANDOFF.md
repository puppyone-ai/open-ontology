# Handoff

Owner of this delivery: session `eef831aa-dba4-4af2-a78b-541bc9e0a212`, paths `benchmarks/result-comparator/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a212/SP12/result-comparator/**` only.

The next reader can rebuild from the fixed archive without editing the root lock:

1. `git archive a661dcdf72ff91561ae6b7cb79916e582884d08a` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, and `crates`.
2. Rename that archive's `rust-toolchain.toml` so rustup does not select 1.98.1.
3. Copy `benchmarks/result-comparator` onto the archive.
4. `cargo test --locked --offline --manifest-path benchmarks/result-comparator/Cargo.toml` with `RUSTUP_TOOLCHAIN=1.94.0`.

The committed `Cargo.lock` is the attempt 5 lock. Do not run `cargo update` for this crate. a207 remains the only writer of the root manifest and the root lock.

Proof to read first: `linux/impl-5/`. Smoke: `linux/smoke/`. Failed attempts stay in place. Contract: `benchmarks/result-comparator/CONTRACT.md`.

Private resources for this session are the network `oo-a212-eef831aa-net` and the volumes `oo-a212-eef831aa-cargo` and `oo-a212-eef831aa-target`, plus `/tmp/oo-a212-eef831aa-dba4-4af2-a78b-541bc9e0a212`. They are removed at the end of this delivery. Do not prune and do not remove another owner's containers.

Not claimed: B-D01 acceptance, a question bank, a quality-benchmark pass, or a release.
