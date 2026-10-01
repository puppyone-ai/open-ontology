# oo-datagen

Standalone generator for the OO-Logistics xs and s datasets. It writes synthetic MySQL ERP and PostgreSQL TMS extracts, a hidden semantic truth directory, SQL and prompt-attack fixtures, and role claims. It does not talk to a database, and it is not a product driver.

The package is its own Cargo workspace (`resolver = "3"`, edition 2024, `rust-version = 1.94.0`). It path-depends on `oo-core` and is not a root workspace member. A later HARNESS change is the only integration that should add it to the root member list and the root lock. Until then, every command uses this manifest:

```sh
cargo test --manifest-path bench/datagen/Cargo.toml --locked
cargo clippy --manifest-path bench/datagen/Cargo.toml --locked --all-targets -- -D warnings
cargo run --manifest-path bench/datagen/Cargo.toml --locked --release -- --help
```

Declared MSRV is 1.94.0. The runs recorded for SP00-W04 used rustc 1.98.1 on aarch64-apple-darwin. Toolchain 1.94 was not installed, so the MSRV was not executed.

## Commands

There is no default seed and no default output directory.

```sh
oo-datagen help
oo-datagen version
oo-datagen generate --scale xs|s --seed UINT --output DIR [--scope DIR] \
  [--if-exists fail|check|resume] [--smell NAME=off]... [--inject-stop-after N]
oo-datagen verify --dataset DIR
oo-datagen export-sources --dataset DIR --output DIR [--scope DIR]
oo-datagen list schema
oo-datagen list fixtures
```

`generate` requires a new directory. `fail` (the default) errors if that directory exists. `check` verifies an existing dataset and does not rewrite it. `resume` deletes only a stage directory that carries this program's mark, and only when the destination itself is absent. Scales `m` and `l` print an error that names SP12 and create nothing.

Output is staged in a sibling `.{name}.oo-datagen-stage` and promoted by rename. A failure removes that stage only when the mark file is the exact text `OO-DATAGEN-STAGE/v1`. The destination is never removed. The scope is followed if it is a symlink (`/tmp` on macOS resolves to `/private/tmp`, and that resolved prefix is not treated as the path component `private`). The destination leaf must not already be a symlink. The check uses the path that was typed. Exact components `secret`, `secrets`, `private`, `credentials`, `customer-data`, `customer`, `customers`, `.ssh`, `.aws`, and `.gnupg` are refused, as is `..` and any component that contains an ASCII control character (`0x00`–`0x1F` or `0x7F`). `generate`, `verify`, and `export-sources` share that rule.

The option grammar rejects a positional token, a repeated single-value flag, and an unknown flag. `--smell` may be repeated. `help` and `version` reject trailing tokens with exit 2.

`--inject-stop-after N` is the cancellation hook. The stage is removed and the destination is not created.

`export-sources` copies `source-inputs/` only, after `verify` succeeds. `verify` on that export fails closed because the truth counts are absent.

## Source and truth

| Root | Who may read it | What is in it |
| --- | --- | --- |
| `source-inputs/` | default consumer, and later a product Pack | DDL, catalogs, CSV, load manifests, untrusted comments, dirty side files, encoding bytes, workload log |
| `hidden-truth/` | benchmark scoring, not a Pack | semantics, relationships, enums, entity clusters, metrics, calendar, expected counts |
| `attacks/` | benchmark scoring | SQL fixtures and one prompt-injection fixture. Logical allow/deny, not a runtime verdict |
| `authorization/` | benchmark scoring | role claims, column policy, expected allow/deny |

`dataset-manifest.json` records protocol `oo-datagen/v2`, seed, partition, `as_of` (`2026-10-01T00:00:00+08:00`), smell flags, file hashes, and visibility. The tree hash covers every file except the manifest and its `.sha256` sidecar. The same protocol and seed rewrite the same bytes. Another seed changes the partition and the ids. Under v2 the five carrier flags change the calendar, money columns, and workload log described below.

Truth text is not written into source comments. Source comments are a separate untrusted file and are not applied. The marker `TRUTH-ONLY-MARKER-9C2E` lives under `hidden-truth/`. A source file that contains it fails `verify`. Public truth in this benchmark tree is still not a product Pack: the manifest sets `product_pack` to false.

## Schema

Eighteen business tables. No foreign keys. Names are the legacy physical names.

