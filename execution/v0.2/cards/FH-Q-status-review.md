## Card FH-Q — split status semantics and per-table semantic review (S6-A-01, S6-G-01; D22)

Branch `v0.2/fh-q` from `1f267b6f5b95180139196fbb90c5b2e716d14699`. Commit as the owner (no agent trailers), trailer `Plan: S6-A-01 S6-G-01`. Read plan
§4 rows S6-A-01/S6-G-01, §5 T9 checklist, D22. Your files: `deploy/lite/**` (scheduler.py, lifecycle.py and tests),
`crates/oo-cli/**` (status, review), `tools/pg-builder/scripts/**` (retained_build, semantics), `docs/guide/**`,
`docs/reference/**` (regenerate with tools/docs/*.py), new `tools/oo-test/t9_status.py`. Do NOT edit `tools/oo-load/**`,
`crates/oo-pipeline/**`, `crates/oo-gateway/**`, `execution/**`, `specs/**`.

Contract from lane P (implemented in parallel; code against it, use a fake loader in unit tests): `oo-load sync`
per-table lines gain `status` fresh|noop|stale with `stale_reason`/`stale_detail`; exit 0 all fresh, 3 partial,
4 all stale, other nonzero global failure. `oo-load publish` accepts partial and reports `stale_tables`. New source
columns are never published until reviewed.

Build:
1. **Scheduler (deploy/lite/scheduler.py).** Exit 3 → continue to candidate/publish and record cycle result
   `partial` with the stale tables; exit 4 or global failure → `failed` (no publish). Record per cycle in the control
   DB event payload: result, reason, stale_tables, review_pending, data_published_at (only when a publication was
   activated).
2. **Per-table semantic review (S6-G-01).** Today `data_only` compares one library-wide digest and any metadata change
   (one comment) makes the whole library `review_required` and stops scheduling. Make the digest per table: tables whose
   semantic inputs are unchanged → data_only as now; a table with metadata change (comment, FK) or new columns →
   `review_pending` for that table only; the cycle still publishes data for all tables under the last reviewed
   registry (new columns excluded); `reviewed_shape` checks only the reviewed projection. Keep data-only digests stable
   for unchanged deployments (existing schedule configs and reviewed registries must keep working; test the migration).
   `oo review` shows pending tables and their changes.
3. **Status (S6-A-01).** `oo status --json` (and human output) gives: `data_published_at`, `cycle`:{result
   succeeded|partial|review_required|failed, at, reason}, `tables`:{stale:[{table,reason,since}],
   review_pending:[{table,changes}]}, plus today's services/credential fields. Health: services up AND
   data_published_at within 2×interval+grace → `healthy`; partial or review_pending → `degraded` (not failed);
   failed cycles alone do not flip health while data is fresh; alert text keyed to data publication age. Update
   docs/guide (status, troubleshooting) and regenerate references.
4. Tests: unit tests for every scheduler branch, status computation, per-table digest and migration of old configs;
   new `t9_status.py` covering T9-08 and T9-09 against a live lite deployment on PostgreSQL 17 (fresh 0700 HOME outside
   evidence, model via the supported import of `~/.config/open-ontology/secrets/model-endpoint-9r` and
   `model-key-9r-sol`, never printed or copied; exact-byte scan of evidence = 0). Run deploy/lite discovery,
   `cargo test --locked -p oo-cli`, pg-builder scripts unittest (OO_LIBRARY_BUILDER_BIN set), clippy, fmt,
   `python3 ci/check_generated_docs.py`. List files bound in `supply-chain/secret-review.json` that you changed.
Report: commits, field names, tests with counts, live results, gaps.
