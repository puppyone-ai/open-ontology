# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a386`. SP09-W05 local analysis.

## Verdict

Card: **not complete**. Do not mark the ledger. Do not treat `rust_exact` CSV and SVG as closure of Plan SP09-W05.

`closes_sp09_w05` stays false. DuckDB CLI, Arrow, Parquet, Excel, a CJK font file, and native-child kill stay outside this tree until a reviewed admission lands. The proposal under `tools/local-analysis/DEPENDENCY-PROPOSAL.md` is the change to review. It is not an audit and not a baseline edit.

This card does not sign its own acceptance.

## Owned paths

- `tools/local-analysis/`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a386/`

## Proof to read

- `linux/smoke/smoke.exit` is 0. Pre-edit rustc 1.94.0 probe.
- `linux/test/test.exit` is 0. Results: 7 library, 0 binary, 6 API, 3 CLI. `linux/test/clippy.exit` is 1 because the component is absent. `linux/test/docker-run.exit` is 1 for that Clippy invocation after the tests passed. HostConfig id `e12f3f650045e42b2e16d55c5c2d4a61bfb155c784b31a732fcc76ef28dc9556`.
- `linux/chain/chain.exit` and `linux/chain/docker-run.exit` are 0. Every file in `linux/chain/phases/` is 0.
- `linux/chain/analysis-proof.txt` is `ok`. `linux/chain/analysis-exits.txt` is `amount=0 wide=0 partial=1 cancel=1 parquet=3 xlsx=3 engine=3`.
- Chart: `linux/chain/out/analysis/analysis.svg`. Wide chart: `linux/chain/out/analysis-wide/analysis.svg`.
- Amount CSV SHA-256 `2edb40e84bc45e0391474c745933839d410282614c337b164ce66f53476a9a8a`. Manifest query id `q_49b26b99a4752eca`.
- Fresh stdout SHA-256 `15ba2ac4a9ac5e86218741a119cb944e0098f88137c4292eb6aceeed2f5b4001` (1527 bytes). Export CSV SHA-256 `f5b287fff0d197d55a1d67893178ff7e68ed5a1338830d24ea8fe0f11db2ca3d`.
- `pg/up.exit` is 0. Version 17.10. No published port. Memory 1 GiB. One CPU.
- `REPORT.md` and `FINDINGS.md`.

## Not repeated

Producer unit tests and producer Clippy from the a340 record. This chain built the bins and ran `a327_live::live_numeric_proof` once. See `linux/chain/producer-units-not-repeated.txt`.

## After the reports

`cleanup.txt`, `source-inventory.txt`, `SHA256SUMS`, and `SHA256SUMS.sha256` are written after this handoff. `SHA256SUMS` lists the evidence directory and skips itself. `SHA256SUMS.sha256` is the digest of that list. The git commit that records the owned paths is not inside the manifest.

## Cleanup

Own container `oo-a386-pg`, volumes `oo-a386-eef831aa-cargo`, `oo-a386-eef831aa-target`, and `oo-a386-eef831aa-pgdata`, network `oo-a386-eef831aa-net`, and `/tmp/oo-a386-eef831aa-dba4-4af2-a78b-541bc9e0a386` are removed after the proof files are on disk. `oo-a386-chain` was removed by `launch.sh`. No prune. Other owners are not touched.

Root `Cargo.toml` and root `Cargo.lock` were not edited.
