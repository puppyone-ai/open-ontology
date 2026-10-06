# Handoff

Owner of this development: session `eef831aa-dba4-4af2-a78b-541bc9e0a267`. Owned paths are `benchmarks/question-bank-suite/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a267/SP12/question-bank-suite/**`.

Verdict in `REPORT.md`: APPROVE_LIMITED for the suite compiler on fixed archive `567d621e2e52b7e0011bdb025dc9337e92ecc5a6`. Read `FINDINGS.md` for the preserved compile and clippy failures, the `not_found` adapter mismatch, and the contract sentence about JSON null. Read `ORIGIN.md` for recipe commits and digests. Read `CLEANUP.txt` for the resources removed after the inventory.

Raw delivered proof: `linux/impl-4/`. Earlier attempts, kept: `linux/impl/`, `linux/impl-2/`, `linux/impl-3/`. Smoke, before the overlay: `linux/smoke/`. Each impl directory has `crate-snapshot/` of the tree that container formatted. The host crate matches the impl-4 snapshot. Inventory: `crate-sha256.txt` and `MANIFEST.sha256`, written by `tools/sha256_manifest.rs` inside the pinned image. `linux/manifest/launch.sh` writes the first evidence manifest. `linux/manifest/refresh.sh` rewrites `MANIFEST.sha256` after `console.log`, `hostconfig.txt`, the exit file, and `CLEANUP.txt` are closed. The refresh stdout stays at `/tmp/oo-a267-eef831aa-dba4-4af2-a78b-541bc9e0a267/manifest-refresh.log` and is outside the manifest. Cleanup removes that directory.

The next reader can rebuild the green attempt from the fixed archive plus this crate:

1. `git archive 567d621e2e52b7e0011bdb025dc9337e92ecc5a6` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, `benchmarks/question-bank`, `benchmarks/sql-runner`, `benchmarks/query-result-adapter`, and `benchmarks/result-comparator`.
2. Rename that archive's `rust-toolchain.toml` so rustup stays on 1.94.0.
3. Overlay `benchmarks/question-bank-suite` from this commit, including its `Cargo.lock`.
4. `cargo test --locked --offline --manifest-path benchmarks/question-bank-suite/Cargo.toml` with `RUSTUP_TOOLCHAIN=1.94.0`, after prebuilding `sql-runner`, `query-result-adapter`, and `result-comparator` into a directory outside the test's cargo lock, and after `rustc --edition 2021` of `tests/support/query_pg.rs`. Set `OO_A267_SQL_RUNNER`, `OO_A267_ADAPTER`, `OO_A267_COMPARATOR`, and `OO_A267_QUERY_PG` to those binaries.

a207 remains the only writer of the root manifest and the root lock.

This work stops at the suite compiler. B-D06, B-D07 as an accepted plan line, the public 300-question bank, two-human annotation, a customer holdout, and a release remain open. The hand bank in `tests/support/mod.rs` is a fixture for this compiler. It is not a parent acceptance.
