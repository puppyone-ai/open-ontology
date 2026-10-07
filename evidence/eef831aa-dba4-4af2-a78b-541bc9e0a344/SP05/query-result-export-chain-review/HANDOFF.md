# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a344`. Independent review of frozen FILE-D04 / E44.

## Verdict

Child: **LIMITED**.

The compiled `query-pg` stdout from this private PostgreSQL was exported by the frozen exporter and matched the independent hand gold for the six-row decimal fixture, the one-row truncation, and the formula-header boundary. The original U+FEFF unit case passed again in the exporter smoke.

Exporter commit `62b4afecdd377fa94310f46467c48a01e61d14db`. Producer commit `21cc96241a841b47dbbcb116549030e38a643ba0`. No product edit. No push. No release.

Parent SP05-W06, AT-FILE-05, AT-SEC-03, xlsx, HTTP authentication, download authorization, retention, deletion, the reaper, publish, and release stay open.

## Owned path

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a344/SP05/query-result-export-chain-review/`.

## Proof to read

- `linux/smoke/smoke.exit` and `linux/smoke/docker-run.exit` are 0. Tests 22 + 3. FEFF unit ok. Clippy exit 0.
- `linux/chain/chain.exit` and `linux/chain/docker-run.exit` are 0. Phase exits under `linux/chain/phases/` are 0.
- `linux/chain/steps/amounts.stdout` — 1527 bytes, SHA-256 `20623e7dc575b618223ac383ceb1b2e9a9e3f9efc1695c58ea2373bcaff4b3eb`
- `linux/chain/steps/partial.stdout` — 616 bytes, SHA-256 `4045364a9f9527c9502bcf39ac0942f90f566c5f49883c8d116f9ef4f5214177`
- `linux/chain/out/complete/export.csv` matches `gold/amounts.csv`
- `linux/chain/out/partial/export.csv` matches `gold/partial.csv`
- `linux/chain/out/formula/export.csv` matches `gold/formula.csv`
- Exporter partial exit is 1, stderr `query-result-export: partial`
- `pg/actual/source-rows.txt` and `pg/actual/target-rows.txt` are the same six lines
- `linux/smoke/source-inventory.txt` lists the frozen exporter sources hashed in the smoke container

`origin/recipes.txt` records the a333 and a340 script origins and the byte-identical copies. `REPORT.md` and `FINDINGS.md` are the run record.

## Not repeated

a340's producer `--lib`, `decimal_json`, `absent_row_filter_null`, and Clippy. This run built the bins and ran the live proof. See `linux/chain/producer-units-not-repeated.txt`.

## After the manifest

`SHA256SUMS` is written by a later inventory container over this directory only. Files written after that hash are not in the manifest: `linux/inventory/inventory.exit`, `docker-run.exit`, `hostconfig.txt`, `console.log`, `mounts.txt`, `tmpfs.txt`, `container-removed.txt`, `volume-removed.txt`, and `cleanup.txt`.

## Cleanup

Own containers, volumes, network, and `/tmp/oo-a344-eef831aa-dba4-4af2-a78b-541bc9e0a344` are removed after the inventory. No prune. Other owners are not touched.

Root `Cargo.toml` and root `Cargo.lock` were not edited. Do not mark the ledger or the spec closed from this review.
