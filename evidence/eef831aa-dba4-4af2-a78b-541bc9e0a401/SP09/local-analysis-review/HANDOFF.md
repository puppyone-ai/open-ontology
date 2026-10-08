# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a401`. SP09-W05 local analysis review.

## Verdict

The evidenced loop writes CSV, Parquet, Arrow, Excel, the Chinese chart, the checked font, and the manifest from a fresh PostgreSQL export, and it checks the DuckDB aggregate against the exact decimal groups. `closes_sp09_w05` stays false. `audited` stays false. Reviewers stay unassigned. Do not mark the ledger. Do not treat this commit as acceptance of SP09, AT-API-04, or the parent goal.

This card does not sign its own acceptance. `tools/local-analysis/DEPENDENCY-PROPOSAL.md` is a proposal. `scan_recorded` is false because cargo-audit and cargo-deny were not run. That gap is separate from the pin check. The proposal is not an audit and not a baseline edit.

## Owned path

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a401/SP09/local-analysis-review/**`

`tools/local-analysis` and the rest of the product tree stayed read-only. The overlay files `linux/focused/a401_bounds.rs` and `linux/focused/extract_otf.rs` were copied into the private archive after the blob check. They are not in commit `e2732e323915a332a85f008c605b50c1f4d5e948`.

## Proof to read

- Fixed commits: analysis `e2732e323915a332a85f008c605b50c1f4d5e948` (subtree `21d95d0f8f16aa8bc464af8f00763b907e650869`), producer `21cc96241a841b47dbbcb116549030e38a643ba0`, exporter `62b4afecdd377fa94310f46467c48a01e61d14db`. Root lock `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`.
- `linux/smoke/smoke.exit` is 0. Network none. Rust 1.94.0. Container removed before the proof.
- `pg/up.exit` is 0. Version `17.10 (Debian 17.10-1.pgdg12+1)`. No published port. Memory 1 GiB. One CPU. Id `fabf791cfe6badfbc946922d224eef5694ba84d0f51d2d85c217e443a8c567b1`.
- `linux/proof/analysis-proof.txt` is `ok`. `analysis-exits.txt` is `amount=0 wide=0 partial=1 cancel=1 engine=3 formula=0`.
- Final `linux/proof/chain.exit` and `linux/proof/docker-run.exit` are 0. Those values are the Clippy resume. The analysis container’s exit 101 is preserved in `linux/proof/failed-attempts/04-clippy-needless-borrow/`.
- `linux/proof/focused-test-final.exit` is 0 (7 passed). `linux/proof/clippy.exit` is 0.
- `linux/proof/xlsx-scan.txt` and `linux/proof/xlsx-formula.txt` are `xlsx_scan_ok`.
- Amount CSV SHA-256 `2edb40e84bc45e0391474c745933839d410282614c337b164ce66f53476a9a8a`. Manifest query id `q_f714069af89b93f6`.
- Fresh stdout SHA-256 `f1f1c1964c646475ff6a1a97363dc99c59a68abd4fb26d5dd2961b7ed3705b55` (1527 bytes). Export CSV SHA-256 `f5b287fff0d197d55a1d67893178ff7e68ed5a1338830d24ea8fe0f11db2ca3d`. Audit SHA-256 `ed4819d6c664163bb1aa68cb61355ee5e5c0efcca41603886787aeb789d544c5`.
- DuckDB unpacked SHA-256 `c0ac0b79e243ee312b3af29c34ac0b94e518aa44c4345036d5d96c1c042c9d48`. Font SHA-256 `2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b`. The font file is gitignored.
- Chart: `linux/proof/out/analysis/analysis.svg`. Wide chart: `linux/proof/out/analysis-wide/analysis.svg`.
- `REPORT.md` and `FINDINGS.md`.

## Raw exits

Analysis and export: amount 0, wide 0, formula analysis 0, partial export 1, partial analysis 1, cancel 1, missing CLI 3. Live negative commands: import-pg-not-ready 1, before-analyze 5, hidden 6, provision-loose 7, provision-over 7. `phases/test-live.exit` is 0. Failed builder exits, in order: 101, 125, 125, 101, 126, 1. Final builder exit 0.

## Not repeated

Producer unit tests, producer Clippy, the author local-analysis matrix, and the earlier rust_exact refusal chain. This run built the bins, ran `a327_live::live_numeric_proof` once, exported three results, and ran the focused boundary test. See `linux/proof/producer-units-not-repeated.txt`.

## Manifests

`source/la-blobs.sha256` lists every blob of `tools/local-analysis` at the fixed commit. `source/expected.sha256` is the producer archive check. `source/dependency-age.txt` is the age and scan record. `SHA256SUMS` lists every file under this evidence directory, repository-relative, and skips itself and `SHA256SUMS.sha256`. `SHA256SUMS.sha256` is the digest of `SHA256SUMS`. The git commit is not inside the manifest. `linux/proof/out/analysis/NotoSansCJKsc-Regular.otf` is in `SHA256SUMS` and is omitted from git.

## Cleanup

Removed container `oo-a401-pg`. `oo-a401-proof` was already removed by the launch scripts. Removed volumes `oo-a401-eef831aa-cargo`, `oo-a401-eef831aa-target`, `oo-a401-eef831aa-native`, and `oo-a401-eef831aa-pgdata`. Removed network `oo-a401-eef831aa-net`. Removed `/tmp/oo-a401-eef831aa-dba4-4af2-a78b-541bc9e0a401`. Afterward this owner had no container, volume, or network. No prune. Left running: `oo-fe01-rust-m14`, `oo-fe01-pg-m14`, `oo-a404-pg`, `oo-a402-pg`, `oo-a397-pg`, `oo-a398-pg`, `oo-a383-pg`, and `oo-a373-pg`. A scan of this evidence tree found no private-key block and no `password=` assignment. `pg/server-san.txt` is the public certificate text. `pg/up.sh` names password files and does not contain the password values.
