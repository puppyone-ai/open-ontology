## Card H-S5d — lane H: corrected T1/T8 at full scale on every P0 version

TODO: S5-H-01. Rebase first. Commit trailers: `Plan:` only — no Co-Authored-By. The blockers your corrected judge found
are fixed and integrated: keyless tables reconcile as whole-table C5 units (lane C) and Oracle C5 digests stream LOB
columns (lane C). Run `oo-test matrix --suites T0,T1,T8 --hook T0=tools/oo-test/t0.py --hook T1=tools/oo-test/t1.py`
at full scale (2M events, 200 tables) on MySQL 5.7.44, 8.0.44, 8.4.7, 9.5.0; PostgreSQL 13.23, 14.24, 15.19, 16.15,
17.11, 18.6; Oracle 23.5, 21.3, 11.2, 19.19 — sequentially, removing each target before the next. Merge rows
(`--merge-only`), commit them to execution/v0.2/evidence-index.json (granted), regenerate the matrix (`--check`,
`--self-test`), and report the table. Lane T is concurrently running T7 Oracle reruns with label `oo.lane=t`; use
`oo.lane=h` and do not touch its containers.
