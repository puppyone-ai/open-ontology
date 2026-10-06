# Handoff

Owner of this development: session `eef831aa-dba4-4af2-a78b-541bc9e0a276`. Owned paths are `benchmarks/query-result-adapter/**`, `benchmarks/sql-runner/**`, `benchmarks/question-bank-suite/**`, and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result/**`.

The session record is `REPORT.md`. Read `FINDINGS.md` for the a267 `not_found` mismatch, the null-cell contract correction, and the two preserved live build failures. Read `ORIGIN.md` for recipe paths and digests. Read `CLEANUP.txt` for the resources removed after the inventory.

Raw proof:

- Pre-edit smoke and the red mismatch: `linux/smoke/`.
- Post-edit tests, clippy, and the green 5-question pipeline: `linux/post/`.
- Failed TLS builds: `linux/attempts/001/` and `linux/attempts/002/`.
- Successful TLS proof: `linux/live/`.
- Owned source copies: `source/blobs/`.

Inventory: `MANIFEST.sha256` and `source/adapter.sha256`, `source/runner.sha256`, `source/suite.sha256`, written by `tools/sha256_manifest.rs` inside the pinned image. `linux/manifest/launch.sh` writes the first evidence manifest. `linux/manifest/refresh.sh` rewrites `MANIFEST.sha256` after `console.log`, `hostconfig.txt`, the exit file, and `CLEANUP.txt` are closed. The refresh stdout stays at `/tmp/oo-a276-eef831aa-dba4-4af2-a78b-541bc9e0a276/manifest-refresh.log` and is outside the manifest. Cleanup removes that directory.

The next reader can rebuild the green bank pipeline from the fixed archive plus the eleven owned files:

1. `git archive 8a6395aef5b1d3b16d1b3d72833d2df2ebaa536e` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, `benchmarks/question-bank`, `benchmarks/sql-runner`, `benchmarks/query-result-adapter`, `benchmarks/question-bank-suite`, and `benchmarks/result-comparator`.
2. Rename that archive's `rust-toolchain.toml` so rustup stays on 1.94.0.
3. Overlay the eleven owned files from this commit.
4. `cargo test --locked --offline` for the three benches with `RUSTUP_TOOLCHAIN=1.94.0`, after the runner, adapter, and comparator binaries exist for the suite proof. The suite proof environment names stay `OO_A267_SQL_RUNNER`, `OO_A267_ADAPTER`, `OO_A267_COMPARATOR`, and `OO_A267_QUERY_PG`.

The TLS proof also needs the private Postgres from `pg/up.sh` and `cargo build --locked --bin query-pg` from the fixed archive, with `LIBCLANG_PATH` set from `linux/live/extract-libclang.sh`. The example source is `linux/live/a276_empty_result.rs`. It is copied into the archive's `crates/oo-gateway/examples/` only inside the build tree.

a207 remains the only writer of the root manifest and the root lock.

This work stops at the compatible empty-result seam. B-D07 as an accepted plan line, the public 300-question bank, human annotation, a customer holdout, and a release remain open. The hand bank is a fixture. It is not a parent acceptance.
