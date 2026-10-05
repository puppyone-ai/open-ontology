# Handoff

Owner of this delivery: session `eef831aa-dba4-4af2-a78b-541bc9e0a231`, paths `benchmarks/sql-runner/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a231/SP12/sql-runner/**` only.

The next reader can rebuild from the fixed archive:

1. `git archive 934c8098703b6462c4f774878d1a4241bd84905d` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, `benchmarks/query-result-adapter`, and `benchmarks/result-comparator`.
2. Rename that archive's `rust-toolchain.toml` so rustup stays on 1.94.0.
3. Copy `benchmarks/sql-runner` onto the archive. The committed lock is the impl-2 lock. Do not delete it unless the next proof must regenerate it inside the archive.
4. `cargo test --locked --offline --manifest-path benchmarks/sql-runner/Cargo.toml` with `RUSTUP_TOOLCHAIN=1.94.0`, one job, on the pinned linux/arm64 image.

Do not run `cargo update` for this crate. a207 remains the only writer of the root manifest and the root lock.

Proof to read first: `linux/proof/`. Passing tests: `linux/impl-2/`. Smoke: `linux/smoke/`. The failed compile stays in `linux/impl/`. Contract: `benchmarks/sql-runner/CONTRACT.md`.

Private resources for this session are network `oo-a231-eef831aa-net`, volumes `oo-a231-eef831aa-cargo`, `oo-a231-eef831aa-target`, and `oo-a231-eef831aa-pgdata`, container `oo-a231-pg`, and `/tmp/oo-a231-eef831aa-dba4-4af2-a78b-541bc9e0a231`. They are removed at the end of this delivery. Do not prune and do not remove another owner's containers. The foreign postgres `oo-a229-pg` is not this session.

The second inventory stdout stays at `/tmp/oo-a231-eef831aa-dba4-4af2-a78b-541bc9e0a231/hash-again.log` and is not part of the hashed tree. Cleanup removes that file with the private tmp directory.

This delivery stops at the trusted SQL runner and its four live suites. B-D04 acceptance, the adapter review, the comparator review, the query-pg review, a question bank, and a release remain open.
