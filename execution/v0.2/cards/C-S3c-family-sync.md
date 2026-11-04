## Card C-S3c — lane C: rebase onto lane M's oo-load, then family sync and acceptance

TODO: S3-C-02, S4-C-03. S2 has exited (ledger E197); S3 items now integrate. Integrated since your last run: lane M's
oo-load sync (`log_sync::{mysql,oracle}`, `c5::run`, a `mysql_engine()` constructor, durable source epochs) and the
coordinator's `duration_micros → interval` load mapping (a342d058c), which fixes your `load_unsupported_type`.
1. Rebase on v0.2/product-loops. `tools/oo-load/src/main.rs` conflicts with your 6444b4c83: keep M's structure and
   make the family the constructor argument (`mysql_engine(family, …)`), so mysql/mariadb/tidb/oceanbase share one arm
   with M's `--sync`/`--c5` paths. Keep each commit building.
2. Granted for this card: `tools/oo-load/**` family arms. Wire MariaDB C1 (primary-only, `mysql-binlog` feature; GTID
   domain-aware source_epoch) and C5; TiDB and OceanBase C5 only (C1 refused with a named error). Unit tests per arm.
3. Acceptance through `oo-load`, with lane M's runner style (`tools/oo-test/t2_p0.py` — extend it, do not fork it):
   MariaDB 10.6/10.11/11.4/11.8 T0, T1 (Agent DB comparison incl. the typed table, concurrent writer), T2 (three C1 +
   three C5 cycles), T8; TiDB 7.5/8.5 and OceanBase 4.3.5.4 T1 + T2 C5 + T8.
