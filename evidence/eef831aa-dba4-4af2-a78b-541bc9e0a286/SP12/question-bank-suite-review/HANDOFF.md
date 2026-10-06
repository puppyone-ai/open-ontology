# Handoff

Reviewer session `eef831aa-dba4-4af2-a78b-541bc9e0a286`. The only owned path is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a286/SP12/question-bank-suite-review/**`. Product crates, the plan, root `Cargo.toml`, root `Cargo.lock`, and the bench locks were not edited.

`REPORT.md` holds the separate verdicts. B-D07 is APPROVE_LIMITED for the question-bank suite on a hand fixture. B-D08 is APPROVE_LIMITED as an extension of the existing adapter, sql-runner, and suite. Read `FINDINGS.md` for the five preserved oracle runs, the canonical `as_of` rule, the presence-form gold cells, and the live TLS pair. Read `ORIGIN.md` for recipe paths. `linux/smoke/origin-recipes.txt` has the SHA-256 of those files at launch. `CLEANUP.txt` records the owned resources removed after the proof.

Raw proof:

- Green MSRV smoke, boundary, author proof, and fresh bank: `linux/smoke/`.
- Preserved oracle failures: `linux/smoke/attempt-001` through `attempt-005`.
- Private TLS proof: `linux/live/`.
- PostgreSQL launch record: `pg/`.
- Eleven seam blobs: `source/blobs/` and `linux/smoke/blob-sha256.txt`.
- Bench tree hashes: `linux/smoke/source/adapter.sha256`, `runner.sha256`, `suite.sha256`.

Inventory: `MANIFEST.sha256`, written by `tools/sha256_manifest.rs` inside the pinned image. `linux/manifest/launch.sh` hashes the evidence tree, copies the first console into `linux/manifest/console.log`, then hashes again. The refresh stdout directory `/tmp/oo-a286-eef831aa-dba4-4af2-a78b-541bc9e0a286/manifest-outside` is removed by that script. `linux/manifest/refresh-hostconfig.txt`, `refresh-removed.txt`, and `refresh.exit` are copied after the second hash, so those three files sit outside `MANIFEST.sha256`.

The green bank pipeline can be rebuilt from the fixed archive alone:

1. `git archive c551afea55d5a9164760878a5d6a76835b869875` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, `benchmarks/question-bank`, `benchmarks/sql-runner`, `benchmarks/query-result-adapter`, `benchmarks/question-bank-suite`, and `benchmarks/result-comparator`.
2. Rename that archive's `rust-toolchain.toml` so rustup stays on 1.94.0.
3. `cargo test --locked --offline` for the adapter, sql-runner, and `question-bank-suite --test cli --test compile --test sha256`, with `RUSTUP_TOOLCHAIN=1.94.0`. The suite proof also needs the runner, adapter, comparator, and a query double, and uses `OO_A267_SQL_RUNNER`, `OO_A267_ADAPTER`, `OO_A267_COMPARATOR`, and `OO_A267_QUERY_PG`.

The fresh hand bank is `oracle/fresh_bank.rs` with `oracle/query_pg.rs`. It is review evidence. It was not copied into the product tree. The TLS proof needs `pg/up.sh` and `cargo build --locked --bin query-pg` from the same archive, with libclang from `linux/live/extract-libclang.sh`. The example source is `linux/live/a286_probe.rs`. It is copied into `crates/oo-gateway/examples/` only inside the build tree.

a207 remains the only writer of the root manifest and the root lock.

This review stops at the two limited verdicts. The public 300-question bank, human annotation, a customer holdout, benchmark quality, parent acceptance, and a release remain open. The hand bank is a fixture.
