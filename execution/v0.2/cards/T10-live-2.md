## Card T10-live-2 — resume live T10 after the quiet-host gate fix (S6-H-02; D24)

Branch `v0.2/t10` at `367761004`. Your previous run was blocked by the "no pre-existing containers" gate. The host carries
eight idle unrelated containers. NEVER stop, inspect beyond `docker stats`, or modify them. The gate now samples their CPU, refuses if not idle, and records them in the report; that is the accepted
quiet-host condition. Do NOT redo the C5 key fix or the docs skeleton already committed; fill the docs with measured numbers.

1. Build (`cargo build -p oo-cli --locked`; oo-load `--features mysql-binlog --target-dir target`). Run T10 in
   this order: Oracle 23, MySQL 8.4, PostgreSQL 17, all phases (baseline, initial, initial_duty100, c1, c5), source
   containers with fixed limits (e.g. 4 CPUs, 8 GB; record them), label oo.lane=t10, at most two DB containers.
2. Mitigation 1 (C5 periodic full read) is already in the tree; measure C5 both with the lite default
   (`full_read_every` 24, change-only transfer) and with `full_read_every 1` to show its effect.
3. For any phase over budget, implement the next mitigations from the card in order (adaptive throttle on source
   activity; short per-chunk transactions instead of one long snapshot, made consistent by the C5 correction, for
   PG xmin / MySQL history list; Oracle undo sizing guidance and C5-over-LogMiner fallback), each with unit tests and a
   correctness re-run (T1 and T2 for that engine at full scale), then re-measure. Keep defaults safe.
4. Write the operator guidance into docs/guide (new section "Production impact") and docs/database-support: measured
   numbers per engine, recommended settings, undo/vacuum/retention sizing, standby/replica option.
5. Exact-byte secret scan is not needed (no model use) but never print credentials. Remove all oo.lane=t10 resources.
Report: per engine × phase table (baseline vs phase p50/p95/p99, throughput, errors, verdict, side-effect peaks,
duration), mitigations with before/after, commits, tests, residuals with operator guidance.