| Dialect | Table | Business | Primary key |
| --- | --- | --- | --- |
| MySQL ERP | `kh` | 客户 | `khid` |
| MySQL ERP | `kh_bm` | 别名 | `khid`, `bmxh` |
| MySQL ERP | `ht` | 合同 | `htid`, `bb` |
| MySQL ERP | `dd` | 订单 | `ddid` |
| MySQL ERP | `ys` | 应收 | `ysid` |
| MySQL ERP | `sk` | 收款 | `skid` |
| MySQL ERP | `tk` | 退款 | `tkid` |
| MySQL ERP | `fp` | 发票 | `fpid` |
| MySQL ERP | `zd` | 字典 | `zdlx`, `zddm`, `yxqq` |
| MySQL ERP | `yh` | 用户 | `yhid` |
| PostgreSQL TMS | `yd` | 运单 | `ydh` |
| PostgreSQL TMS | `yd_mx` | 明细 | `ydh`, `hh` |
| PostgreSQL TMS | `gj_YYYYMM` | 轨迹 | none. Monthly shards |
| PostgreSQL TMS | `cl` | 车辆 | `cph` |
| PostgreSQL TMS | `sj` | 司机 | `sjid` |
| PostgreSQL TMS | `xl` | 线路 | `xlbh` |
| PostgreSQL TMS | `ck` | 仓库 | `ckid` |
| PostgreSQL TMS | `kc` | 库存 | `ckid`, `sku`, `yf` |

`source-inputs/workload/txn_log.csv` has no primary key. It is the seeded insert/update/delete log (table, txn, sequence, epoch, position). Position is `ordinal * 100 + seq` in epoch 1. It is not a binlog position and not a WAL LSN. `oo-core`'s position codec is a later phase.

CSV null is the unquoted `\N`. Text fields are quoted. With `cents` on, money columns are integer fen (`17550`, SQL `bigint`). With `cents` off, the same columns are exact scale-2 yuan (`175.50`): MySQL `decimal(20,2)` and PostgreSQL `numeric(20,2)`. `hlje` stays `decimal(40,12)`. `kc.sl` stays an integer. A quantity such as `10.50` in `ext_json` is unchanged. Main tables keep the declared primary keys, including `ys.ysid`, `cl.cph`, `xl.xlbh`, and `kc (ckid, sku, yf)`. Sharded `gj_*` has no primary key. `ht` stays `(htid, bb)`. There are no foreign keys. Broken enum, duplicate key, and NUL bytes are side files under `dirty/` and are omitted from `load.json`.

`verify` parses each `CREATE TABLE` in the generator's dialect and compares that table's columns, types, nullability, and primary key with the catalog declaration and with `catalog.json`. A comment that names a key on another table does not satisfy the key. MySQL `enum`, `set`, and `unsigned` stay distinct from the PostgreSQL types.

Cross-database names disagree on purpose: `kh.khid` versus `yd.kh_dm`, `dd.ddid` versus `yd.erp_ddh`, `dd.je_fen` versus `yd.sfje`.

Names are synthetic: 浣熊市警察局 and its aliases, 北港钢铁一厂 and 北港钢铁贸易 (similar, not merged). There are no customer brands and no real contact values. Phone and id canaries are the tokens `CANARY-PII-PHONE-0000` and `CANARY-PII-SFZH-0000`.

### Counts with every smell on

The largest table is the logical track (`gj`).

| | xs | s |
| --- | --- | --- |
| track rows | 10 000 | 100 000 |
| customers | 200 | 2 000 |
| orders, waybills, receivables | 2 000 | 20 000 |
| waybill lines | 6 000 | 60 000 |
| aliases | 202 | 2 002 |
| receipts | 1 802 | 18 002 |
| invoices | 1 501 | 15 001 |
| refunds | 100 | 1 000 |
| month-end updates | 1 000 | 10 000 |
| txn log rows, both transaction flags on | 9 100 | 91 000 |
| txn log rows, `multi_table_txn` off | 3 100 | 31 000 |
| txn log rows, `month_end_txn` off | 8 100 | 81 000 |
| txn log rows, both transaction flags off | 2 100 | 21 000 |
| vehicles / drivers | 50 / 40 | 500 / 400 |

`m` is 5 000 000 track rows and `l` is 50 000 000. The scale enum records those sizes. `generate` refuses to run them.

Shard files are `gj_202601` through `gj_202609`, plus `gj_202611` when the overlap smell is on. The November file then holds one row, and the other nine files make up the rest. `xl` stays at 24 rows, under the 1 000-row line used by the R9 fixture. The January shard is over that line.

## Roles and attacks

Roles: `dispatcher`, `finance_east`, `finance_west`, `manager_east`, `warehouse`, `auditor`.

