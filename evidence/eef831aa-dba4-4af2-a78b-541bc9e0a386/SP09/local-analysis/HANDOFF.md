# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a386`. SP09-W05 local analysis.

## Verdict

The evidenced loop writes CSV, Parquet, Arrow, Excel, the Chinese chart, the checked font, and the manifest from a fresh PostgreSQL export, and it checks the DuckDB aggregate against the exact decimal groups. `closes_sp09_w05` stays false. `audited` stays false. Reviewers stay unassigned. Do not mark the ledger. Do not treat this commit as acceptance of SP09, AT-API-04, or the parent goal. Codex updates the ledger.

This card does not sign its own acceptance. The proposal under `tools/local-analysis/DEPENDENCY-PROPOSAL.md` is not an audit and not a baseline edit.

## Owned paths

- `tools/local-analysis/`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a386/`

## Proof to read

- `linux/full-test/test.exit` is 0. `linux/full-test/clippy.exit` is 0. `linux/full-test/fetch.exit` is 0. `linux/full-test/generate-lockfile.exit` is 0. `components.txt` includes `clippy-aarch64-unknown-linux-gnu`.
- `linux/full-test/failed-attempt-1` and `failed-attempt-2` keep the offline-before-fetch exit 101 and the `ChunkReader` compile failure.
- `linux/test/clippy.exit` remains 1 from the missing Clippy component. `linux/chain/analysis-exits.txt` remains `amount=0 wide=0 partial=1 cancel=1 parquet=3 xlsx=3 engine=3`.
- `linux/chain-full/chain.exit` and `docker-run.exit` are 0. `analysis-proof.txt` is `ok`. `clippy.exit` is 0. `xlsx-scan.txt` is `xlsx_scan_ok`. `analysis-exits.txt` is `amount=0 wide=0 partial=1 cancel=1 engine=3`.
- `linux/chain-full/failed-attempt-1` is the deflate `grep` miss. The product commands in that attempt had already returned the same exit line.
- Chart: `linux/chain-full/out/analysis/analysis.svg`. Wide chart: `linux/chain-full/out/analysis-wide/analysis.svg`. Workbook: `linux/chain-full/out/analysis/analysis.xlsx`. Font digest `2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b`. The font file is gitignored.
- Amount CSV SHA-256 `2edb40e84bc45e0391474c745933839d410282614c337b164ce66f53476a9a8a`. Manifest query id `q_49b26b99a4752eca`.
- Fresh stdout SHA-256 `15ba2ac4a9ac5e86218741a119cb944e0098f88137c4292eb6aceeed2f5b4001` (1527 bytes). Export CSV SHA-256 `f5b287fff0d197d55a1d67893178ff7e68ed5a1338830d24ea8fe0f11db2ca3d`. Audit SHA-256 `b0579351099153df2a5076c4744018f6fe3d71bbfb7dfe590aee3289e3307bb5`.
- `pg/up.exit` is 0. Version 17.10. No published port. Memory 1 GiB. One CPU. Current inspect id `29fe08709fa2a61f249265a4a4b817b4eba58243d69067bd0942b474c257ef1d`.
- `REPORT.md` and `FINDINGS.md`.

## Not repeated

Producer unit tests and producer Clippy from the a340 record. Each chain built the bins and ran `a327_live::live_numeric_proof` once. See `linux/chain/producer-units-not-repeated.txt` and `linux/chain-full/producer-units-not-repeated.txt`.

## Manifests

`source-inventory.txt` lists every file under `tools/local-analysis` with repository-relative paths. `SHA256SUMS` lists every file under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a386` and `tools/local-analysis`, repository-relative, and skips itself and `SHA256SUMS.sha256`. `SHA256SUMS.sha256` is the digest of `SHA256SUMS`. The git commit is not inside the manifest. `linux/chain-full/out/analysis/NotoSansCJKsc-Regular.otf` is in `SHA256SUMS` and is omitted from git. From the repository root, `shasum -a 256 -c evidence/eef831aa-dba4-4af2-a78b-541bc9e0a386/SP09/local-analysis/SHA256SUMS` checks the list.

## Cleanup

Removed containers `oo-a386-pg` and `oo-a386-full-test`. Removed volumes `oo-a386-eef831aa-cargo`, `oo-a386-eef831aa-target`, `oo-a386-eef831aa-native`, and `oo-a386-eef831aa-pgdata`. Removed network `oo-a386-eef831aa-net`. Removed `/tmp/oo-a386-eef831aa-dba4-4af2-a78b-541bc9e0a386`. `oo-a386-chain-full` was already removed by `launch.sh`. Afterward this owner had no container, volume, or network. No prune. Left untouched: `oo-a373-pg`, `oo-a383-pg`, `oo-a395-build`, and the exited `oo-a064`, `oo-a373`, and `oo-a383` rust containers. Password and key bytes were absent from the evidence tree. The only overlap with `/tmp` secrets was the public SAN template that `pg/up.sh` writes.
