## Card C-S5k — lane C: C5 correction of keyless tables (product defect found by review M1)

TODO: S5-H-01, S5-C-04, S5-D-03, S5-E-04. Rebase on v0.2/product-loops first. Commit trailers: `Plan:` only — never
Co-Authored-By. Lane H's corrected T1 judge (`tools/oo-test/t1.py`, H-S5C-VALIDATION.md) now corrects a concurrent
snapshot with one product C5 cycle, and that exposed a product defect: for tables with no unique key the snapshot uses
physical ranges (PostgreSQL ctid on 14+, single whole-table read on 13; Oracle ROWID; check MySQL), but C5 then
fails — PG 14–16 `ctid_requires_exported_snapshot` (crates/oo-connectors/src/postgres/mod.rs:338), PG 13 `stale_chunk`
(mod.rs:219/613/730) — because C5 reuses baseline ranges (lane N's durable-range change) and physical ranges are not
durable across snapshots (docs/product/02 §… "物理分块只能在单个快照内使用").
Fix (smallest correct): for a keyless table, C5's durable unit is the WHOLE table — one range with a whole-table
content digest; when it differs, re-read the whole table inside one exported snapshot (physical chunks allowed within
that snapshot) and replace that table's generation atomically; idle cycles still zero writes. Apply to PostgreSQL
(13–18), MySQL, Oracle, SQL Server, MariaDB/TiDB/OceanBase, Db2/DM8/Kingbase wherever keyless snapshots use physical
or non-durable ranges. Unit tests per engine; a live regression with a keyless table under a concurrent writer on
PostgreSQL 13, 14, 17, MySQL 8.4, Oracle 23 through `tools/oo-test/t1.py` (the corrected judge) must pass.
Propose the doc text for docs/product/02 in your report.