`finance_east` sees east customers with `csbz=0`, including soft-deleted rows. `manager_east` also requires `scbz=0`. `dispatcher` sees `yd`, `yd_mx`, `gj`, `cl`, `sj`, `xl`, `ck`, `kh`, `kh_bm`, and `zd`, and does not see `ht`, `dd`, `ys`, `sk`, `tk`, `fp`, `yh`, or `kc`. `warehouse` sees `ck`, `kc`, `yd`, `yd_mx`, and `zd` for warehouse `CK01`, and does not see `kh`, `kh_bm`, `ht`, `dd`, `ys`, `sk`, `tk`, `fp`, `gj`, `cl`, `sj`, `xl`, or `yh`. `auditor` may read every business table and may not read raw PII columns. For each role, allow and deny are disjoint and their union is the eighteen tables. These are expected decisions for a later gateway. This crate does not enforce them and does not claim a security pass. `runtime_executed` stays false.

SQL fixtures cover writes, multi-statement text, writable CTE, dangerous functions, role claims, settings, cross-role reads, raw versus masked columns, star expansion, unfiltered star on a large shard, catalog and schema escape, union and lateral, comment tricks, locks, recursive shape, a view/subquery row-filter bypass, a cartesian product, cancel, and legal controls. The view case is `SQL-VIEW-SUBQUERY` (`view_or_subquery_row_filter_bypass`). The cartesian case is `SQL-CARTESIAN` (`cartesian_product`). The positive join is `SQL-LEGAL-JOIN` (`bounded_equijoin`). Each case has `expected_decision` and `expected_reason`. The file does not contain a model verdict and does not record an executed gateway test. The prompt fixture is the synthetic canary, not a secret.

## Determinism

`oo-splitmix64/v1` is SplitMix64 with the published Java `SplittableRandom` constants. Known vectors live in `src/prng.rs` and were checked against an independent mix. Domain streams are `domain_rng(seed, domain, index)`. The partition is the top 16 bits of the first output of `SplitMix64::new(seed)`. Ids are `{prefix}{partition:04X}{index:07}`.

Output order is index order. JSON object keys are sorted. Line endings are LF. `as_of` is the constant above. Nothing in the generated bytes comes from the wall clock, a UUID, or a compressor.

`verify` parses the manifest, checks the sidecar, recomputes file hashes, and checks relationships from the files: waybill amounts against orders, the HX invoice against calendar and closing receipts, refund links, txn position arithmetic, USCC checksums, DST resolution through `oo_core::tz::resolve_local`, declared primary keys, parsed DDL, and the smell flags. Shared constants in `facts.rs` are the seeded spec. The count formulas and the calendar document are duplicated in `verify.rs` so a bug in `plan.rs` or `truth.rs` cannot grade itself. Off states for the five carrier flags are in `fixtures/coverage.md`.

## Dependencies

`oo-core` is a path dependency. The generator calls the approved value APIs: `ExactDecimal`, `parse_json`, `validate_date`, `tz::resolve_local`, `TZDB_VERSION`, `CANONICAL_VALUE_VERSION`, and `TYPE_POLICY_V1`.

`sha2` `=0.10.9` with default features off and `std` on (so `sha2-asm` stays off) is the same pin `oo-core` already uses. It is here because `oo_core::hash_bytes` hashes a buffer, and the generator hashes files as it writes them. The crate was published on 2025-04-30. No other direct crate was added. `crypto-common` and `generic-array` in this lock are pinned to the versions in the root lock (`0.1.6` and `0.14.9`).

`js-sys` 0.3.105 and `wasm-bindgen` 0.2.128, together with `wasm-bindgen-macro`, `wasm-bindgen-macro-support`, and `wasm-bindgen-shared` 0.2.128, are pinned to the same releases as the root lock. Those five were published on 2026-09-04. `js-sys` 0.3.106 and the `wasm-bindgen` 0.2.129 family were published on 2026-09-25. On 2026-10-01 those September 25 releases were under the 604800-second floor, so this lock keeps the September 4 releases. The five crates are reached only through getrandom's wasm32 dependencies. A wasm32 build of this lock was not run and is not a supported target.

## What SP12 still owns

This slice does not finish SP12-W01 or AT-EVAL-01.

- The 300-question set, gold labels, and the dual-annotation sample are not generated here.
- Metric D, its sample size, and human review are not measured.
- Scales `m` and `l` are not run. The writer is streaming, and that is the shape those runs need. It is not a claim that a 5 million or 50 million run happened.
- There is no scorer, no eval runner, and no product `oo-eval`.
- `CorePositionCodec` is not used. The txn log is an independent epoch/sequence/position.

Status of this slice: in review. The contract test covers two xs generations, two s generations, a different seed, mutation rejects, export isolation, smell-off counts, path refusal, and the SP12 refusal. That is not an acceptance pass of AT-EVAL-01.
