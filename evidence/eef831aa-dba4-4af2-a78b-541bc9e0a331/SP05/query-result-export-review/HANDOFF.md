# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a331`. Independent review of FILE-D04 / E44. Fixed commit `15558917e2c5be12b74dd1d64fb542b7d05a03af`.

## Verdict

Child: **WITHHELD**.

The frozen exact-decimal requirement is unfulfilled. The baseline emits int text and refuses OID 1700. The exporter refuses a decimal kind. The proved CSV path does not replace that clause. Parent SP05-W06 stays open. This handoff does not release.

## Proof

- `linux/smoke/` exit 0 before any review overlay. Container `a807d442e09beb94e50554b9ba3f9ab78fbe32ef70ab8b19a4783e36d9b3a2ae`. Toolchain `rustc 1.94.0`. Archive `66095` files. Fixture cmp 0. SHA-256 `54cabda641bf1a2128e0246a78eb1a761895a0aa5a4226d136ad8aec399ad6eb`.
- `linux/proof/` exit 0. Container `b7245b71806f170795993ef24090f383110f4dfa4b08ec76e5664972807b0b88`. Author tests 15 + 3 passed. Clippy `-D warnings` exit 0. Oracle exit 0.
- Reviewer hand gold `oracle/hand-clerk.csv`, 171 bytes, SHA-256 `9c7c1562db84359e654e50f7f836465062759af12d84f36d52aa6f845c1607f1`.
- U+FEFF class `unprefixed` in `linux/proof/oracle-out/feff-class.txt`. Decimal and numeric hand rows exited 5 and wrote nothing.
- Package manifest stamp `c53ecce730033363051149ef6605af457e2c03f1910f82b3cb9b718c0d228f87`.

`origin/recipes.txt` records the a204, a200, a199, a266, and a325 blobs and the hashes of this review's scripts. `REPORT.md` and `FINDINGS.md` are the run record.

## Left open

Exact decimal from a compiled query-pg cell. xlsx. Download authorization, retention, deletion, and the reaper. The hand formula file and the FEFF row are not compiled receipts.

Smallest product fix for the withhold is a real exact-decimal cell from query-pg, then a copy of that text into the CSV and sidecar. The smallest FEFF change, if a later freeze asks for it, is one `'\u{FEFF}'` skip in `formula_prone`. Neither edit was applied.

## Resources

Smoke and proof containers were already removed by their launch scripts. `cleanup.txt` records removal of network `oo-a331-eef831aa-net`, volumes `oo-a331-eef831aa-cargo` and `oo-a331-eef831aa-target`, and `/tmp/oo-a331-eef831aa-dba4-4af2-a78b-541bc9e0a331`. No prune. Other owners' containers were not removed.

`SHA256SUMS` is written after this handoff by `tools/sha256.rs` in the inventory container. Files written after that manifest are `linux/inventory/inventory.exit`, `docker-run.exit`, `hostconfig.txt`, `console.log`, `mounts.txt`, `tmpfs.txt`, `container-removed.txt`, and `volume-removed.txt`. They are the hash-step remainder. The short-lived volume `oo-a331-eef831aa-hashcargo` is removed by that launch script after the manifest.

Root `Cargo.toml` and root `Cargo.lock` stay unchanged. Do not mark the ledger or the spec closed from this review.
