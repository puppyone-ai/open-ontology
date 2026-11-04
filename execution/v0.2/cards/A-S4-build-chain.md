## Card A-S4 — lane A: self-review TUI and the full `oo build` chain

TODO: S4-A-01, S4-A-02. S3 has exited (E207). Rebase on v0.2/product-loops first. Integrated since your last run: lane G's
ontology export (`oo ontology open/export`), thousand-table Builder work, SQL Server in doctor, lane M's oo-load sync.
Lane M is concurrently doing publication follow (release-coordinator, Gateway pointer switch, lite scheduled cycle):
do not edit `tools/release-coordinator/**`, `crates/oo-gateway/**` or `deploy/lite` scheduler/worker wiring; call
M's publication step through its CLI/library entry once integrated, or stub behind one function and name it.
Deliver (keep going; commit as you go):
1. S4-A-01 owner self-review TUI (ratatui): entities, relationships, metrics, sensitive-column classification; accept /
   edit / reject per item; G14 review inputs; writes the reviewed registry the publisher consumes; non-TTY `--review-file`
   gives the same result. Recorded-interaction test like your `oo init` test.
2. S4-A-02 `oo build` full chain: pull → Self-build → self-review → publish → Gateway → issue first user token →
   print the share prompt (lane B's `oo share-prompt`); `--resume` resumes after interruption at every stage (test: kill
   at each stage boundary, resume, same result).
3. Rerun the three-engine one-command draft on the pinned versions (PostgreSQL 17.11, MySQL 8.4.7, Oracle 23.5) and then
   the full chain on PostgreSQL 17.11 end to end (bounded live model usage).
