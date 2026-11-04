## Card C-S3b — lane C: finish MariaDB, TiDB and OceanBase acceptance

TODO: S3-C-02, S4-C-03. Rebase on v0.2/product-loops first (it now has the aligned image pins, oo-load sync for
PostgreSQL, source_epoch accessors and lane H's audit/matrix harness). Your earlier run stopped on a host disk/Docker
failure; Docker and disk are healthy again (check `df -h /` yourself; stop starting containers below 20 GB free).
Granted for this card: the `crates/oo-connectors/src/lib.rs` re-export you requested
(`pub use mysql::{mariadb, oceanbase, tidb};`), as its own commit.
Deliver (keep going; commit as you go):
1. Rerun your strengthened live assertions on MariaDB 10.6/10.11/11.4/11.8 and TiDB 7.5/8.5.
2. MariaDB T0–T2 through `oo-load` (Agent DB comparison T1, C1 primary-only + C5 for T2) with T8, on all four versions.
   Engine dispatch in oo-load is lane M's: if oo-load needs a family entry, add the smallest arm with a unit test and name it.
3. TiDB snapshot + C5 through oo-load with T1/T8; OceanBase CE 4.3.5.4 snapshot + C5 to a pass, or a precise reason it
   cannot pass on this host (amd64 emulation, memory) with the evidence.
4. Migrate the remaining `tools/mysql-preview` family paths onto the connector, or state precisely what stays and why.
Pin the versions you use in `testing/infra/images.pins` only through lane H's format (one patch per major).
