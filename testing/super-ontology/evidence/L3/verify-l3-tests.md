# verify-L3: test runs on so/verify-L3 (base feat/super-ontology e82490f57), 2026-11-17

Host: local arm64 Docker Desktop. No registry access during this run (docker.io and auth.docker.io unreachable), so no image could be pulled or built.

## cargo

- `cargo test -p oo-docs --locked`: exit 0; unit 338 passed, all integration binaries 0 failed (ignored tests not counted here).
- `cargo test -p oo-connectors --features objectstore-s3 --locked --lib objectstore`: exit 0; 24 passed.

## --ignored tests (oo-docs), Postgres container from digest 839fbb5a, track_commit_timestamp=on, max_connections=200, trust auth, --test-threads=1

| binary | result | note |
|---|---|---|
| api_live | rc=0, 11 passed, 0 failed |  |
| bench_compile | rc=0, 1 passed, 0 failed | run with --release |
| conformance_constraints | rc=0, 1 passed, 0 failed |  |
| conformance_events | rc=0, 1 passed, 0 failed |  |
| conformance_freshness | rc=0, 1 passed, 0 failed |  |
| conformance_jobs | rc=0, 1 passed, 0 failed |  |
| conformance_review | rc=0, 1 passed, 0 failed |  |
| conformance_share | rc=0, 3 passed, 0 failed |  |
| conformance_state | rc=0, 1 passed, 0 failed |  |
| conformance_sync | rc=0, 1 passed, 0 failed |  |
| consistency_live | rc=0, 2 passed, 0 failed |  |
| constraints_live | rc=0, 3 passed, 0 failed |  |
| contract | rc=0, 3 passed, 0 failed |  |
| correct_api_live | rc=0, 1 passed, 0 failed |  |
| disk_guard_live | rc=0, 1 passed, 0 failed |  |
| download | rc=0, 0 passed, 0 failed | 0 run: file needs --features objectstore-s3 and an S3 endpoint (OO_S3_*), not available here |
| eval_live | rc=101, 0 passed, 1 failed | first run rc=101: OO_EVAL_OUT not set in the invocation; rerun with OO_EVAL_OUT, passed (191.7 s, model off) |
| extract_llm_stage_live | rc=0, 5 passed, 0 failed |  |
| faults | rc=101, 8 passed, 1 failed | first run rc=101: OO_DOCS_TEST_PG_CONTAINER not set; rerun with it set: 8 passed, postgres_restarted_under_a_running_engine_loses_nothing HUNG (no result after 20+ min), killed |
| files_api_live | rc=0, 6 passed, 0 failed |  |
| freshness_live | rc=0, 3 passed, 0 failed |  |
| gates_live | rc=0, 3 passed, 0 failed |  |
| ingest_fs | rc=0, 15 passed, 0 failed |  |
| install | rc=0, 8 passed, 0 failed |  |
| jobs_live | rc=0, 11 passed, 0 failed |  |
| l3_routing | rc=0, 2 passed, 0 failed |  |
| latency | rc=0, 1 passed, 0 failed |  |
| llm_live | rc=0, 4 passed, 0 failed |  |
| log_redaction | rc=0, 1 passed, 0 failed |  |
| pipeline_live | rc=0, 5 passed, 0 failed |  |
| promotion_live | rc=0, 1 passed, 0 failed |  |
| publish_live | rc=0, 5 passed, 0 failed |  |
| rebuild_live | rc=0, 1 passed, 0 failed |  |
| refresh_changes_live | rc=0, 1 passed, 0 failed |  |
| reprocess | rc=101, 3 passed, 1 failed | real assertion failure, reproduced on rerun: reprocess.rs:300 expects 2 queued parse jobs after one pump tick, code gives 1 (REPROCESS_INFLIGHT = 1 in crates/oo-docs/src/jobs/stages.rs:60); test and constant from the same commit f2b604e99 |
| resolve_live | rc=0, 5 passed, 0 failed |  |
| retention_live | rc=0, 1 passed, 0 failed |  |
| review_api_live | rc=0, 1 passed, 0 failed |  |
| review_live | rc=0, 5 passed, 0 failed |  |
| share_live | rc=0, 10 passed, 0 failed |  |
| sidecar_formats | rc=0, 7 passed, 0 failed |  |
| sidecar_supervision | rc=0, 6 passed, 0 failed |  |
| speaker_live | rc=0, 1 passed, 0 failed |  |
| state_live | rc=0, 3 passed, 0 failed |  |
| temporal_live | rc=0, 3 passed, 0 failed |  |

Not in this table: `download` (0 run, see above), and the S3 cells below.

## Not run in this verify (blocked)

- `super_ontology_l3.py --run-id verify-l3`: stopped at the start, `oo serve` refused the locked docs-engine image (`sha256:5555bbd3...`, absent on the host). Rebuilding it needs the pinned Python base (`sha256:281d39ff...`), which could not be pulled.
- `super_ontology_compose.py`: same engine image requirement, not run.
- `oo-test fast --layer L1 --cells docs-rustfs,docs-minio`: `rustfs/rustfs` and `pgsty/minio` images not present and not pullable.
