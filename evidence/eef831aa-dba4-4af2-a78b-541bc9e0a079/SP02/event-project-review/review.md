# SP02-W01-c review

Decision: **approve** `SP02-W01-c` only, the `jobs-event-project/1` event project binding.

Commit `9224d31e1c1d4a60d44b6984b8ad496a09e706a1`, tree `9e4bd758216287ee78c162349950b988da25978b`, parent `304fcf9f116bbbabe0f91a2caca5df5949ef476c`, subject `jobs: bind task event project to its task`. Reviewed from this session's git archive. Workspace HEAD at archive time was `dd3f6da74168f2d254b4fda0431cde2532f0a230`. This review does not sign SP02-W01, SP02-W01-a, AT-CTL-01, AT-CTL-02, the jobs driver, CAS, the 30s lease, the 10s heartbeat, 100 workers, backup and restore, the target fence, or the vertical flow. It is a public functionality review. It is not protected-CI, customer, owner, runsc, or release approval.

A068-F2 is closed for a runtime `LOGIN` insert: an event project must equal the task project. A068-F1, A068-F3, A068-F4, and A068-F5 stay open. The prior SP02-W01-a approval of `70a4e72c3ea698038e0bdd38df29e38c01dfd24d` is left as that review recorded it.

## Archive

`git archive --format=tar 9224d31e1c1d4a60d44b6984b8ad496a09e706a1 -- Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo contracts` produced 300 files. `git get-tar-commit-id` returned the same commit. The finite manifest matches `git ls-tree` (`raw/manifest-diff.txt` empty, `manifest_match=yes`). The commit contains 1426 paths. `70a4e72` is an ancestor. Frozen diffstat of `0000`, `0001`, and `jobs-control-v1.json` against the parent is empty.

| File | SHA-256 | Git blob | Parent and `70a4e72` |
|---|---|---|---|
| `0000_bootstrap_roles.sql` | `445d7248aa252a96a4103ae1b0be0000744eef2c0603037c19df6a0f8eeb0b52` | `cc24c2efe6d28518088a872807be379ae648079b` | same blob |
| `0001_jobs_control_expand.sql` | `b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75` | `82879c91bdd98ac65ecd07f284744e4a688b1d8e` | same blob |
| `jobs-control-v1.json` | `d352c86dbf2836360e541a3511af23674ce9692e687a6ac7041d99118dc8c2ca` | `5515c24df23dd69f504c64ab9c8035e3830084aa` | same blob |
| `0002_event_project_binding_expand.sql` | `4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee` | `2d60527b11b5935bafe00f6b6fd7fced9016a5e7` | new |
| `jobs-event-project-v1.json` | `c8bd60c5ba9dd284b9d125d889dd5cef3d97f97e4a046d3f6e57656b786f7b42` | `fa6f50adce436dd70fae22c4be17b214658f1c02` | new |

`0002` locks `jobs.task_events` `SHARE ROW EXCLUSIVE`, then `jobs.tasks`. It counts events with no task of the same `id` and `project_id`, raises `OO004`, and otherwise adds non-deferrable `tasks_id_project_unique` on `(id, project_id)` and non-deferrable `task_events_task_project_fkey` `(task_id, project_id)` referencing that key, `ON UPDATE RESTRICT` `ON DELETE RESTRICT`. It creates `jobs.event_project_readiness` with `security_invoker`, revokes `PUBLIC`, and grants `SELECT` to `oo_jobs_runtime`. Executable text does not call `pg_advisory_xact_lock` and does not insert into `jobs.schema_migrations`. The view reads that table. Comments name the applicator lock `6401202601`. `jobs.service_readiness` still reports `jobs-control/1`.

`docs/17-rust-implementation.md` §3.1 still requires owner and generation compares, a heartbeat, and target-database fencing outside this schema. The plan's SP02-W01 stop condition remains AT-CTL-01/02. The published event contract lists the same non-claims.

## Linux smoke

One builder, `oo-a079-rust194`, image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. Capacity before launch, from `pid,ppid,etime,comm` and builder names only: `host_cargo=0 rust_builders=0 global=0`. Inspect: `Privileged=false`, network `oo-a079-rust-net`, `CapDrop=ALL`, `no-new-privileges`, no port bindings, tmpfs `/tmp`, memory 8g, 4 cpus, pids 4096.

