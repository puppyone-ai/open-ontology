# Handoff

Product commit `6ac2216505723b51ae116b1938aca47846ade35f` adds:

- `crates/oo-pipeline/src/bin/analyze-pg.rs`
- `crates/oo-pipeline/tests/analyze_pg_cli.rs`
- `contracts/pipeline/analyze-pg-cli-v1.md`

The binary is Cargo's autobin. `oo-pipeline.rs` was not edited, so `analyze-pg` is not a subcommand of `oo-pipeline`. Stdin protocol is `oo.pipeline.analyze-pg/v1`. The password and CA are file paths in that document. The command calls `target_statistics::analyze_mapped_shadows` and writes `oo.pipeline.analyze-pg-result/v1`. `published` and `estimate_is_exact` stay false on the success path recorded here.

Fixed archive for the proof was `a48db85fc13148874349d318affd995f7212188d`. Rebuild from that archive plus the three files, or from the product commit. Do not treat a later workspace HEAD as the archive. `RUSTUP_TOOLCHAIN=1.94.0` because `rust-toolchain.toml` at the archive says `1.98.1`.

Deciding Linux proof: `linux/proof/docker-run.exit` 0, unit tests 0, clippy 0, live test 0, catalog harness 0, secret scan 0. Failed attempts stay under `linux/proof/attempt-1` and `linux/proof/attempt-2`. The catalog file `overlay/catalog_proof.rs` is evidence. It is copied into the proof tree and is not part of the product commit.

Review the frozen files and the boundary cases already in `analyze_pg_cli.rs`. A new expansion needs its own frozen child before code. An open caller transaction cannot be fed to this process. Exit 6 `open transaction` is the library mapping. Lock timeout, statement timeout, and forged relation stay library mappings. This proof did not rerun those campaigns.

This evidence does not accept M1-D06B and does not publish.
