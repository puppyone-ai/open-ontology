# SP02-W01-b1 lease primitive review

Decision: approve for the jobs-lease-operations/1 SQL primitive on commit c312cae2daf75cfaa0b618066d709b9e529b3f95.

This signs that primitive only. SP02 parent, the Rust driver, scheduler, NOTIFY, 100 whole workers, target fence, Loader, vertical proof, and release stay unsigned. Coordinator receipt stops this review.

## Source

| Item | Value |
| --- | --- |
| Commit | c312cae2daf75cfaa0b618066d709b9e529b3f95 |
| Tree | 19319751f04a7cb048bf3980a5408357d10acf58 |
| Parent | 4477f23205e989ac3c2207098efc369e6286cb98 |
| Handoff read, not compiled | dd5ecf39604b8467a0d1f91d56d2e90473fa2c48 |
| Migration 0003 SHA-256 | 5cb2e9030a479903ecc57ce32fc3ee851f00d146e02ab3902fcbe7ab09501034 |
| Contract SHA-256 | 63009790833468ddb4affa766a8310893469f72895e958119ecc837b92a1365b |
| 0000 / 0001 / 0002 | 445d7248aa252a96a4103ae1b0be0000744eef2c0603037c19df6a0f8eeb0b52 / b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75 / 4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee |

The oracle hashed the archive bytes before any mutation. They match the published digests. `git archive` of c312cae supplied the smoke and the SQL. Workspace HEAD was not moved.

## Linux arm64 Rust 1.94

Image `rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, platform linux/arm64, `RUSTUP_TOOLCHAIN=1.94.0`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`.

Focused `oo-jobs` command, locked, offline after fetch. No `--ignored`.

| Phase | Exit |
| --- | --- |
| clippy component | 0 |
| cargo fetch --locked | 0 |
| cargo test --locked --offline -p oo-jobs | 0 |
| cargo clippy --locked --offline -p oo-jobs --all-targets -- -D warnings | 0 |
| worst | 0 |
| container | 0 |

`lease_operations_live` stayed ignored. The ignore text names container `oo-a102-pg1710-jobs`. This smoke did not start that container.

## PostgreSQL 17.10 LOGIN oracle