`rust-toolchain.toml` says `1.98.1`. The smoke set `RUSTUP_TOOLCHAIN=1.94.0`. Recorded compiler: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM `21.1.8`, Debian 13. `airgap_claim=no`. `cargo fetch --locked` had no `-p`.

| Phase | Exit |
|---|---|
| fetch | 0 |
| test | 0 |
| clippy `-D warnings` | 0 |
| worst | 0 |
| docker run | 0 |
| container | 0, `OOMKilled=false` |

Passed: `control_schema_files_match_the_published_contract`, `event_project_files_match_the_published_contract`. The contract test hashes files with `sha256sum` or `shasum`.

## Postgres fixture

One fixture, `oo-a079-pg1710`, image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1) on aarch64-unknown-linux-gnu`. Inspect: `Privileged=false`, network `oo-a079-pg-net`, cap drop `ALL` plus `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`, `no-new-privileges`, no published port, memory 2g, 2 cpus, pids 512. Password file mode `600`, 49 bytes. Evidence stores that mode and size. `pg/postgres.log` is redacted. `pg/launch.sh` exited after the server was accepting connections because `{{json .HostConfig.Tmpfs}}` has no entry when Tmpfs is absent. The container was left up. `pg/inspect.txt` is a later inspect of that same container. The launcher was not run again.

Probes used archived SQL through `docker exec psql`. They did not call `event_project_fixture.sh`. Runtime proof is `LOGIN` role `oo_a079_login` (`oo_a079_login|oo_a079_login|false|true`). Owner proof is `LOGIN` role `oo_a079_owner_login`, then `SET ROLE oo_jobs_owner`. On the legal upgrade, `session_user` stayed `oo_a079_owner_login` and `rolsuper` stayed false while `current_user` was `oo_jobs_owner`.

First runner: 141 cases, 11 failed, exit 1 (`pg/probe-runner.exit`). Those 11 are recorded below and were not rewritten. Follow-up: 13 cases, 0 failed, exit 0. One later advisory probe exited 0.

## Binding

On `oo_fresh`, after `0000`+`0001`+`0002` committed in one transaction:

- Lifecycle claim and succeed, fail, and cancel-from-queued committed with matching projects.
- Same-project direct insert committed as `0||99|spoofed-worker|caller-supplied|true` (`pg/out/direct_same.stdout`).
- Cross-project insert, including `SET CONSTRAINTS ALL DEFERRED`, returned `23503` on `task_events_task_project_fkey` (`pg/out/cross_deferred.stderr`). The note was absent.
- Schema-qualified `SET CONSTRAINTS jobs.task_events_task_project_fkey DEFERRED` returned `42809` (`pg/out/follow_named_constraint_deferred.stderr`).
- NULL project, NULL task, and omitted project returned `23502`. `not-a-uuid`, empty, and short text returned `22P02`. A missing task returned `23503` on `task_events_task_id_fkey`.
- A composite swap, P1 task id with P2 project, returned `23503` on the composite key. A multi-row insert and a CTE that inserted a P2 task plus a P1 event both rolled back.
- While a runtime session held a task row and updated only `lease_until`, a same-project insert committed, a cross insert returned `23503`, and a project update returned `42501`. Aftermath project was P1, cross notes 0, same-project note 1 (`pg/out/hb_after.stdout`).
- Catalog text (`pg/out/fresh_catalog.stdout`): foreign key `ON UPDATE RESTRICT ON DELETE RESTRICT`, `condeferrable=false`, `convalidated=true`; unique `(id, project_id)` the same; `project_id` and `task_id` are `uuid NOT NULL`; view reloption `{security_invoker=true}`; `PUBLIC` select false and `oo_jobs_runtime` select true; `prosecdef` count 0; 8 functions in `jobs`; `service_readiness` names `jobs-control/1`; `event_project_readiness` names `jobs-event-project/1`; `task_events_task_id_fkey` remains.
- Follow-up identity on the upgraded database: `event_id` is `bigint` identity `a` (always), `project_id` and `task_id` are `uuid NOT NULL`. That column list matches the pre-upgrade database `oo_bad`, 1039 bytes. User triggers match, 510 bytes, including `tasks_audit_update|O|tasks`.

On the upgraded `oo_legal` database, a deferred cross insert of project P2 for the P1 task `...0001` returned `23503` and left no `followup-legal-cross` row.

The first `legal_cross` statement committed because task `...0002` was seeded in project P2. `follow_first_legal_cross_was_same_project` is `P2|P2|true`. That failed assertion stays in `summary.json`.

## Migration

Fresh install on an empty `oo_fresh`: session A took `pg_advisory_xact_lock(6401202601)`, slept, and applied `0000`, `0001`, and `0002` with the real checksums. `CREATE ROLE` inside that transaction committed (exit 0). Session B with `lock_timeout` 2s received `55P03` (`pg/out/fresh_b.stderr`). Repeat with the same checksums left the constraint OIDs byte-identical.

`0002` itself does not take that advisory lock, does not insert the marker, and does not set `lock_timeout` or `statement_timeout`. The fresh, upgrade, repeat, and rollback scripts do. Ready is the conjunction of `jobs.service_readiness` against the `0001` bytes, the bootstrap checksum against `0000`, and `jobs.event_project_readiness` against `0002`. `schema_version_present` is version plus phase.

Legal upgrade by the owner login left the task, event, schedule, occurrence, and lease snapshot unchanged (1493 bytes) and left the `jobs-bootstrap/1` and `jobs-control/1` rows unchanged (341 bytes). The new row is `jobs-event-project/1|4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee|oo_jobs_owner`. Function identity before and after matched (499 bytes). Repeat of the real checksums left OIDs unchanged. Repeat with a zero digest returned `OO001` and the stored checksum stayed the real hash. Schedule id `a0790001-0001-4000-8000-000000000051` is a UUID.

Preexisting mismatch, the a068 shape, committed on v1: event project P2, task project P1, note `a068-cross`, ctid `(0,3)`, xmin `803`. Early repeat returned `OO002`. Owner execution of `0002` returned `OO004` (`pg/out/bad_upgrade.stderr`). Data snapshot stayed 639 bytes, migration rows stayed 342 bytes, leak probe `true|0|0`.

Race: a runtime transaction inserted the bad row and slept. The owner `0002` was observed waiting on a relation `Lock`. The bad transaction committed. `0002` then raised `OO004` (`pg/out/race_mig.stderr`). The bad row remained. No constraint leaked. Under `READ COMMITTED`, the post-lock count sees a row committed while the migrator waited.

Forced `OO003` after the DDL and the marker insert, before `COMMIT`, left data and migration snapshots unchanged and leak probe `true|0|0`.

`oo_lie` stored 64 zero hex. `lie_view` printed `true|<64 zeros>`, then `false`, then `true`: the version marker is true, the file conjunction is false, and `jobs-control/1` readiness stays true. Repeating the lie left OIDs unchanged. Repeating the real hash returned `OO001` and the zero digest remained.

Search path: a `trap` schema with its own `task_events` and `tasks`, plus `SET LOCAL search_path = trap, pg_catalog`, still created `task_events_task_project_fkey` in `jobs`. `path_check` printed `t`, `t`, `1`, and that constraint name.

Lock and timeout, real `0002` file unless noted:

- Another session held `SHARE ROW EXCLUSIVE` on `task_events`. `statement_timeout` 2s returned `57014` at the first `LOCK` (`pg/out/stmt_mig.stderr`). No leak.
- A runtime heartbeat updated only `lease_until`. `lock_timeout` 3s returned `55P03` at `LOCK TABLE jobs.tasks` (`pg/out/lock_mig.stderr`). No deadlock, because `tasks_audit_update` does not insert when only `lease_until` changes. No leak.
- Follow-up lock order: owner `LOCK task_events`, sleep 5s, `LOCK tasks`, while a runtime claim changed `queued` to `running`. The migrator received `40P01`. Detail: one process waited for `ShareRowExclusiveLock`, the other for `RowExclusiveLock` (`pg/out/follow_dead_mig.stderr`). The claim committed. Leak probe `true|0|0|0`. The sleep opens the window. The two lock modes are the ones in `0002`. This probe did not also run the `OO004` scan inside that window. The separate race above did, and it aborted.

`oo_a079_login` acquired `pg_advisory_xact_lock(6401202601)` and rolled back (`pg/out/follow_runtime_advisory.exit` 0). A068-F4 remains.

## Role and privilege

Runtime replay of `0002` inside a transaction: `42501` permission denied for table `task_events`. Bare owner replay outside a transaction: `25P01`. That second result is not a privilege denial. Runtime `SET ROLE`, disable trigger, replication role, drop constraint, and project update: `42501`. Owner event update and owner project update: `23514`. Owner drop of the foreign key inside `BEGIN`/`ROLLBACK` left the OID. Owner `DISABLE TRIGGER ALL` stopped on the internal RI trigger with `42501`; the bypass note count stayed 0 and user triggers stayed `O`. Superuser disable of `tasks_enforce_update` still hit `23503` on the composite foreign key. `oo_a079_nobody` cannot select the readiness view (`42501`). A table created by the owner inside a rolled-back transaction did not grant `PUBLIC`. Role `rolconfig` for owner and runtime is null. No new `SECURITY DEFINER` function was created by `0002`. The adopted-definer path in A068-F5 was not planted again.

## Preserved failures

Author evidence under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a075/SP02/event-project-binding` was not edited. Attempt 1 omitted `contracts/` and the control-schema test exited 101. PostgreSQL attempt 1 expected `t` from `boolean::text` and saw `true`. Attempt 2 used a schedule id containing `s`. Attempt 3 expected delete-before-update in `pg_get_constraintdef`; PostgreSQL 17 prints `ON UPDATE RESTRICT` first. Attempt 4 treated a bare `LOCK` as a privilege failure and got `25P01`. Product `0002` stayed `4ede04e8…` across those attempts.

