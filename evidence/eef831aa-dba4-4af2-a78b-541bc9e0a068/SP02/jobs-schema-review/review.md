# SP02-W01-a review (a068)

Decision: **approve** for SP02-W01-a only.

The reviewed tree is commit `70a4e72c3ea698038e0bdd38df29e38c01dfd24d`, tree `0d1c949ae35c68c07ecaf08438a2ea23944eedd3`, parent `6790bb641e97098326a6994cbfc9a316c3c11fd9`, subject `jobs: add the jobs-control/1 control schema`. `git archive` of that commit and the extracted review archive are the same 614 files, byte for byte (`raw/archive-identity.txt`). Workspace HEAD at the identity check was `2b68c89934d4ac1a73467ef563b14a895d6c3778` and was left untouched.

This signature covers the jobs-control/1 expand schema, its roles, grants, triggers, migration book, and the published contract. It leaves unsigned: full SP02-W01, AT-CTL-01, AT-CTL-02, the target-database fence, backup and restore, a 100-worker claim, the whole vertical, and the separate owner, customer, protected-CI, and runsc gates.

a064 reports, committed raw logs, and `execution/prompts/v01-control-jobs-schema.md` were references. Acceptance here is the fresh Linux smoke and the fresh PostgreSQL probe against the archive.

## Linux arm64, Rust 1.94

Image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, platform `linux/arm64`. Container `oo-a068-rust194-smoke`, bridge network, 8g memory, no published ports, `no-new-privileges`, privileged false, archive mounted read-only, no home mount, no Docker socket (`linux/inspect.json`). `airgap_claim=no`: `cargo fetch` used the network and downloaded the locked graph.

`rustc 1.94.0 (4a4ef493e 2026-03-02)`, commit `4a4ef493e3a1488c6e321570238084b38948f6db`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, Debian 13 (trixie). Toolchain selected with `RUSTUP_TOOLCHAIN=1.94.0`.

Commands, from `execution/prompts/linux-msrv-focused-checks.md`, with no `-p` on fetch:

- `cargo fetch --manifest-path Cargo.toml --locked` phase_exit 0 (`linux/fetch.exit`)
- `cargo test --manifest-path Cargo.toml --locked --offline -p oo-jobs` phase_exit 0 (`linux/test.exit`)
- `cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-jobs --all-targets -- -D warnings` phase_exit 0 (`linux/clippy.exit`)

`cargo test` compiled the package and ran one integration test, `control_schema_files_match_the_published_contract` (1 passed). The library tests and doc-tests were empty. Clippy finished with `-D warnings` and no warning. Container exit 0. One own Rust builder. Before it started, `running_rust_name_match=0`. The a066 PostgreSQL container that was already running was left alone (`raw/builders-before-smoke.txt`).

The Rust test hashes `migrations/0000_bootstrap_roles.sql` and `migrations/0001_jobs_control_expand.sql` from disk with `sha256sum` or `shasum`, compares those digests to the pins in the test, and checks that `contracts/jobs/control-schema/jobs-control-v1.json` contains the same digests. `include_str` is absent (`raw/contract-test-shape.txt`). The test reads the SQL files. It also pins the expected digests, so a byte change fails until the pin, the JSON, and the files move together.

## Checksums and migration

Independent SHA-256 of the archive files, matching `git show` of the same paths and the JSON contract:

| File | SHA-256 |
|---|---|
| `0000_bootstrap_roles.sql` (`jobs-bootstrap/1`) | `445d7248aa252a96a4103ae1b0be0000744eef2c0603037c19df6a0f8eeb0b52` |
| `0001_jobs_control_expand.sql` (`jobs-control/1`) | `b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75` |
| `jobs-control-v1.json` | `d352c86dbf2836360e541a3511af23674ce9692e687a6ac7041d99118dc8c2ca` |

PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1) on `aarch64-unknown-linux-gnu`. Image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Container `oo-a068-pg1710-jobs`, bridge, 2g, pids 512, privileged false, port 5432 unpublished, password supplied by a mode-0600 file mount and `POSTGRES_PASSWORD_FILE` (`pg/inspect.json`). Env keys recorded; env values omitted. The reference `tests/pg_fixture.sh` was not executed, because its container labels belong to a064. The probe `\ir`'d the archive SQL and fed archive `protocol_repeat.psql` the checksum variables.

