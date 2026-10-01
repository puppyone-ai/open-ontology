# Bad-smell coverage

Default generation turns every smell on. `--smell NAME=off` clears one. The count and file smells below change a row count or omit a file. `cents`, `fiscal_calendar`, `closing_period`, `multi_table_txn`, and `month_end_txn` keep the legacy columns and change the carrier bytes. The remaining smells keep the carrier row and replace the bad value with a clean one, so the file set stays stable.

`verify` recomputes counts from the scale and the smell flags, then reads the CSV, DDL, catalog, calendar, and workload files. It does not call the row writers. Smell flags in `hidden-truth/anomalies.json` are checked last. A manifest-only flag flip fails on the carrier, and an anomalies-only flip fails as `anomaly flag drift`.

Protocol `oo-datagen/v2` is the interpretation of these flags. Two runs of this protocol with the same seed write the same bytes.

## Count or file changes

| Smell | On | Off |
| --- | --- | --- |
| `alias_duplicate_customer` | aliases = customers + 2, including 浣熊市警察局后勤中心 sharing the valid tax id | aliases = customers, that name is absent |
| `metric_invoice_vs_receipt` | order 0 has a March receipt and an April receipt. The April `jzbz` follows `closing_period` | order 0 has one receipt on the invoice date with `jzbz=0` |
| `dst_overlap` | `gj_202611.csv` exists and holds the one overlap row | that file is absent; January–September cover the track table |
| `code_meaning_by_time` | dictionary keeps the `khzt=2` row whose era starts `2000-01-01` | that dictionary row is omitted |

## Value smells

| Smell | Where it shows when on |
| --- | --- |
| `tinyint_012` | customer `zt` takes 0, 1, and 2 |
| `zero_date` | `qdrq_raw` is the text `0000-00-00` on 合成客户-零日期. The column is `char(10)`, not a date type |
| `text_date` | waybill `fhrq` holds `2026/1/5` and `2026-02-30` |
| `unsigned64_above_i64` | `lsid` is `18446744073709551615` |
| `high_precision_numeric` | `hlje` is `1234567890123456789.123456789012` |
| `cents` | money columns are integer fen (`bigint`, lexical digits such as `17550`). Off keeps the same column names and writes exact scale-2 yuan (`175.50`): MySQL `decimal(20,2)`, PostgreSQL `numeric(20,2)`. `hlje` and `kc.sl` do not change. Truth metrics use the same representation |
| `multi_currency` | USD and EUR customers exist beside CNY |
| `enum_set` | `khbq` is `vip,delay`. Invalid enum `nope` is only in `dirty/invalid-enum.csv` |
| `emoji` | 🚚 in a customer name and a track message |
| `gbk_bytes` / `gb18030_bytes` | the binary fixtures. Off writes a 0-byte file at the same path and `included: false` |
| `timezone_mismatch` | January track row: local `2026-01-15 08:00:00`, zone `UTC`, instant text `2026-01-15T00:00:00Z` |
| `dst_gap` | `2026-03-08 02:30:00` America/New_York |
| `soft_delete` | 合成客户-已软删除 has `scbz=1` |
| `test_customer` | TEST-调度演练客户 has `csbz=1` |
| `dictionary_gap` | `khfl=7` and waybill `zt=99` are absent from `zd` |
| `tax_id_null` | 合成客户-税号空 |
| `tax_id_placeholder` | 18 zeros. The check digit is `0`, and the registration department is not 1, 5, 9, or Y, so it is not a valid USCC |
| `tax_id_15` | `310101234567890` |
| `tax_id_bad_checksum` | `91350100M000100Y44` |
| `similar_not_merge` | 北港钢铁一厂 and 北港钢铁贸易 stay separate rows |
| `fiscal_calendar` | fiscal year starts in April: FY2026 is `2025-04-01` through `2026-03-31`, March is `FY2026-P12`, April is `FY2027-P1`. Off uses a calendar year: `2026-01-01` through `2026-12-31`, March `FY2026-P03`, April `FY2026-P04`. `kjq` stays `YYYY-MM` |
| `closing_period` | `closing_lag_days` is 5. With the receipt split on, the April HX receipt is `jzbz=1` and the March closing total is 17550 fen. Off sets the lag to 0. With the split still on, that April receipt remains and is `jzbz=0`, and the March closing is 7550 fen |
| `multi_table_txn` | each order is one shipment group of four inserts (`dd`, `ys`, `yd`, `yd_mx`) in `workload/txn_log.csv`. Off writes one `dd` insert per order, note `order`. Position stays `ordinal * 100 + seq` in epoch 1. It is not a binlog position and not a WAL LSN |
| `month_end_txn` | one `ME` batch updates `track/10` receivable rows. Off omits that batch, so `month_end` is 0. With both transaction flags on, xs `txn_rows` is 9100 and s is 91000. With both off, the log is one row per order plus the refund deletes |
| `dangling_ref` | last waybill `erp_ddh` is `NO-SUCH-ORDER` |
| `amount_mismatch` | order `REF-MISMATCH-001` line sum is header minus 1 fen |
| `prompt_injection` | synthetic canary `OO-CANARY-PROMPT-7F3A` on 合成客户-备注注入 and in `comments/untrusted.sql` |

Dirty side files are always present and are not listed in `load.json`: invalid enum, duplicate primary key, and a NUL byte.

## What this matrix is not

It does not record a pass of AT-EVAL-01, a 300-question set, a human review of metric D, or a run at 5 million or 50 million track rows. Those remain SP12.
