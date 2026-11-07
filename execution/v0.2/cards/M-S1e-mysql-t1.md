## Card M-S1e — lane M: full MySQL T1/T8 after the type fix

TODO: S1-C-01, S1-C-02, S1-M-01. Rebase on v0.2/product-loops first: c1ebe3524 captures SET as text and transcodes latin1
(your two proposals, applied by the coordinator). Re-run the full MySQL T1 matrix with T8 through oo-load and
tools/oo-test/t1.py on 5.7.44 (amd64), 8.0.44, 8.4.7, 9.5.0 at full scale (2,000,000-row table, concurrent writer,
corrective re-read, no-rewrite resume, T8 reader peak / duty / target bytes). Fix remaining mapping gaps in
crates/oo-connectors/src/mysql/map.rs if any (granted), with unit tests; commit as you go. Report per version.
Lane E edits only the Oracle hook in t1.py; keep your edits to the generic parts and MySQL.
