## Card G-S5f — lane G: dialogue rows for the remaining P0 versions

TODO: S5-G-01. Rebase first. Commit trailers: `Plan:` only — no Co-Authored-By.
1. Emit T5/T6 verdict rows for the earlier runs that predate verdict rows (PostgreSQL 17.11 20/20
   `g-s5c-e309-pg-20261105`, Oracle 23.5 20/20 `g-s5c-e309-oracle-20261105`, MySQL 8.4.7 19/20
   `g-s5d-e312-mysql-20261105`) only if their raw evidence is still present and hash-verifiable; otherwise rerun them.
2. Rerun T6 (one fresh run each, standing on its own) on every P0 version without a 20/20 run: MySQL 8.0.44, 8.4.7,
   9.5.0; PostgreSQL 13.23, 14.24, 15.19, 16.15, 18.6; plus Oracle 19.19 (new, enterprise 19c pinned by lane E19).
   Model provider errors were frequent earlier: before each run, probe the model egress with a one-token call and wait
   (up to 30 min) until three consecutive probes succeed; a run that fails only on UPSTREAM_TIMEOUT/protocol errors may
   be repeated once as a separate run. Investigate MySQL 9.5 Q20 (a real wrong answer last time): diagnose and name the
   owning component with evidence.
3. Merge rows, regenerate the matrix.
