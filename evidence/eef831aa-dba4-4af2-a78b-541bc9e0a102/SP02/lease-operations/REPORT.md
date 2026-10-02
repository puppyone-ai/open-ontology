# SP02-W01-b1 lease operations report

Session `eef831aa-dba4-4af2-a78b-541bc9e0a102`. This is b1 only: database-owned lease functions and their contract and fixture. The Rust driver, scheduler, NOTIFY loop, and AT-CTL-01/02 remain unimplemented. `crates/oo-jobs/src/lib.rs` still says claim, lease, and scheduling behavior is not implemented here. No plan checkbox was changed. No push.

## Baseline and commits

Task baseline `aeb351cfb3b8c31e305568cd655ed057d4afdbf5`, tree `62ca52ddfc358efb83dc032241938aa862775324`, parent `4d59c8f467c3328d153113ed74f753369a3c054b`.

Implementation commit `c312cae2daf75cfaa0b618066d709b9e529b3f95`, tree `19319751f04a7cb048bf3980a5408357d10acf58`, parent `4477f23205e989ac3c2207098efc369e6286cb98`. That parent is the workspace HEAD at commit time, later than the task baseline. The commit contains only the lease migration, migrations README, `contracts/jobs/lease-operations`, the oo-jobs contract and live tests and fixtures, and this evidence directory as it stood then. This report and the Linux implementation smoke are the following commit.

## Source pins

| File | SHA-256 |
| --- | --- |
| `0000_bootstrap_roles.sql` | `445d7248aa252a96a4103ae1b0be0000744eef2c0603037c19df6a0f8eeb0b52` |
| `0001_jobs_control_expand.sql` | `b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75` |
| `0002_event_project_binding_expand.sql` | `4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee` |
| `0003_job_lease_operations_expand.sql` | `5cb2e9030a479903ecc57ce32fc3ee851f00d146e02ab3902fcbe7ab09501034` |
| `jobs-control-v1.json` | `d352c86dbf2836360e541a3511af23674ce9692e687a6ac7041d99118dc8c2ca` |
| `jobs-lease-operations-v1.json` | `63009790833468ddb4affa766a8310893469f72895e958119ecc837b92a1365b` |

0000, 0001, 0002, and the jobs-control contract were not edited. The obsolete pre-fix 0003 digest `a00cfc68bc8f2ec4a3ac96cdecda724ecbaf8bc42d16cc2c6c4cbbd244e40b50` is not the published file. No Cargo.toml, Cargo.lock, or new dependency was written.

## What 0003 installs

Version `jobs-lease-operations/1`, phase expand. Owner `oo_jobs_owner` creates `SECURITY DEFINER` functions `enqueue`, `claim`, `heartbeat`, `complete_success`, `complete_failure`, `retry_failure`, and `cancel`, with `search_path=pg_catalog, jobs`. `PUBLIC` execute is revoked. `oo_jobs_runtime` execute is granted. Helpers `_require_lease_install`, `_lease_token`, and `_lease_result` are invoker and not executable by runtime or public. The migration refuses `current_user` other than `oo_jobs_owner` (`OO013`) and refuses to adopt an existing function of those names (`OO014`). It does not replace triggers from 0001 or 0002.

Readiness is the conjunction of `jobs.lease_operations_readiness` and the applicator comparing the stored checksum to the trusted file hash. The view only checks that a 64-hex `jobs-lease-operations/1` expand row exists. `jobs.service_readiness` still reports only `jobs-control/1`. A v1-ready database does not authorize these functions.

Direct runtime `INSERT`/`UPDATE`/`DELETE` on `jobs.tasks` and `jobs.task_events`, and `USAGE` on `jobs.task_events_event_id_seq`, are revoked. `SELECT` remains. Schedule and `jobs.leases` column grants from 0001 stay. `target_fence_certified` stays false and runtime cannot update it.

Lease length is `clock_timestamp() + 30 seconds`. Heartbeat poll of 10 seconds is a later driver concern. Claim scans at most 8 rows. Retry delay is `1 << (attempt-1)` seconds for attempt 1..6 and 60 seconds from attempt 7, with no jitter. `already_applied` is specified and never produced. The token prefix `jlt1.` is unauthenticated base64 from PostgreSQL `encode`, which wraps at 76 characters. Callers do not present the token. CAS uses project, task, worker, and generation.

## Phase exits

Baseline Linux arm64 Rust 1.94 smoke of `aeb351cf`, recorded under `baseline/`: fetch, test, clippy, clippy-component, worst, and `docker-run.exit` are all `0`.

Author-host checks on a clean `git archive` overlay plus these owned files, Rust 1.98.1, `CARGO_TARGET_DIR=/tmp/oo-a102-target` (removed after the run):

- `cargo clippy --locked --offline -p oo-jobs --all-targets -- -D warnings` exit 0 (`host/console.log`, `clippy_exit=0`).
- `cargo test --locked --offline -p oo-jobs` exit 0. Contract tests passed. `lease_operations_live` stayed ignored.
- `cargo test --locked --offline -p oo-jobs --test lease_operations_live -- --ignored` exit 0, 40.29 seconds, against container `oo-a102-pg1710-jobs`. `live/result.txt` is `live_fixture=pass`.

