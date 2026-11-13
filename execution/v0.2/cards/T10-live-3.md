## Card T10-live-3 — measure live T10 at a sustainable rate (S6-H-02; D24)

Branch `v0.2/t10` (this card is committed on the branch; never delete it). Your last run showed burst calibration
overstates a sustainable open-loop rate: every ten-minute baseline hit `scheduler_queue_full`. The harness now runs
64 open-loop workers and, after burst calibration, `settle()` runs 60 s sustained trials stepping the rate down 30%
until error-free; the settled rate is the foreground rate for all phases. Do not change that method except to fix a
defect; if a ten-minute baseline still errors, lower the rate once more by 30% and record why.

Unrelated containers on the host are idle and must never be stopped, inspected beyond `docker stats`, or modified.
Evidence must never contain their names (the harness records count and peak CPU only). Never write unrelated container
or customer names into any file.

Then follow steps 1–5 of `execution/v0.2/cards/T10-live.md` (Oracle 23, MySQL 8.4, PostgreSQL 17; all phases; both
C5 policies; mitigations in order only for phases over the D24 budget, each with tests and a T1/T2 correctness re-run
for that engine; fill docs/guide and docs/database-support "Production impact" with measured numbers).
Commit as the owner, no agent trailers, trailer `Plan: S6-H-02`.
Report: per engine × phase table (baseline vs phase p50/p95/p99, throughput, errors, verdict, side-effect peaks,
duration), mitigations with before/after, commits, tests, residuals with operator guidance.
