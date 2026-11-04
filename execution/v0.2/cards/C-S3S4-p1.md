## Card C-S3S4 — lane C: P1 MySQL-compatible families (MariaDB, TiDB, OceanBase)

TODO: S3-C-02, S4-C-03. Lane label: `c`. Spec: REQ-ENG-01/04. Each product is its own `EngineFamily`
(D10: compatibility is never inherited). Builds on your MySQL work on your own lane branch (it may not be on the integration branch yet — do not stop for that; rebase on v0.2/product-loops first).
Read docs/database-support/04-database-profiles.md (MariaDB, TiDB, OceanBase sections), tools/mariadb-preview.

Deliver:
1. S3-C-02 MariaDB 10.6 / 10.11 / 11.4 / 11.8 (`mariadb:<ver>` arm64): own VersionRule table (MariaDB GTID
   domain-server-seq format, JSON = LONGTEXT alias, auth plugins, SHOW BINLOG STATUS naming), SourceEngine
   (reuse MySQL internals where behavior is identical and say where it is not), C1 via binlog where
   supported, C5. Converge tools/mariadb-preview into the product crate. T0–T2 per version.
2. S4-C-03 TiDB 7.5 / 8.5 (`pingcap/tidb:v<ver>` pinned, unistore single node is acceptable for fixtures;
   record that) and OceanBase CE 4.x MySQL mode (`oceanbase/oceanbase-ce`): snapshot + C5 only (TiDB has no
   MySQL replication protocol; TiCDC out of scope — capability Unsupported with reason; OceanBase binlog is
   synthetic — capability Unsupported/needs-setup with reason). T0–T2 (C5) per version. Watch memory:
   OceanBase needs several GB; run alone.
Write scope: crates/oo-connectors/src/{mysql,mariadb,tidb,oceanbase}/** , tests, testing/engines/{mariadb,tidb,oceanbase}/**
(not seed/), tools/mariadb-preview/**. Prefix `connectors/<family>`.
