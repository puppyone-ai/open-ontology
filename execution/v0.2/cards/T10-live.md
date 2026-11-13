## Card T10-live — measure production impact live and mitigate (S6-H-02; D24)

Branch `v0.2/t10` at `1eb41620a7f5259ba2db9c41abc9b87372357ddb`. The host is now quiet (no other lane runs). Commit as the owner, no agent trailers,
trailer `Plan: S6-H-02`. Read `execution/v0.2/cards/T10-production-impact.md` (method, budget D24, mitigation order)
and your harness `tools/oo-test/t10.py` with its command list.

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