This reviewer's first runner failures remain in `pg/probe-console.log` and `pg/out/summary.json`:

| Case | What the output shows |
|---|---|
| `sql_no_advisory`, `sql_no_migration_insert` | Comments and the view mention the lock and `schema_migrations`. Follow-up separated comments from executable text. |
| `constraint_not_deferrable` | Unqualified name, `42704`. Qualified name, `42809`. |
| `fresh_catalog` | Catalog rows match the contract. The checker demanded one physical line. |
| `legal_identity_*`, `legal_triggers_*` | `42725` on `"char"`. The later "unchanged" compares of empty stdout are not evidence. `oo_bad` versus `oo_legal` with `::text` casts match. |
| `legal_cross_after_upgrade` | Same-project insert against the P2 task. The P1 task was rejected afterward. |
| `dead_mig_holds_events`, `audit_trigger_deadlock` | `SET deadlock_timeout` returned `42501` before any lock. The retry without that setting deadlocked and leaked nothing. |

## Still pending

Driver, claim CAS, 30s lease default, 10s heartbeat, 100 workers, backup and restore, target fence, and vertical integration are pending. Bootstrap checksum trust, the caller-supplied marker, runtime possession of advisory key `6401202601`, and a superuser-planted `SECURITY DEFINER` remain installer residuals. A live claim can deadlock an in-flight `0002`; the aborted transaction does not leave the constraint or a cross-project row. Pausing writers, setting timeouts, and retrying `40P01` belong to the applicator.

The reviewer did not stage or commit.

## Cleanup

Before removal, `docker ps -a` showed `oo-a079-pg1710` Up and `oo-a079-rust194` Exited (0). Names `oo-a072` and `oo-a074` were already absent. Networks `oo-a079-pg-net` and `oo-a079-rust-net`, and volumes `oo-a079-cargo`, `oo-a079-pg-data`, and `oo-a079-target`, were present. Both inspected containers were unprivileged.

Removed only those exact names:

```
docker rm oo-a079-rust194
docker rm -f oo-a079-pg1710
docker network rm oo-a079-rust-net oo-a079-pg-net
docker volume rm oo-a079-cargo oo-a079-target oo-a079-pg-data
rm -rf /tmp/oo-a079-eef831aa-dba4-4af2-a78b-541bc9e0a079
```

Each docker remove printed the resource name and exited 0. The temp directory test reported `absent`. A later `docker ps -a` still shows other sessions (`oo-a080`, `oo-a081`, `oo-a083`, `oo-a084`, `oo-a064`). No `docker system prune`. `git diff --cached` is empty. Evidence is untracked at `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a079/`. The reviewer did not stage or commit. Raw command log: `cleanup.txt`.
