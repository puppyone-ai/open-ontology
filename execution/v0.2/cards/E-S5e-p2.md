## Card E-S5e — lane E: Db2 and DM8 (owner-authorised), T0–T1

TODO: S5-E-03 (plan exit T0–T1). Rebase on v0.2/product-loops first. Disk is no longer the constraint (≈490 GB free
after an owner-authorised cleanup). The owner explicitly accepted the DM8 license for this testing (decisions.md,
E340) — you may accept the DM8 installer's license prompt for the package you already downloaded (checksum recorded in
testing/engines/db2/S5-SUMMARY.md), and nothing else. KingbaseES stays out of scope (owner is obtaining it).
1. Db2 11.5.9.0 (`icr.io/db2_community/db2`, amd64, privileged is required by that image — allowed for this one
   container; never touch binfmt/sysctl): SourceEngine snapshot + C5 (feature-gated module, own oo-load module + one-line
   dispatch, oo-test matrix entry with explicit verdict rows via tools/oo-test/verdicts.py), T0, T1 (Agent DB comparison,
   concurrent writer), T8. Driver: cargo deny + cargo audit, report.
2. DM8: build a container image from the downloaded package (Dockerfile under testing/engines/dm8/, image built locally,
   not pushed), then the same: snapshot + C5, T0, T1, T8. If DM8's wire protocol needs a proprietary client library,
   use the thick-worker pattern (as for Oracle 11) and say so.
Build prerequisites first (dispatch deletes Cargo targets after a run).
