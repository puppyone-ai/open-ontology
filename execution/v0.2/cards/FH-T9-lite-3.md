## Card FH-T9-lite-3 (second rerun) — integrated lite T9 (status, per-table review, restore, weak CPU) on the integration tree

Worktree `v0.2/rel` reset to `a49655baf03048a9ccdfb0c3e17f03eb8e842747` (v0.2/field-hardening). Report-only: no commits/tracked edits, no public pushes,
no `npm publish`, no `docker login`. Outputs under `evidence/fh-t9-lite-3/`. Read `evidence/rel-rehearsal-6/REPORT.md`
and `exact-commands.md` (method), plan §5 T9 list, `deploy/lite/fh-q-evidence.json`, `crates/oo-gateway/FH-R-VALIDATION.md`.
1. Build the lite runtime image (arm64, from this tree's `deploy/lite/Dockerfile`, now a release build) and the oo CLI;
   serve the image from a throwaway local registry by digest (as rehearsal 6) and stage/pack the npm tarball.
2. Fresh 0700 HOME outside evidence, npm install, PostgreSQL 17 fixture (oo.lane=rel; 3 related tables with 2 FKs and
   comments), `oo init` with the supported model import of `~/.config/open-ontology/secrets/model-endpoint-9r` and
   `model-key-9r-sol` (never printed/copied elsewhere), `oo up`, `oo build`, D6 self-review publish, sync interval 60 s.
3. T9-09: `oo status --json` shows data_published_at, cycle.result succeeded, healthy. T9-08: edit one table's comment
   → next cycle: only that table `review_pending`, others published (Agent query of an unchanged table returns fresh
   data after a source insert); status `degraded`. T9-01 via lite: revoke SELECT on one table from the source role →
   that table `stale: source_permission`, cycle `partial`, others fresh; Agent query on it carries the stale reason;
   restore grant → next cycle fresh. T9-11: stop the source → cycle failed, data_published_at unchanged, health keyed
   to publication age.
4. T9-10 via lite: `pg_dump`/`pg_restore` the Agent DB into a fresh container used by a re-pointed deployment (or the
   documented restore procedure) and show Agent queries still succeed; one tamper case (replace a published view) refused.
5. T9-12: run the Gateway catalog warm-up and 20 representative queries in a container limited to 2 CPUs, for this
   release image and for the earlier dev image `ghcr.io/puppyone-ai/open-ontology/lite@sha256:75623ff22175d7194a7f8809b24fbddd9cb45cd5c628dff0cb1af5359f783bf5`
   (pull with the host Docker login via DOCKER_CONFIG; read-only); report times and the ratio.
6. Exact-byte scan of evidence for both model files = 0; clean up all oo.lane=rel resources, the registry and HOME.
Pass 1 (`evidence/fh-t9-lite/REPORT.md`) found three integration blockers, now fixed on this tree: missing oo_target_admin role, broad data-login grants invalidating logical identities, and publish-retained rejecting isolated receipts. Read it first and reuse its scripts. Validate fresh install, repeat up/build, and that identities stay valid.
Pass 2 (`evidence/fh-t9-lite-2/REPORT.md`; reuse its scripts) left three findings, fixed on this tree: Gateway now reads `sync_state` stale reasons (T9-01 Agent reason); an all-noop cycle after a partial one replays the active publication (no `mixed_retained_runs`), and status gains `data_verified_at` with health keyed on it; the first reviewed build no longer exits `BUILD_LITE_GATEWAY_SERVICE` (status retained_ready before the first cycle). Verify each explicitly: first build exits 0; T9-01 Agent response names source_permission; after a partial cycle and grant recovery, at least two all-noop cycles succeed with data_verified_at advancing and health healthy while data_published_at stays put.
Report: step table pass/fail with cause, JSON excerpts of status, timings, secret-scan hits, gaps.