Probe `pg/out/summary.json`: 117 cases, 0 failed.

Fresh database `oo_control`, after a forced failure of the real migration (both SQL files and the checksum insert, then `OO003`): both product roles absent, `jobs` absent, relation count 0 (`rollback_absent.stdout`). Replay with the real hashes committed. `protocol_repeat` with the real hashes exited 0. The two `schema_migrations` rows, including `applied_at`, were byte-identical before and after, and the 25 `pg_class` rows were byte-identical (`rows_before_repeat.stdout`, `rows_after_repeat.stdout`, `count_before_repeat.stdout`, `count_after_repeat.stdout`). Repeat with an expand digest of 64 zeroes returned `OO001` and left those rows unchanged.

`jobs.service_readiness` with the real expand hash returned `true|true` (`ready_match.stdout`): version marker present, file compare true. The same view compared with the lie digest returned `true|false` (`ready_lie_compare.stdout`). The durable marker exercised here is `schema_migrations` for `jobs-bootstrap/1` and `jobs-control/1`. Backup and restore were not run.

A direct apply that stored digest `ab` repeated 32 times committed. `jobs-control/1` kept that lie, and `schema_version_present` for that version was true (`lie_stored.stdout`). A second session blocked on `pg_advisory_xact_lock(6401202601)` and then failed `42710` (`duplicate_object`, role `oo_jobs_owner` already exists). `protocol_repeat` with the real hashes then returned `OO001`, and the stored checksum stayed the lie (`lie_still_stored.stdout`). After drop of that database and `DROP OWNED` / `DROP ROLE`, both roles were absent (`wipe_probe.stdout` `true|true`). A host check of a contract JSON whose expand hash had been replaced with the lie exited 3 before any SQL.

After the good apply, `protocol_repeat` with the lie digest returned `OO001` and the stored rows stayed the real file hashes (`rows_after_wrong_digest.stdout`). A holder of the advisory lock that slept 6 seconds caused a waiter with `lock_timeout` 2 seconds to return `55P03`. The following repeat with the real hashes exited 0.

Failure of the real SQL left neither a ready marker nor a half-created schema. The SQL itself records whatever 64-hex checksum the caller passes. The supported check hashes the release files and treats readiness as version marker and file compare together, which is what `contracts/jobs/control-schema/README.md` states.

## Permission oracle

Oracle query: `pg/sql/oracle.sql`. Output: `pg/out/oracle.stdout`. Column order for roles is `rolsuper`, `rolinherit`, `rolcreaterole`, `rolcreatedb`, `rolcanlogin`, `rolbypassrls`, `rolreplication`.

`oo_jobs_owner` and `oo_jobs_runtime` are all false on those flags: NOLOGIN privilege groups, NOINHERIT, no role or database creation, no superuser, no bypassrls, no replication. `pg_has_role('oo_jobs_runtime','oo_jobs_owner','member')` is false.

Effective rights were tested as `oo_a068_login` (`LOGIN INHERIT`, member of `oo_jobs_runtime`, `admin_option` false). `session_user` and `current_user` were both `oo_a068_login` (`whoami_login.stdout`). That session ran the legal queue flow without `SET ROLE`: claim, heartbeat, succeed, cancel, fail, retry, second claim, succeed, same dedupe key in two projects, a same-project schedule occurrence, and a control lease (`legal_member.stdout`, exit 0). `SET ROLE oo_jobs_owner` returned `42501`. `GRANT oo_jobs_runtime` returned `42501`. An owner LOGIN member, `oo_a068_owner_login`, also received `42501` when it tried to grant `oo_jobs_owner` onward.

`jobs` schema ACL is owner `UC`, runtime `U`. Public usage on `jobs` is false. `migration_lock` has no runtime grant. `schema_migrations` is runtime `SELECT` only (`migration_insert` false). `task_events` is runtime `ar`. Views `dead_letters`, `task_audit`, and `service_readiness` are `security_invoker` in the SQL and runtime `SELECT` in the catalog. No jobs relation has row level security. The only extension is `plpgsql`.

Shipped functions are owned by `oo_jobs_owner`, `prosecdef` false, `proconfig` `search_path=pg_catalog, jobs`, ACL owner `X` and runtime `X`. `has_function_privilege(public, jobs.enforce_task_update(), EXECUTE)` is false. Runtime `CREATE` on schema `jobs` is false. Runtime database `CREATE` and `TEMP` are false.

