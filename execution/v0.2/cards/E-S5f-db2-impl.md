## Card E-S5f — lane E: Db2 SourceEngine implementation; DM8 on arm64

TODO: S5-E-03. Rebase first. Your Db2 fixture now starts and answers SQL (S5E-SUMMARY). This card is implementation —
do not stop after setup work; commit after each step:
1. Db2 SourceEngine (feature `db2`, module crates/oo-connectors/src/db2/): choose the driver path first (pure-Rust DRDA
   crate if one passes cargo deny/audit; otherwise the thick-worker pattern used for Oracle 11 with IBM's CLI driver
   inside the worker image — no native driver in the default artefact). probe/capabilities, source-ordered keyset
   chunks with ReadBudget, digest_range for C5, watermark. oo-load module + one-line dispatch. Unit tests.
2. Db2 11.5.9.0 T0, T1 (Agent DB comparison with concurrent writer), T8 with explicit verdict rows.
3. DM8: the x86_64 package dies under Rosetta (`Illegal instruction` in disql). Check whether Dameng publishes an
   aarch64 (Kylin/UOS/Linux arm64) DM8 package on the same public download page covered by the owner's acceptance
   (E340); if so, download it (record URL, checksum) and build an arm64 image; otherwise record the downgrade (needs a
   real x86 host, D1) and stop DM8.
