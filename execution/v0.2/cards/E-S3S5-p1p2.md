## Card E-S3S5 — lane E: P1 SQL Server, P2 Db2 / Dameng / KingbaseES

TODO: S3-E-02, S5-E-03. Lane label: `e`. Spec: REQ-ENG-01/04. Builds on your Oracle work on your own lane branch (it may not be on the integration branch yet — do not stop for that; rebase on v0.2/product-loops first).
Read docs/database-support/04 (SQL Server, Db2, domestic DB sections), tools/mssql-preview.
Deliver:
1. S3-E-02 SQL Server 2019 / 2022 / 2025 (`mcr.microsoft.com/mssql/server:<ver>-<CU>`, amd64 under Rosetta;
   2025 needs CU1+). SourceEngine (tiberius path per docs), VersionRule table, snapshot + Change Tracking
   (C2) / CDC (C1) graded by real probe with DBA statements (enabling CT/CDC is a DBA action, never done by
   the product), C5. T0–T1 per version (T2 where CT/CDC works). Converge tools/mssql-preview.
2. S5-E-03 Db2 LUW 11.5 / 12.x (`icr.io/db2_community/db2`, amd64; privileged container required by the
   image — label it and remove it after): snapshot T0–T1 via the driver path in docs (ODBC/CLI worker
   isolation per REQ-SRC-11; if no admissible driver exists, record Unsupported with the reason). Dameng DM8
   and KingbaseES: images are not on public registries; check the official download pages without logging
   in. If a download needs an account, record `blocked: needs human account` and stop for that engine (never
   register or sign in). If a public image is obtainable, T0–T1.
Write scope: crates/oo-connectors/src/{mssql,db2,dameng,kingbase}/**, tests, testing/engines/{sqlserver,db2,dameng,kingbase}/**,
tools/mssql-preview/**. Lock changes reported. Prefix `connectors/<family>`.