Column privileges for `oo_jobs_runtime` on `jobs.tasks`, as `SELECT|INSERT|UPDATE` (`pg/out/column_privileges.stdout`):

| Column | SELECT | INSERT | UPDATE |
|---|---|---|---|
| id, project_id, kind, payload_version, payload, config_version, artifact_digest, dedupe_key, max_attempts | true | true | false |
| run_at | true | true | true |
| state, attempt, lease_owner, lease_until, generation, last_error, updated_at | true | false | true |
| created_at | true | false | false |

A direct insert of a running row is `42501` because `state` and the lease columns are not insertable. `task_events` INSERT is true on every column, including `project_id` and `note`. UPDATE is false on every event column.

Future objects, created with `SET ROLE oo_jobs_owner` from `oo_a068_owner_login` (`current_user` `oo_jobs_owner`, `session_user` `oo_a068_owner_login`): the new function owner is `oo_jobs_owner`, ACL `{oo_jobs_owner=X/oo_jobs_owner,oo_jobs_runtime=X/oo_jobs_owner}`, public execute false, runtime execute true. The new table ACL is NULL, so public and runtime table privileges are false. Both objects were dropped.

A rolled-back demonstration, outside the product migration: `ALTER DEFAULT PRIVILEGES IN SCHEMA` revoke of function execute from PUBLIC left `has_function_privilege(public)` true (`schema_only|true`). The global `ALTER DEFAULT PRIVILEGES` revoke left it false (`global_revoke|false`). The shipped expand file uses the global revoke. That is why owner-created functions in `jobs` do not keep the built-in PUBLIC EXECUTE grant. A schema-only revoke cannot remove that built-in grant.

Search path: a superuser planted `a068_evil.gen_random_uuid` (fixed uuid ending `00ee`) and `a068_evil.clock_timestamp` (year 2000) and granted them to the runtime. The member set `search_path` to `a068_evil, pg_catalog` and inserted. The stored id was `0ebd0b3c-b3d7-4d14-a314-a03d507f7131` and the timestamps were `2026-10-02 00:03:29` (`search_path_poison.stdout`). Column defaults stayed on the real clock. The statement rolled back. Trigger bodies also set `search_path` to `pg_catalog, jobs`.

Adopted definer, planted by the fixture superuser and then dropped: `jobs.a068_adopt()` remained `SECURITY DEFINER` with a NULL ACL, so PUBLIC EXECUTE stayed. The runtime member called it, `current_user` inside the function was `oo_jobs_owner`, and schema `a068_adopted_side` appeared (`adopt_execute.stdout`). The runtime member cannot `CREATE FUNCTION`, so it cannot open this path. An installer that `ALTER OWNER`s a definer into `jobs` bypasses the default-privilege revoke, because default privileges apply to `CREATE`, not to adoption.

`oo_a068_nobody` (LOGIN, `CONNECT` only) received `42501` on the readiness view and on `jobs.tasks`. The runtime member received `42501` on `other_ns.marker`.

## DB constraints, then driver gaps

Legal flow ran first. Every negative below ran as `oo_a068_login` in `--single-transaction`, returned the listed SQLSTATE, and the catalog snapshot after the whole negative set matched the post-legal snapshot (`snapshot_unchanged` true).

`23514` check or trigger: bad state, queued row with a lease, running row without a lease, queued jumped to succeeded, terminal row reopened, generation decreased, generation negative, attempt negative, attempt above `max_attempts`, `max_attempts` 0 and 33, payload over the octet bound, uppercase digest, `last_error` length 2049, heartbeat that changes `lease_owner` without a new generation, lease generation decreased, occurrence project differing from its schedule, occurrence project differing from its task, retry back to queued when `attempt >= max_attempts`.

`23505`: same-project duplicate dedupe key; duplicate `(schedule_id, scheduled_at)`.

`42501`: update of payload, config, or digest; `DROP SCHEMA`; `CREATE TABLE` in `jobs`; `CREATE FUNCTION`; `CREATE ROLE`; `SET ROLE oo_jobs_owner`; grant of the runtime role; `ALTER TABLE`; insert, update, or delete of `schema_migrations`; lock of `migration_lock`; delete or truncate of tasks; update or delete of events; write of `target_fence_certified`; `CREATE EXTENSION`; create in `public`; `CREATE TEMP`; insert of a running task; `ALTER ROLE`; `SET session_replication_role`.

