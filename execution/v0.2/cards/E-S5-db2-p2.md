## Card E-S5 — lane E: Db2 snapshot; DM / KingbaseES if images are obtainable

TODO: S5-E-03 (plan exit T0–T1). Works ahead of the S4 exit; integration after it. Rebase on v0.2/product-loops first.
Follow your SQL Server structure (connector module behind a feature, own oo-load module + one-line dispatch, oo-test
matrix entry, pins in lane H's format). Lane N (native) is editing tools/oo-test T3/T4 files and lane F adds OLAP
arms: keep your edits to new files plus one-line registrations.
1. Db2: `icr.io/db2_community/db2` (amd64, emulated; it needs privileged mode and ~4 GB RAM — run alone, check
   `df -h /` ≥ 25 GB before pulling). SourceEngine snapshot with source-ordered chunks, ReadBudget, C5 digests;
   T0 and T1 (Agent DB comparison, concurrent writer) and T8 on Db2 11.5 (latest community patch). A driver crate needs
   cargo deny + cargo audit (report); if no acceptable pure-Rust or ODBC-free path exists, use the thick-worker pattern
   lane E used for Oracle 11 and say so.
2. DM8 and KingbaseES (D5): only images or packages downloadable without registration. If one needs registration or a
   license key, stop for that engine, record exactly what is required (URL, terms), and do not register or accept terms.
   If obtainable: snapshot + C5 + T0/T1.
