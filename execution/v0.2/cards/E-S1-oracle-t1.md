## Card E-S1 — lane E: port the Oracle T1 hook and run Oracle T1/T8 from the shared suite

TODO: S1-E-01. Rebase on v0.2/product-loops first. Your t1-hook-integration.patch no longer applies: lane M added T8
sampling to tools/oo-test/t1.py. Port your Oracle hook onto the current t1.py (Oracle-specific parts only; you are
granted tools/oo-test/t1.py for that), remove the .patch file from your tree, then run
`python3 tools/oo-test/t1.py --engine oracle --versions 23,21,11 --run-id ...` from YOUR branch (11g through the thick
worker): full scale, concurrent writer, corrective re-read, no-rewrite resume, and T8 (reader sessions peak from
v$session, read duty, target bytes). Commit the hook as its own commit so the coordinator can integrate it ahead of your
S2 commits. Report per version.
