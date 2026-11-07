## Card M-S1 — lane M (coordinator-owned, executed by Codex): staging/generation model and rolling scheduler

TODO: S1-M-01, S1-M-02. Lane label: `m`. Spec: REQ-ENG-02/03 (loader side). Playbook: docs/product/02 §3–4, §6.

Context: much of this exists from a production deployment — crates/oo-pipeline (native_update.rs,
native_c5_range.rs, target_installer.rs, migrations up to 0118), tools/source-onboarding
(refresh_profile.rs bounded concurrency 1–4, refresh_incremental.rs immutable cycle budget,
refresh_schedule.rs), tools/job-worker (native_schedule*.rs, lease.rs). Do NOT rewrite it. Your job is to
prove the plan exits against it, close real gaps, and expose it to the engine interface.

Deliver:
1. S1-M-01: document (crates/oo-pipeline/README or module docs) and verify the path
   private staging → full-range atomic replace → publish, the immutable-parent + change-chunk generation
   model (no whole-parent copy per change chunk) and target write metering (bytes written per cycle). Add
   metering if missing. Exit test: two real SIGKILLs (one after staging, one after target commit) on a
   real PostgreSQL target, resumed by the same task with a higher lease epoch, with zero duplicate apply
   (row-level comparison). Add a thin adapter so any `SourceEngine` ChunkReceipt stream
   (crates/oo-connectors/src/engine) can feed this loader path; a unit test with a fake engine.
2. S1-M-02: verify bounded rolling dispatch (concurrency 1–4, refill as soon as a slot frees, serial per
   (source, target_database)), stop-dispatch-on-failure (in-flight tasks finish, receipts kept) and the
   immutable cycle budget (resume never extends the deadline; downtime counts). Exit test: scheduler unit
   tests where one slow task does not block other slots and a failure stops new dispatch.
New migrations only as new files after 0118 (coordinator merges migration order). Never modify existing
migration bytes.

Write scope: crates/oo-pipeline/src/** (no edits to existing migrations; new migration files allowed),
tools/job-worker/src/**, tools/source-onboarding/src/refresh_*.rs, tests for these. Prefix `pipeline`,
`job-worker`, `source-onboarding`. Real-PG tests with a postgres:17 container labelled oo.lane=m.
