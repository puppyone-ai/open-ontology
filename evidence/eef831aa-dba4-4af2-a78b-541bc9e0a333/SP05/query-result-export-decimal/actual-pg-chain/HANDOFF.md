# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a333`. Task a333, resumed after a340 APPROVE_LIMITED on producer `21cc96241a841b47dbbcb116549030e38a643ba0`.

## Verdict

Live chain: the compiled `query-pg` stdout from this private PostgreSQL was exported by the frozen exporter and matched the hand gold for the six-row decimal fixture, the one-row truncation, and the formula-header boundary.

Exporter commit `62b4afecdd377fa94310f46467c48a01e61d14db`. Producer commit `21cc96241a841b47dbbcb116549030e38a643ba0`. No product edit in this resume. No push.

This handoff does not accept the parent goal and does not release. The parent `REPORT.md` still records the earlier unit-only withhold. That file was left in place.

## Owned paths

Evidence added under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a333/SP05/query-result-export-decimal/actual-pg-chain/`. `tools/query-result-export` is unchanged from `62b4afec`.

## Proof to read

- `linux/chain/steps/amounts.stdout` — compiled six-row stdout, 1527 bytes, SHA-256 `498ff77d32d24b18f556694e8ea73d398a5a622e79907861bf9b68a584c97b5f`
- `linux/chain/steps/partial.stdout` — same binary, `max_rows` 1, 616 bytes, SHA-256 `bf720bfee030ea1f0548693f8ff9e04c2ad68e652b3e1be406045bb628d7a4b4`
- `linux/chain/out/complete/export.csv` matches `gold/amounts.csv`
- `linux/chain/out/partial/export.csv` matches `gold/partial.csv`
- `linux/chain/out/formula/export.csv` matches `gold/formula.csv`
- `pg/actual/source-rows.txt` and `pg/actual/target-rows.txt` are the same six `psql` lines

Phase exits in `linux/chain/phases/` are 0, including `gold-complete`, `gold-partial`, and `gold-formula`. Exporter partial exit is 1, which is the contract exit for an incomplete export. `linux/chain/chain.exit` and `docker-run.exit` are 0.

## Not repeated

a340's producer `--lib`, `decimal_json`, `absent_row_filter_null`, and Clippy. This run built the bins and ran the live proof. See `linux/chain/producer-units-not-repeated.txt`.

## After the manifest

`SHA256SUMS` is written by a later inventory container over this directory only. It does not replace the parent directory's `SHA256SUMS`. Files written after that hash are not in the new manifest: `linux/inventory/inventory.exit`, `docker-run.exit`, `hostconfig.txt`, `console.log`, `mounts.txt`, `tmpfs.txt`, `container-removed.txt`, `volume-removed.txt`, and `cleanup.txt`.

## Cleanup

Own containers, volumes, network, and `/tmp/oo-a333-eef831aa-dba4-4af2-a78b-541bc9e0a333` are removed after the inventory. No prune. Other owners are not touched.