`22003`: generation `9223372036854775807` plus one.

Owner session, `SET ROLE oo_jobs_owner` from the fixture superuser inside the transaction: payload update, delete of a migration row, `target_fence_certified` true, and occurrence update each returned `23514`. The triggers constrain the owner as well as the runtime. A superuser that disables triggers is outside this role contract and was not given a pass.

Overlapping inserts of the same `(project_id, dedupe_key)` as `oo_a068_login`: one transaction committed and the other returned `23505` (`[(0, ''), (3, '23505')]`). One row remained, payload `{"side": "b"}` (`dedupe_race_stored.stdout`).

The same member, each statement committed inside a transaction and then rolled back, so the durable snapshot matched the pre-gap snapshot:

- `lease_until` more than 23 hours ahead was accepted.
- An update claim of a task with `run_at` seven days ahead moved it to `running`.
- `UPDATE jobs.tasks SET state = 'succeeded' WHERE id = ...` with no `lease_owner` or `generation` predicate succeeded a live row.
- A second update replaced `lease_owner` with `worker-thief` and set generation 2 on a live lease.
- Insert of project `33333333-3333-4333-8333-333333333333` succeeded.
- Direct `task_events` insert with note `forged-by-member` stored one row.
- Event project P2 on task `a0680001-0001-4000-8000-000000000008` (a P1 task) stored.

Default 30 second lease, 10 second heartbeat, and old-worker compare-and-swap are driver SQL. Direct runtime `UPDATE` and `INSERT` can ignore them. That is acceptable for this schema-only contract because `jobs-control-v1.json` lists claim function, heartbeat function, `NOTIFY`, target advisory lock, row level security, and queue driver under `not_installed`, and the README leaves the 30s lease, 10s heartbeat, and worker CAS to later work. It means a later driver is bound by its own statements, not by these column grants. AT-CTL-01 and AT-CTL-02 stay unsigned. Control leases in this schema have `exclusivity_domain = control_jobs` and `target_fence_certified` false. They are not a target fence.

`oo_jobs_runtime` is an internal jobs principal. The LOGIN member could not become the owner, could not grant roles, and could not change schema, migration metadata, or grants.

## Findings

None of these block SP02-W01-a. Detail and evidence paths are in `findings.json`.

1. Driver gaps in F1 are real and are already declared outside this contract. They block the later control-loop acceptance.
2. `task_events.project_id` can disagree with `tasks.project_id` (F2). Occurrence insert cannot. A project-scoped audit needs a later rule.
3. The migration SQL trusts the checksum variables it is given (F3). The file-hash check refuses a mutated contract before SQL, and a wrong digest after a good apply is a sticky `OO001`. Callers that only read `schema_version_present` would treat a lied book as the version marker. The contract requires the file compare as well.
4. The runtime member can take advisory key `6401202601` for the length of a transaction (F4). That delays migration. It does not escalate.
5. `ALTER OWNER` of a superuser-created `SECURITY DEFINER` into `jobs` keeps PUBLIC EXECUTE and runs as `oo_jobs_owner` (F5). Owner-created functions do not. Installers have to create routines as `oo_jobs_owner` rather than adopting definers.

`protocol_first` is one transaction, so a forced `OO003` after both files removed the roles and the schema. A second first-apply against an already committed install fails `42710` on `CREATE ROLE` rather than taking the idempotent repeat path. Repeat is `protocol_repeat.psql`.

## Cleanup

Secret scan of the evidence tree against the fixture password file found 0 byte hits across 591 files, including this review. `pg/postgres.log` has no `POSTGRES_PASSWORD=` assignment. The password file mode was `0600`. Full `docker inspect` of the PostgreSQL container was kept only under `/tmp`, did not contain the password, and was deleted with the temp tree. Filtered inspect files store env keys only.

`docker rm -f` removed `oo-a068-rust194-smoke` and `oo-a068-pg1710-jobs`. A following `docker ps -a --filter name=oo-a068-` listed no containers. `/tmp/oo-a068-eef831aa-dba4-4af2-a78b-541bc9e0a068` is gone (`raw/cleanup.txt`). No `docker system prune`, and no foreign container was removed. Probe LOGIN roles had already been dropped inside the database; the product roles existed only in that container.

This review does not commit and does not edit source, Cargo, lockfiles, CI, or specs.
