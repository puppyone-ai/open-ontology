# Handoff

Role-pack command on source commit `af3308c32fa9e90bb29fab970b691d57dbc35279`, tree `ef9b913640afd14cb87c9e79baaa321ebb205560`, parent `44153c488720cd290e9363763c60b0830717a9d6`. Fixed library base `eff1959bf028a65b20b7fca855d79c23e4b188fd`, tree `2f08d4ecef7ad2f3b617e8a04afcb45f0cf0186f`.

The requirement record is `REPORT.md`. The author notes are `FINDINGS.md`. The digest procedure is `ORACLE.md`. `source/blobs.sha256` lists content hashes and git blob ids. `MANIFEST.sha256` lists every file in this directory except itself.

Deciding proof: `linux/commit/` on the git archive of `af3308c3`. Clerk digest `1ab9c3bc971f87aab7d71b400432de2aacee3804579b4870fa93e5cd19854522`. Audit digest `7add1ff8fa967b47e6cab4ac3a187ed885e68b86902e5d1b05a59201f14242be`. Pre-edit smoke is `linux/preedit/` (`role-pack` absent, worst 0). Preserved fixture failures are `linux/focused/` and `linux/focused-2/`. The green overlay rerun is `linux/focused-3/`.

Owned product paths in `af3308c3`:

- `crates/oo-builder/src/bin/oo-builder.rs`
- `crates/oo-builder/src/lib.rs`
- `crates/oo-builder/src/role_pack_cli.rs`
- `crates/oo-builder/tests/role_pack_cli.rs`
- `contracts/builder/role-pack-cli-v1.md`

`role_pack.rs`, drafts, captured catalog code, core, the role-pack contract, the lock, the gateway, the spec, and the ledger were not part of this commit. The worktree lock stayed dirty and was not archived. Its committed blob matches the fixed base: `8364c36f2c70919cd3a1df18a2e57698a44051a8`.

Independent review is next. This file does not approve SP07-W06. Visibility input is public data and is not a live permission proof. A successful command is not SQL execution and not a grant. Nothing was pushed.

`CLEANUP.txt` records removal of this task's network, cargo volume, target volume, and `/tmp` tree. Launch scripts already removed the builders. The pinned Rust image stays. `docker prune` was not run.
