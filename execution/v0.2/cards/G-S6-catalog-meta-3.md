## Card G-S6-catalog-meta-3 — scheduled retained cycles re-export the declared source catalog

TODO: S3-G-02, S6-B-01. Branch `v0.2/g-catalog-meta` (fast-forward/reset it to `20a3884d045c14e90752102690ed55d9a4f989dd`, which already contains your passes
1-2 merged). Commit rules as before (owner identity, no agent trailers, `Plan: S3-G-02 S6-B-01`).

Defect (rehearsal pass 5, `../rel/evidence/rel-rehearsal-5/REPORT.md` and `requested-shared.patch`): the first lite
build pins `source-catalog.json`, but the scheduled cycle (`deploy/lite/lifecycle.py` provision_schedule →
`deploy/lite/scheduler.py` data_only → `retained_build.facts_for`) builds without it, so an unchanged source yields a
different data-only digest, `review_required`, and scheduling is disabled (`oo status` healthy=false).

Coordinator decision: do NOT reuse the first build's pin (it would hide later FK/comment changes). Each scheduled cycle
must re-export the catalog fresh from the source, with the same code path as the first build, and pin it for that cycle:
- Expose the existing export (crates/oo-cli/src/admin/build_retained.rs, your pass 2) as a non-interactive hidden admin
  subcommand of `oo` (e.g. `oo admin source-catalog --stack <stack.json> --source <id> --secrets-dir <dir> --output
  <file>`; match existing hidden/admin command conventions and help text policy; JSON result on stdout, named errors).
  The runtime image already contains oo (deploy/lite/Dockerfile builds oo-cli); find its installed path there.
- `scheduler.py` data_only: before `facts_for`, when the build config says the first build used a catalog (record
  that fact in the schedule config written by provision_schedule, e.g. `'source_catalog': 'export'`, derived from the
  first build's builder-config.json; older deployments without it keep today's behaviour), run the export into the cycle
  work dir (0600), pin it into `build['source_catalog']`, then proceed. Export failure is a named cycle failure, not a
  silent fallback to no-catalog.
- A genuine metadata change (FK added/removed, comment edited) must give review_required; unchanged metadata must give
  data_only with the reviewed digest. Decide and document whether comment-text-only edits count (they feed drafts, so
  review_required is the conservative answer; say which you chose and why in the report).
- Schedule-config migration: deployments provisioned before this change must not hit
  LITE_SCHEDULE_CHANGE_REQUIRES_EXPLICIT_MIGRATION just from the new key; handle it explicitly and test it.
- Tests: scheduler/lifecycle unit tests (deploy/lite/test_*.py) for unchanged → data_only, FK change → review_required,
  export failure, pre-change schedule config; oo-cli test for the subcommand; fmt, workspace clippy -D warnings,
  `cargo test --locked -p oo-cli`, `python3 ci/check_generated_docs.py` (regenerate docs/reference/cli.md and
  docs/guide/troubleshooting.md via tools/docs/*.py — you may commit those regenerated files), list secret-review-bound
  files you change.
- Live: fresh lite deployment against a PG17 fixture with 2 FKs + comments (labels oo.lane=g; up to three DB containers):
  first build + self-review publish, then at least one scheduled cycle reaching data_only with `oo status` healthy=true;
  then add a comment change in the source and show the next cycle returns review_required. Use the model only through
  the owner files by secret_ref and the supported import into a fresh 0700 HOME outside evidence (as rehearsal pass 4
  did); exact-byte scan of evidence for both files must be 0. Remove everything you started.
Final report: commits, design decisions, tests with counts, live cycle evidence, gaps.
