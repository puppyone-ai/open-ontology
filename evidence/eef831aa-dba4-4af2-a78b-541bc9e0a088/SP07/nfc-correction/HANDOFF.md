# Handoff — a088 builder NFC correction

Stop here for an independent review of the delivery commit. Do not treat this slice as a finished SP07 or a finished vertical loop. Do not mark findings 1, 2, or 5 closed from this handoff.

## What to review

The delivery commit adds or changes only the NFC correction and this evidence directory:

- `crates/oo-builder/src/nfc.rs`
- `crates/oo-builder/src/nfc_tables.rs` (deleted from the product; still in git history)
- `crates/oo-builder/src/lib.rs`
- `crates/oo-builder/src/ident.rs`
- `crates/oo-builder/Cargo.toml`
- `crates/oo-builder/NOTICE.md`
- `crates/oo-builder/ERRATUM.md`
- `crates/oo-builder/tests/nfc_conformance.rs`
- `crates/oo-builder/tests/unicode/`
- `Cargo.lock` (two dependency names under `oo-builder` only)
- `NOTICE`
- `contracts/builder/structure-draft/README.md`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a088/SP07/nfc-correction/`

No connector, xtask, harness, supply-chain baseline, or deny policy edits. No golden regeneration. Older evidence trees were not rewritten. `baseline/attempt1/` and `fixed/attempt1/` keep their defer exits of 3.

Linux tested snapshot base commit `fc877c482a61729ea46e779a674a46f8f72c80c5`, tree `d40755c31abd82d932b56886aa048ab4d7aa5e4b`, parent `bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df`, plus the worktree overlay above. The overlay hashed equal to the snapshot before commit. Docs commits `36521150fb5a449850d854c3c8ffd5273f259fa2` and `93867666747a46200ee5b34fe5a490d9038096d1` landed after the snapshot. They do not change the builder, the lock, `NOTICE`, the structure-draft README, `oo-core`, `oo-jobs`, or `oo-connectors`. The delivery commit hash, tree, and parent are in `delivery-commit.txt`.

Reviewed failure input remains a071 commit `4c989bce58407cf19d3d93dc600162d7322d92f8`. Closure-only repair remains `eebc0ea3b835655fdb3ab088e6f7351d0d316a43`. Connector pin source remains `2980e85af22c3ade05272c66c7d2835b52730322`, which is not an approval of this correction.

## Commands and exits

Baseline smoke of `39376ac1e68c0ee066f1c46cd8c7144ea598ef52`: fetch 0, test 0, clippy 0, clippy-component 0, docker-run 0.

Fixed run of the overlay: fetch 0, tree 0, test 0, clippy 0, clippy-component 0, worst 0, docker-run 0.

Image both runs: `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

`rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8.

```
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings
```

Fixed test counts: 27 lib passed, 0 binary, 4 `nfc_conformance` passed, 4 CLI passed. Conformance assertions that passed: Unicode 16 rows 19965, Unicode 17 rows 20034, assigned failures 0, unassigned deltas 0, parity failures 0, 34 new marks, 1112064 scalars. U+1E14 and U+0045 U+0304 U+0300 normalize to U+1E14. `e` U+1ACF U+0323 normalizes to U+1EB9 U+1ACF.

`unicode-normalization` 0.1.25, checksum `5fd4f6878c9cb28d874b009da9e8d183b5abc80117c40bbd187a1fde336be6e8`, `UNICODE_VERSION` (17, 0, 0). Direct dependency `tinyvec` 1.13.3 feature `alloc`, checksum `fd3ca314f692efd6c868f8408f53fe444634a845f96c028b97d35f6a1f79f0ee`. Same version the connector already pins. `cargo-deny` and `cargo-audit` were not installed and were not run.

Raw logs: `baseline/console.log`, `fixed/console.log`. Phase integers are the `*.exit` files.

## Left open

Findings 1, 2, and 5 stay open for independent fixed-SHA review. Findings 3 and 4 stay as repaired by `eebc0ea` only. Also open: deny/audit execution, unsigned numeric scale, physical 63-byte mapping, real PostgreSQL import, profile, classification, publish, RBAC, query, customer access, CI, runsc, and release.

Own containers, networks, volumes, and `/tmp/oo-a088-*` were removed after the logs were copied. See `cleanup.txt`. No push, pull request, or release was made.
