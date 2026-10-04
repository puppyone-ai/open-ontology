# a159 full-import review

Verdict: **APPROVE LIMITED** the unpublished PostgreSQL full import on commit `cc764cbc5f9d5d2cf6056a2c41ff2612161b5ff4`, tree `da85327b402378cfdc3b826300205569c43b3642`, parent `2996562ceb332cc0ce505c703e775615fa321e5b`.

The approved surface is `import_unpublished` on one idle target connection: a real `SnapshotOwner` cut, the approved COPY encoder and sink, masked quality, and `commit_apply_receipt` in one transaction, then `finish_complete` and a stored `awaiting_publish` seal. Schema install is not that seal.

This approval does not cover publication, an Agent grant, a full type certificate, CDC, a control-database mirror, a job, or a stream acknowledgement. It does not close parent SP04.

## Fixed source

| Item | Value |
| --- | --- |
| Commit | `cc764cbc5f9d5d2cf6056a2c41ff2612161b5ff4` |
| Tree | `da85327b402378cfdc3b826300205569c43b3642` |
| Parent | `2996562ceb332cc0ce505c703e775615fa321e5b` |
| Subject | pipeline: add unpublished PostgreSQL full import |
| Author evidence | `549a82a0` (not this review's proof) |
| `0005` SHA-256 | `a9cef2013019784857353ff4fe060350d71a4e9ac4ab18551b992a7a9b52ecb6` |

The workspace was not checked out. At evidence time `HEAD` was `fef8b7e2ff275c2b1aaaca1657b6d4e1023e8bc5`. `git diff cc764cbc -- crates/oo-pipeline contracts/pipeline/full-import-v1.md contracts/pipeline/full-import-v1.json` was empty. An earlier reading in this session saw `HEAD` `5bfbd49ac8f73080f74003af8b22d335a4dcecdb`; later coordinator commits moved `HEAD` and left those paths unchanged.

Blob ids are in `linux/smoke/fixed-blobs.txt`.

## Reused approvals

| Primitive | Commit | Evidence | What this review did |
| --- | --- | --- | --- |
| Reader completion | `16ca2969cd449653a64d98d9831d3c8b8976942a` | a149 | `snapshot.rs` diff against the fixed commit is empty |
| Reader `finish_complete` | `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c` | a153 | `snapshot.rs` and `lib.rs` diff is empty |
| Physical mapping | `b5415544c9eb8d608eafd411884627dda9ee7c84` | a142 | `0004` and `physical_mapping.rs` diff is empty |
| COPY encoder | `d80472d8a99afd66d8d0e71420f7be2bb62e65c8` | a148 | `copy_encoder.rs` diff is empty |
| Installer, including the route that runs `0004` alone | `4ed22b8c1189e72b06b20a45335860ac9a3ea44e` | a152 | `0000`–`0004`, encoder, and mapping diffs are empty. `target_installer.rs` is 258 insertions and 0 deletions. The insertion that adds `install_full_import` starts after `verify_readiness` returns |

a152 already proved the mapping-only installer route. The author note that this route was still open is stale. This review did not rerun that matrix. The narrow live check is below.

## Package smoke

One `linux/arm64` Rust builder, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Archive of `cc764cbc` with no overlay and no PostgreSQL. Container `oo-a159-smoke` `45e940765ae46b9c1f5704f5be3be16658c0c608fc80885719c563b74ab5b79c`. Private network, `cap-drop ALL`, not privileged, no published ports, `HOME=/tmp`, private cargo and target volumes.

| Phase | Exit |
| --- | --- |
| toolchain | 0 |
| archive hash check | match |
| simple `unsafe` classifier | 0, one token: `lib.rs:12` `#![forbid(unsafe_code)]` |
| `cargo fetch --locked` | 0 |
| `cargo test -p oo-pipeline --lib --locked --offline` | 0, 11 passed |
| `cargo clippy -p oo-pipeline --all-targets --locked --offline -- -D warnings` | 0, `Finished dev` in 10.85s |

`sqlx-postgres` printed two unfulfilled `dead_code` expectations under `crates/oo-connectors/vendor/sqlx-postgres`. Those are dependency warnings. The crate under `-D warnings` still exited 0. Logs: `linux/smoke/`.

## Private PostgreSQL

One server, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a159-pg` `b7fc82d41f2f5568a0ccba63706fc4e5d2ab9b671775f4ccb9f188d391a21041`. Network `oo-a159-eef831aa-net`, alias `oo-a159-pg`, no published ports, not privileged, memory 1g, `cap-drop ALL` plus `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. Source and target were databases on this server. The Rust builder mounted `ca.crt` only. The server key stayed in the postgres mount. Passwords were files, not process arguments.

## Composition oracle

Probe container `oo-a159-probe` `c8fe4f45f045d5883c1b9b7e634f65ae753daa999f6a9a8efef4d028da91d06a`, same Rust image and private volumes, after the smoke container was removed. Second archive of `cc764cbc` plus `tests/full_import_a159_review.rs`. Production file hashes matched the smoke expects. `cargo test --locked --offline -p oo-pipeline --test full_import_a159_review -- --ignored --test-threads=1` exit 0. The test passed in 14.15s. Outcome: `linux/probe/live/outcome.txt`.

Fixture is `yard.crates` (3 rows) and `yard.tags` (2 rows), plus `yard.bulk` and `yard.notes`. The token `spruce-keel` is in the source label. Independent projection digest prefix `a159-oracle-v1`.

| Check | Result |
| --- | --- |
| Server | PostgreSQL 17.10 |
| `install` then `install_full_import` | Bootstrapped, then Installed, then Replay |
| Frozen `applied_at` for bootstrap, fence, preflight, and mapping | unchanged |
| `install` after `0005` | Replay, frozen readiness still holds |
| `pg_read_all_stats` for `oo_target_owner` | inherit `true`, set `false`; role `rolinherit` stays `false` |
| Runtime and Agent membership of `pg_read_all_stats` | false |
| `_load_held_generation`, `commit_apply_receipt`, `full_import_assert_bound` | lock text, `_load_held_generation`, `backend_start`, and `pg_backend_pid` still present |
| Runtime `install_full_import` | `NotAdministrator` |
| Lie checksum of 64 `e` on `full-import/v1` | `OO001`, stamps unchanged, frozen readiness holds, full-import readiness is `ChecksumMismatch`, `full_import_run` absent |
| Runs created by install | 0 |
| `import_unpublished` of both tables | `awaiting_publish`, 5 rows, source generation 1 |
| Session row | `backend_pid` and `backend_start` equal `pg_stat_activity` on the writer connection |
| Oracle | crates `301ea1bbe4416d9bea11121343daeb48b3048ea45f3784df7f69063137b753eb` (3), tags `b3169d53e3a17365d1361235d31544b7f4468a412cd20ccf599eaa7b61efa8b2` (2). Source projection equals target projection. A second `SnapshotOwner` `finish_complete` reproduced both canonical digests |
| Shadows | logged (`relpersistence` `p`), positions `0,1,2` and `0,1` |
| Stored mapping | `plan_physical_mapping` and `lookup_physical_mapping` equal the stored name |
| Quality | no raw column; `spruce-keel` absent from quality text and present in the shadow; batch `input_digest` matches the run; accepted rows sum to 5 |
| Publications | 0 |
| Agent `SELECT` on the shadow and on quality | denied (`42501` or `3F000`) |
| Direct `UPDATE` of `target_generations` by the administrator | `OO014`, `renewed_at` unchanged |
| Second runtime `INSERT` into an open shadow, and an unbound `commit_apply_receipt` | blocked, receipt count unchanged |
| `finish` before exhaustion | error contains `not exhausted`, state `retired` |
| `max_rows_per_chunk = 1` on 3 rows | `row_chunks` 3, state `awaiting_publish` |
| Value budget 16 against a 400-byte text | `ImportError::Source`, run `retired`, awaiting count unchanged |
| `jsonb` oid 3802 | `Unsupported`, type name contains `json`, shadow count unchanged |
| Insert during an open snapshot, then a fresh generation on the same shard | first seal 3 rows; fresh seal 4 rows; new physical name; old shadow stays 3 |
| Cancel, 3-second deadline, and `pg_terminate_backend` of the source | each run `retired` |
| Second adopt of the held shard | `55P03` or `OO017` |
| Other shard | `awaiting_publish` |
| `fail_fence`, then a new adopt | generation increased; stale pull returned `OO011`, `OO018`, or `OO013`; `finish` returned `reader is not exhausted`; state stayed `open` and was not `awaiting_publish` |
| Rollback of an uncommitted batch | other connection sees 0 rows, quality unchanged, recover `Absent` |
| Terminate the writer before the commit response | recover `Absent` |
| Commit, then a conflicting payload | row count unchanged, recover `Replay` for the stored digest and `Conflict` for the other |
| Replay on a new connection with the stored owner | `ApplyStep::Replay`, row count unchanged |
| Source audit | `oo_a159_reader` writes 0, `oo_a159_writer` writes 1, reader `INSERT` privilege false |
| Receipt call with no adopted run | receipt count and `awaiting_publish` count unchanged |

Attempt 1 is `linux/probe/attempt-1/`. The overlay did not compile (borrow, `Debug`, and `query_scalar` lifetime). Exit 101. No product file was edited. The green probe is the corrected overlay.

## Safe Rust

`crates/oo-pipeline/src/lib.rs:12` is `#![forbid(unsafe_code)]`. The smoke classifier found that single `unsafe` token in the pipeline sources it scanned. Clippy `-D warnings` on the package is the extended check. Both exits are 0.

## Grant

`0005_full_import_expand.sql` lines 8–39 and `grant_caller_backend_visibility` in `target_installer.rs` grant `pg_read_all_stats` to `oo_target_owner` `WITH INHERIT TRUE, SET FALSE`. The live catalog matched. Runtime and Agent are not members. `0001` still raises `this backend does not hold the target lock` from `_load_held_generation`, and `commit_apply_receipt` still calls that function. The grant is not a security approval.

## Requirement map

| Claim | Fixed source | Evidence |
| --- | --- | --- |
| Real `import_unpublished` from `SnapshotOwner` through COPY to one target connection, quality, receipt, `finish_complete`, `awaiting_publish` | `full_import.rs` `import_unpublished`, `send_batch` in `hold_batch`, `finish` calls `finish_complete` then `full_import_seal_run` | `linux/probe/live/outcome.txt` `main_state=awaiting_publish`, session bound, oracle digests |
| Independent schema, count, value, and canonical digest on two tables and a different fixture | `yard.crates`, `yard.tags` in the overlay | oracle lines, rows 3 and 2, projections equal |
| Concurrent writer keeps the open cut; a fresh generation rereads | mapping identity includes source generation | `coherent_cut=3_then_4`, `old_shadow_unmixed=3` |
| No fabricated completion, table manifest, or selected-coverage gap | `finish` requires `saw_exhausted` and binding match | `coverage_gap=retired`; `registration_only=not_complete`; install run count 0 |
| `SnapshotFinished` does not admit | unit test concatenates the banned name; entry calls `finish_complete` | smoke `lib.stdout` that test passed |
| Cancel, deadline, source loss, and typed or read failure retire the whole run | `cancel`, `fail_reader`, refusal path | `cancel=retired`, `deadline=retired`, `snapshot_lost=retired`, `oversized=retired` |
| Unsupported types stay truthful | `APPROVED_OIDS`; jsonb is absent | `unsupported_jsonb=refused`, shadow count unchanged |
| Fence holds lock, session, `backend_start`, owner, and generation at write and commit | `full_import_assert_bound`, `commit_apply_receipt` calls `_load_held_generation` | `session_bound=1`, `fence_checks=present`, `admin_dml=OO014`, runtime insert blocked |
| Runtime DML cannot bypass the fence; Agent cannot read shadow or quality | shadow triggers; `REVOKE` in `0005` | `runtime_dml=blocked`, `agent_denied=1` |
| No pool COPY plus an unrelated receipt; registration is not completion | COPY uses the caller transaction | `receipt_only_unbound=blocked`, `registration_only=not_complete` |
| Bounded COPY chunks, masked quality, row lineage | `CopyBudgets`; quality table has no raw column; `_oo_pos` | `backpressure_chunks=3`, `quality_masked=1`, `quality_lineage=1`, positions `0,1,2` and `0,1` |
| Stored physical mapping, replay does not rehash or copy again | `plan_physical_mapping`; `stage_decision` returns `replay` | `mapping_stored=1`, `replay_rows=unchanged` |
| Crash before commit response is absent; crash after replays; mismatch aborts with no duplicate rows | `rollback`, `commit_ambiguous`, `abort_conflicting`, `recover` | `rollback=absent`, `crash_before=absent`, `crash_after=replay`, `conflict=aborted` |
| Same shard blocked, other shard imported, takeover increments | `adopt_target_generation` | `same_shard=blocked`, `other_shard=imported`, `takeover=incremented` |
| Source reader writes stay 0 | reader privilege and audit trigger | `source_reader_writes=0`, `source_audit=writer_only` |
| Safe Rust | `lib.rs` `forbid(unsafe_code)` | smoke classifier and clippy exit 0 |
| Stats grant is versioned, owner only, set false, fence checks remain | `0005` DO block and `grant_caller_backend_visibility` | `stats_grant=owner_inherit_true_set_false`, `fence_checks=present` |
| `0005` install, replay, checksum, and readiness leave `0000`–`0004` and `install` in place | installer diff 258 insertions, 0 deletions; `install` does not execute `0005` | `frozen_applied_at=unchanged`, `installer_oo001=unchanged`, `installer_0005=installed_replay` |
| Mapping-only route | unchanged `0004` since a142; a152 approved the route that runs `0004` alone | diffs in `source/`; a152 HANDOFF. This review did not rerun that matrix |
