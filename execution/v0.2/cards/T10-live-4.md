## Card T10-live-4 — measure live T10 with bounded-backlog open loop (S6-H-02; D24)

Branch `v0.2/t10` (this card is committed on the branch; never delete it). Your last run proved the harness pass rule,
not the databases, failed: a 2048-slot client queue turned sub-second DB stalls into "errors". Coordinator change
(already committed): the open-loop queue holds `BACKLOG_SECONDS` (30) of arrivals, since latency measured from the
intended start already shows queueing in p95/p99; only DB errors and backlog beyond 30 s count as errors. Oracle runs
32 foreground connections (ORA-12516 above that on Oracle Free); others 64. Calibration then settle() as before.
Do not change the method except to fix a defect. If Oracle still refuses sessions at 32, raise the fixture's
`processes`/`sessions` init parameters on the T10 source container only, and record it.

Unrelated containers on the host are idle and must never be stopped, inspected beyond `docker stats`, or modified;
never write their names anywhere.

Then follow steps 1–5 of `execution/v0.2/cards/T10-live.md` (Oracle 23, MySQL 8.4, PostgreSQL 17; all phases; both
C5 policies; mitigations in order only for phases over the D24 budget, each with tests and a T1/T2 correctness re-run
for that engine; fill both "Production impact" docs with measured numbers). Prioritize finishing all phases for all
three engines over extra retries. Commit as the owner, no agent trailers, trailer `Plan: S6-H-02`.
Report: per engine × phase table (baseline vs phase p50/p95/p99, throughput, errors, verdict, side-effect peaks,
duration), mitigations with before/after, commits, tests, residuals with operator guidance.