One owned server: `17.10 (Debian 17.10-1.pgdg12+1)`, image `postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, trust auth, no password on argv, no host port, bridge network `eef831aa-dba4-4af2-a78b-541bc9e0a107-net`. The oracle is a std-only Rust client compiled in the same 1.94 image after the smoke container exited. `cargo build --offline` exit 0. No network rebuild. Binary SHA-256 `7e2b0e0560ea55fad4383780658eba582a085e97f02b5438e691c66dd2104229`.

Logins are `a107_runtime`, `a107_owner`, and `a107_public`. The author names `oo_a102_*` were not used.

| Phase | File | Oracle exit |
| --- | --- | --- |
| 1 identity | phase-1.txt | |
| 2 lie | phase-2.txt | |
| 3 upgrade-history | phase-3.txt | |
| 4 behavior | phase-4.txt | |
| 5 expiry | phase-5.txt | |
| 6 hold | phase-6.txt | full container 0, 137 passed, 0 failed |
| 7 restart | phase-7.txt | restart container 0, 5 passed, 0 failed |

Restart of the owned server was issued at 2026-10-02T20:10:40Z while a runtime transaction was open. The committed queued and running markers remained. The uncommitted enqueue was absent. Stored checksums remained.

Coverage that passed on this run, all through `a107_runtime` except labeled owner or superuser seeds:

- Empty claim, future `run_at`, disallowed kind, order `run_at` then id, cross-project dedupe, replay that keeps `run_at`.
- OO010 static text for nulls, empty and duplicate kinds, 65 kinds, worker shape, max_attempts 0 and 33, digest, array payload, bad JSON, top-level secret keys, infinity, and oversize payload. The canary was absent from message and detail.
- Stored jsonb of 16384 octets enqueued. Compact input of 16384 octets stores as 16385 because PostgreSQL jsonb text inserts one space, and that input is OO010.
- 64 kinds. Heartbeat generation 0 is OO010. Cancel of a queued generation 0 succeeds; a second cancel is terminal. Complete of that cancelled row with generation 1 is terminal.
- Lease gap about 30 seconds from the database clock. Heartbeat extends about 30 seconds and writes no task event. Stuffing the token into the worker is rejected. The token is `jlt1.` plus wrapped base64; CAS uses the columns.
- Lost lease for wrong project, worker, or generation. Bad failure kind leaves the row running. Success plus the event rolls back together, then the committed path returns terminal and never `already_applied`.
- Retry delays measured in the same statement: 1, 2, 4, 8, 16, 32, then 60 seconds. Two attempt-1 samples were 0.999837 and 0.999904. Attempt increments on the next claim. At the cap, `last_error` is `attempt_exhausted`.
- Business table and enqueue roll back together, and commit together.
- Claim rollback and `pg_terminate_backend` restore `queued|0`.
- While the caller transaction is open, a superuser `FOR UPDATE NOWAIT` gets 55P03. After commit, and after each of 100 autocommit claims, NOWAIT succeeds. Runtime `UPDATE` is 42501, so the runtime cannot be the lock probe.
- 100 concurrent runtime sessions, distinct pids `w000`–`w099`, 100 tasks, attempt 1, generation 1, 100 running events, 0 cross-project events, then 20 further claims returned none. Ten sessions on one job: one claim and nine none. Cancel versus complete: one terminal transition.
- Elapsed 32 seconds, labeled `clock_source=elapsed_db_32s`: expired heartbeat is `lost_lease` and does not revive; the same worker reclaims a new attempt and generation; the old generation is `lost_lease`; the first cap call finalizes 8 and returns none while a later due row stays queued; the next call finalizes the 9th and claims that row; cancel of an expired running row returns the invalidated binding and does not kill the backend.
- Owner seeds, labeled `clock_source=owner_seed_not_public_api`: `run_at` pulls between retries, and generation 9223372036854775807 raises OO012 with no claim of a later row until the owner moves that row's `run_at`.
- Direct task and event DML and `nextval` are 42501. Public execute and helper execute are 42501. Seven functions are security definer, owner `oo_jobs_owner`, `search_path=pg_catalog, jobs`. A public `clock_timestamp` does not affect claim. Schedule insert still works. `target_fence_certified` stays false; runtime update is 42501; owner update is 23514.
- `jobs.service_readiness` stays `jobs-control/1`. A lie checksum of 64 zeros makes `lease_operations_readiness.schema_version_present` true. Repeating with the file hash raises OO001 and leaves the lie in place.
- 0002 on a mismatched event raises OO004 and does not delete the row. Owner DELETE is 23514. The fixture then removed that row under `session_replication_role=replica`, labeled as fixture cleanup. Legal row hashes were unchanged across the real 0002 and 0003. Repeat with the same hashes kept the claim OID. A wrong repeat hash raised OO001.
- Runtime applying 0003 raises OO013. An existing `jobs.enqueue` raises OO014 and is left in place. A forced OO003 rolls the functions and the checksum row back. `jobs.planted_a107` survived.
- Migration advisory lock: a waiter with `lock_timeout` 2s receives 55P03. `a107_runtime` can hold lock 6401202601.
- Lock order: owner `SHARE ROW EXCLUSIVE` on `task_events`, then on `tasks`, against a claim. The owner session received 40P01. The claim committed `running|1`. No cross-project event.

`src/lib.rs` still says claim behavior is not implemented here.

## Scoped findings

These stay open. They do not change the b1 decision, and this review did not edit the product.

1. Docs/17 still describes backoff plus jitter. The contract lists jitter as not claimed. 0003 has no jitter, and the two attempt-1 delays matched.
2. `crates/oo-jobs/tests/lease_operations_live.rs` hardcodes `oo-a102-pg1710-jobs` and is ignored. The independent oracle used `PGHOST` on this session's server.
3. A068-F3: readiness treats any stored 64-hex checksum as present. The view does not hash the release file.
4. A068-F4: the runtime can take advisory lock 6401202601.
5. A068-F5: 0003 refuses only the lease function names. `jobs.planted_a107` remained.
6. The a079 `task_events` then `tasks` cycle still deadlocks with claim. This migration's comment says that cycle is not closed. The run reproduced 40P01.
7. A top-level secret key is OO010. A nested `password` key is enqueued. The contract says nested keys are not scanned.

Owner-only updates in the oracle are labeled in `cases.jsonl` and are not public API evidence.

## Retained attempts

Earlier oracle attempts failed in the harness and are kept under `oracle/attempt-1` through `oracle/attempt-4`, with notes. Attempt 1 did not reach SQL (`q` shadowed the quote helper). Attempt 2 expected an owner DELETE that the immutability trigger rejects with 23514. Attempt 3 used the wrong replay identity, unquoted JSON, input length instead of stored jsonb length, generation 0 for complete, and `pg_locks` for granted tuple locks. Attempt 4 probed `FOR UPDATE` as the runtime and got 42501. The final `oracle/out/cases.jsonl` has 142 passing rows and no failed row.

## Resources

One Rust builder at a time, after the smoke container exited. One PostgreSQL fixture. No subagents and no new worktree. Owned cleanup of the UUID containers, volumes, and network is recorded in `cleanup.txt`.