Implementation Linux arm64 smoke of commit `c312cae2daf75cfaa0b618066d709b9e529b3f95`, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, host `aarch64-unknown-linux-gnu`, rustc `1.94.0 (4a4ef493e 2026-03-02)`. Commands were exactly `cargo fetch --locked`, `cargo test --locked --offline -p oo-jobs`, and `cargo clippy --locked --offline -p oo-jobs --all-targets -- -D warnings`. `airgap_claim=no`. Exits under `impl/`: clippy-component, fetch, test, clippy, worst, and `docker-run.exit` are all `0`. The ignored live test was not executed in that builder: the mandated command has no `--ignored`, the builder has no docker socket and no psql, and the fixture database is outside that network. The builder log shows `lease_operations_live ... ignored`.

## Fixture

PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1), container `oo-a102-pg1710-jobs`, network `oo-a102-pg-net`, volume `oo-a102-pg-data`. Caps added: `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`, after drop ALL. No published port. Local trust inside the container. Login roles were `oo_a102_runtime_login`, `oo_a102_owner_login`, and `oo_a102_public_login`. `whoami` returned `oo_a102_runtime_login|oo_a102_runtime_login` for both `session_user` and `current_user`. No customer data. Password file was mode 0600 under `/tmp` and was not copied into evidence or argv.

## Oracle inventory

Proved on that fixture, with digests in `live/cases.tsv` and bodies in `live/out/`:

- Bad history: `OO004`, foreign-key count `0`, versions stay bootstrap and control. Legal upgrade snapshot SHA-256 `40842028275b768a64c3fd9117a3bc8c78bf903f0f5938c72d6b11c04a7ed70f` before and after 0003.
- Runtime replay of 0003: `OO013`, function oid unchanged.
- Planted `jobs.claim()`: `OO014`, stub still returns `7`, not security definer.
- Forced failure: `OO003`, no lease version row, no claim function.
- v1 service readiness `true`, claim `42883`.
- Functions without the version row: empty readiness, claim `OO015`.
- Lie checksum of 64 zeros: view `true|000…0`, Rust file-hash conjunction false, enqueue still returns `enqueued`. Stored checksum is not a file hash.
- Live ready line `true|5cb2e9030a479903ecc57ce32fc3ee851f00d146e02ab3902fcbe7ab09501034`. Repeat keeps the oid. Mismatch checksum `OO001`, version bytes and oid unchanged.
- Same project and dedupe with the same identity replays, including jsonb key order. `run_at` is not identity. Cross-project same dedupe inserts a second task. Different payload same key is `OO011` with the row count unchanged.
- Runtime task and event DML `42501`. Public execute `42501`. Runtime execute of `_lease_token` `42501`. Schedule insert still returns `scheduled|…`. Privileges `false|false|false|true|false|false|true|true|true|false|false|false`. Fence insert `fence|false`, runtime update `42501`, owner update `23514`, read `false`.
- Sixteen invalid inputs `OO010`, including nulls, bounds, bad JSON, a top-level password key, and the canary kind. Captured stderr does not echo that canary.
- Claim gap 30 seconds by the database clock. Stale worker `lost_lease`. Heartbeat extends. Event count `2` across enqueue and claim. Heartbeat does not add an event. Old generation `lost_lease`. Success clears the owner. Repeat and any later generation return `terminal`. Output has no `already_applied`.
- Ten-second `pg_sleep` then heartbeat: statuses `enqueued|claimed|extended|true`, new gap in 28..=30, and the call took at least 9 seconds.
- Expired running, same worker name: new attempt 2, generation 2, new token, generation 1 complete and heartbeat `lost_lease`, new generation succeeds.
- Queued cancel and expired running cancel each transition once. A second cancel and a later heartbeat return `terminal`. Running cancel returned the cleared owner and generation.
- Future `run_at` and an empty project return `none`.
- Enqueue plus `1/0` rolls back `22012` and leaves count `0`. Explicit rollback of claim leaves `queued|0|0|1`.
- Attempt-cap claim finalizes 8 expired rows (`counts|8|1`), returns `none`, then claims the later queued row at attempt 1. Complete of a capped row returns `terminal`.
- Generation `9223372036854775807` claim is `OO012` and leaves `queued|9223372036854775807`.
- Retry delays fell in 0..=2, 31..=33, and 59..=60. Exhausted retry stores `attempt_exhausted`. A mismatched legal generation returns `lost_lease` and does not mutate.
- Search-path poison: planted `public.clock_timestamp()` left the catalog-clock gap at 30 (`gap|claimed|30`), then the function was dropped.
- Lock cycle: owner `40P01` (`deadlock_owner` exit 3), claim exit 0. This DDL does not close that cycle.
- Migration waiter: `55P03`. Version bytes unchanged. Holder was terminated.
- 100 concurrent claim sessions: `batch_counts` `100|100|100|100|100|100`. Locks afterward `0|0`.
- Killed open claim: `queued|0|0|1`, then same worker name `claimed|1|1|worker-hold`.
- Restart durability SHA-256 `ba236ac9ddb120f8ba5910e18c101ab3a399d3a04ff8fa839e58db756135875b` before and after.

## Cleanup

Exact a102 containers, networks, and volumes were removed after the logs were copied. `cleanup.txt` lists them. `oo-a098-pg-b254bfa9-65d9-4209-9ac6-4aac2b2289b3` was still running afterward. No prune. Host overlay `/tmp/oo-a102-host`, target `/tmp/oo-a102-target`, and `/tmp/oo-a102-eef831aa-dba4-4af2-a78b-541bc9e0a102` were removed. The fixture password file was in that temp tree and was not committed.
