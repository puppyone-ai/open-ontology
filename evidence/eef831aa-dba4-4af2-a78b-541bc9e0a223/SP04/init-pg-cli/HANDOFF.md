# Handoff

Product commit `6e1e6e576d1afd2bde10dedad649f09dee95bd34` adds:

- `crates/oo-pipeline/src/bin/init-pg.rs`
- `crates/oo-pipeline/tests/init_pg_cli.rs`
- `contracts/pipeline/init-pg-cli-v1.md`

The binary is Cargo's autobin. `oo-pipeline.rs` was not edited, so `init-pg` is not a subcommand of `oo-pipeline`. Stdin protocol is `oo.pipeline.init-pg/v1`. The password and CA are file paths in that document. The command calls `target_installer::install`, `verify_readiness`, `install_full_import`, and `verify_full_import_readiness`.

Fixed archive for the proof was `c2db9c430b822a824a1062b6c3c7345924df7dd8`. Rebuild from that archive plus the three files, or from the product commit. Do not treat the dirty workspace HEAD as the archive. `RUSTUP_TOOLCHAIN=1.94.0` because `rust-toolchain.toml` at the archive says `1.98.1`.

Deciding Linux proof: `linux/proof/docker-run.exit` 0, clippy 0, offline tests 0, live test 0, `live/outcome.txt` `init_pg_cli_ok`. Failed attempts stay under `linux/proof/attempt-1`, `attempt-2`, and `attempt-3`.

Review the frozen files and the boundary cases already in `init_pg_cli.rs`. A new expansion needs its own frozen child before code. The open seam is a partial `_oo` schema that `oo_target_owner` cannot use: the installer returns a database permission error, and this CLI reports exit 1. The proof fixture authorizes that schema to `oo_target_owner` and records exit 6.

This evidence does not accept M1-D05A and does not publish.
