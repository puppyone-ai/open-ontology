# Handoff

Owner of this delivery: session `eef831aa-dba4-4af2-a78b-541bc9e0a225`, paths `benchmarks/trial-statistics/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a225/SP12/trial-statistics/**` only.

The next reader can rebuild from the fixed archive without editing the root lock:

1. `git archive c2db9c430b822a824a1062b6c3c7345924df7dd8` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, and `crates`.
2. Rename that archive's `rust-toolchain.toml` so rustup does not select 1.98.1.
3. Copy `benchmarks/trial-statistics` onto the archive. Delete the copied `Cargo.lock` if the next proof must regenerate it inside the archive. The committed lock is the attempt 4 lock.
4. `cargo test --locked --offline --manifest-path benchmarks/trial-statistics/Cargo.toml` with `RUSTUP_TOOLCHAIN=1.94.0`.

Do not run `cargo update` for this crate. a207 remains the only writer of the root manifest and the root lock.

Proof to read first: `linux/impl-4/`. Smoke: `linux/smoke/`. Failed attempts stay in place. Contract: `benchmarks/trial-statistics/CONTRACT.md`.

Private resources for this session are the network `oo-a225-eef831aa-net` and the volumes `oo-a225-eef831aa-cargo` and `oo-a225-eef831aa-target`, plus `/tmp/oo-a225-eef831aa-dba4-4af2-a78b-541bc9e0a225`. They are removed at the end of this delivery. Do not prune and do not remove another owner's containers.

The second inventory stdout stays at `/tmp/oo-a225-eef831aa-dba4-4af2-a78b-541bc9e0a225/hash-again.log` and is not part of the hashed tree. Cleanup removes that file with the private tmp directory.

This delivery stops at the paired trial-statistics primitive. B-D03 acceptance, a question bank, a quality benchmark, and a release remain open.
