# a085 review — SP04-W05-a target fence and apply receipts

Decision: **request_changes** for SP04-W05-a, the database foundation only.

Commit `83704982a8d9459c51ed7c2020c51d21c91fb8cd`, tree `494a0f015c35e3eb75548e9d4dad71bb28d0acd7`, parent `78d106fcb3e7e1705b617ed6df58933d85297e7b`. Subject: pipeline: add the target fence and apply-receipt schema. Acceptance used a git archive of that commit. Workspace HEAD at archive time was `911f317cde6d87ec49c247afc35681c7c64d5c91` and was left untouched. a083 owns the main root.

The fence, role split, replay, conflict rollback, and from-empty install hold on PostgreSQL 17.10. The closed segment rule does not. A runtime member stored three receipts whose segment index, segment count, and bitmap do not match either legal shape. `commit_apply_receipt` returned `inserted`. That blocks this slice.

Full Loader, CDC, control mirror, ACK, 100 workers, physical name mapping, a real vertical, dual arch, runsc, customer data, protected CI, and release remain pending. This review does not sign them.

## Identity and inputs

| Item | Value |
| --- | --- |
| Commit | `83704982a8d9459c51ed7c2020c51d21c91fb8cd` |
| Tree | `494a0f015c35e3eb75548e9d4dad71bb28d0acd7` |
| Parent | `78d106fcb3e7e1705b617ed6df58933d85297e7b` |
| Archive tar SHA-256 | `df0a6de227b29333d60ccc0c39a73297b42eafe25d5afbb52b86aa25e3d29824` |
| Archive file count | 325 (Cargo.toml, Cargo.lock, rust-toolchain.toml, crates, contracts/pipeline/target-fence-receipts) |
| Bootstrap SHA-256 | `37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208` |
| Expand SHA-256 | `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9` |
| Contract JSON SHA-256 | `429faa9d32e4b42960e805e0883a2f67c8783d936e9094f0747e9c5e21a23521` |

The three hashes match the published contract pins. Files are in `raw/`.

## Linux arm64, Rust 1.94

One builder. Image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, platform `linux/arm64`, `--pull never`. Container `oo-a085-msrv-c4e1a085-7b21-4d55-9c10-a085fence01`. Private bridge, tmpfs `/tmp`, named cargo and target volumes, read-only archive at `/src`, `HOME=/tmp`, no host port, no socket, no shared host cache, `--cap-drop ALL`, no capability added, `no-new-privileges`, not privileged. `hostconfig.txt` records that posture.

Before launch, host `cargo`/`rustc`/`rustdoc`/`clippy-driver` count was 0. One foreign rust-builder was already up: `oo-a084-tools2-7402787d-72a5-4328-ad9f-269f55dddb4c` (owner a084). That occupied one of the two global slots, so this review started the second. `oo-a080-msrv-f669cc94` was up with comm `sleep` (docker-init pid 35357, sleep pid 35373, elapsed 53:50). It has no `oo.kind=rust-builder` label and was not counted as a builder.

`RUSTUP_TOOLCHAIN=1.94.0`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, Debian 13. The image did not already contain clippy, so `rustup component add clippy --toolchain 1.94.0` downloaded it. `airgap_claim=no`.

| Phase | Exit |
| --- | --- |
| rustup component add clippy | 0 |
| `cargo fetch --manifest-path Cargo.toml --locked` | 0 |
| `cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline` | 0 |
| `cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings` | 0 |
| docker run | 0 |

`target_fence_files_match_the_published_contract` passed. Library and doc tests were empty. Elapsed 26 seconds. Clippy finished with no warning. The contract test checks that the source line after `SECURITY DEFINER` contains `SET search_path`. Catalog `proconfig` is the runtime proof and is recorded below.

## PostgreSQL 17.10 fixture

One fixture. Image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `--pull never`, `--data-checksums`, `scram-sha-256`, no host port. Container `oo-a085-pg1710-b7d2a085-6c18-4a31-8e44-a085pg0001`. Labels `oo.owner=a085`, `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a085`, `oo.kind=postgres`. Privileged false. Capabilities: drop ALL, add CHOWN, DAC_OVERRIDE, FOWNER, SETGID, SETUID. Private bridge. `postgres-before.txt` listed no other running `oo.kind=postgres` container. No foreign database was reset.

`SELECT version()` returned PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1) on aarch64. `data_checksums` is `on`.

Passwords were generated inside the container and stayed there. Evidence records byte counts and SQLSTATE values. Two earlier driver attempts failed before any login existed (`pg/attempts/driver-auth/`): a shell brace with no space, then a mode-700 directory the server user could not traverse. Those attempts are driver history. The recorded suite is the third run, after `chmod 755` on that directory and `relpersistence::text` casts in the catalog query. The third run drops and recreates only this fixture's databases, and drops the roles this fixture created, after the container identity check.

Recorded suite: `pg/out/summary.json`, 144 cases, 16 failures. Follow-up on the same database: `pg/out/supplement-summary.json`, 28 cases, 0 harness failures. The segment follow-ups are observations that stored rows. A stored illegal row is a product defect even though `observe` marks the harness line ok.

## Catalog and grants

Roles `oo_target_owner` and `oo_pipeline_runtime` are NOLOGIN, NOSUPERUSER, NOCREATEDB, NOCREATEROLE, NOINHERIT, NOBYPASSRLS, NOREPLICATION. Runtime is not a member of the owner (`pg_has_role` false). Bootstrap does not grant the owner role to runtime.

Login roles used only by this review:

| Login | Inheritance | Membership | search_path |
| --- | --- | --- | --- |
| `oo_a085_runtime` | INHERIT, SET FALSE | `oo_pipeline_runtime` | `pg_catalog` |
| `oo_a085_worker` | INHERIT, SET FALSE | `oo_pipeline_runtime` | `pg_catalog` |
| `oo_a085_owner` | NOINHERIT, SET TRUE | `oo_target_owner` | `pg_catalog` |
| `oo_a085_gateway`, `oo_a085_agent` | none | none | connection refused, SQLSTATE 42501 |
| `oo_a085_nobody` | CONNECT only | none | `schema_ready` SQLSTATE 42501 |

Runtime database privileges: TEMP false, CREATE false, schema CREATE false. PUBLIC TEMP is false. Owner has CONNECT, CREATE, and TEMP from bootstrap.

Every `pg_advisory%`, `pg_try_advisory%`, and `set_config(text, text, boolean)` shows `public=false` and `runtime=false`. Owner holds EXECUTE on `pg_try_advisory_lock(int, int)`, `pg_advisory_unlock(int, int)`, `pg_advisory_xact_lock(bigint)`, and `set_config(text, text, boolean)`.

Runtime EXECUTE is limited to `schema_ready`, `adopt_target_generation`, `renew_target_generation`, `fail_target_generation`, `assert_target_fence`, and `commit_apply_receipt`. `lock_key`, `_allow_write`, `_session_nonce`, and the reject triggers are revoked from runtime and PUBLIC.

SECURITY DEFINER functions and their `proconfig`:

| Function | search_path |
| --- | --- |
| `_allow_write`, `_backend_holds_lock`, `_bind_session`, `_session_nonce` | `pg_catalog` |
| `_load_held_generation`, `adopt_target_generation`, `assert_target_fence`, `commit_apply_receipt`, `fail_target_generation`, `renew_target_generation` | `pg_catalog, _oo` |

Invokers `_require_digest`, `_require_position`, `_require_positive_generation`, `_require_shard`, `lock_key`, `reject_direct_write`, and `reject_migration_mutation` also set `search_path=pg_catalog`. `schema_ready` is an invoker with `pg_catalog, _oo`.

Four ordinary tables are `relpersistence=p`, owner `oo_target_owner`, RLS off: `schema_migrations`, `migration_lock`, `target_generations`, `apply_receipts`. `pg_publication` count is 0. `oo_control` and `oo_agent_bare` have no `_oo` schema. Function bodies in `_oo` contain zero matches for `table_freshness`, `source_watermark`, `jobs.leases`, or `6401202601`.

`target_generations` columns are `shard_id, generation, state, owner_token, session_nonce, lock_key1, lock_key2, backend_pid, adopted_at, renewed_at, failed_at`. There is no `backend_start` column.

## Independent cases that held

From-empty apply of bootstrap and expand on `oo_agent` committed. A prior transaction on `oo_rollback` raised `OO003` during expand. After that rollback, `_oo` was absent and both product roles were gone (role count 0). `CREATE ROLE` participated in the transaction on this PostgreSQL 17.10. A second database bootstrap then raised `42710` because the roles already existed from the successful `oo_agent` apply, and `oo_second` stayed without `_oo`.

`schema_ready` was false and `service_readiness` returned no row before bookkeeping. After the published checksums were inserted, `schema_ready` of the expand hash was true, `schema_ready` of 64 zeroes was false, and the view showed the stored expand hash. `protocol_repeat` with the real hashes committed and left the rows unchanged. `protocol_repeat` with a zero expand hash raised `OO001` and left both stored checksums as the real file hashes.

On a separate database, `oo_agent_lie`, both migration rows were stored as `ef` repeated 32 times. `schema_ready` of that lie was true, `schema_ready` of the real expand hash was false, and `schema_version_present` was true. The stored caller checksum is the bookkeeping value. It is not a hash the server computed from the files.

Runtime denials, each SQLSTATE 42501: `set_config`, both advisory try and xact lock, `pg_advisory_unlock_all`, direct generation INSERT, TRUNCATE, CREATE TEMP, CREATE FUNCTION, `SET ROLE` to the owner and to the runtime group, `session_replication_role=replica`, `_allow_write`, `_session_nonce`, `lock_key`, SELECT on generations and receipts, and migration INSERT. `SET _oo.writer = 'apply-receipt'` succeeded for the runtime login. The custom GUC is writable. Table privileges still stop the INSERT. Owner direct receipt INSERT and owner DELETE of `schema_migrations` raised `OO014`.

Closed receipt values that committed on one adoption: all four codecs, position 0 with the partial completion triple, position `9` repeated 38 times, a one-byte bitmap with index 0 and count 1, batch length 128, and source and table-generation length 63. Rejections: bad codec and lineage `OO015`, uppercase digest `23514`, fractional and negative and `10^38` positions `22003`, illegal completion triple `23514`, 8193-byte bitmap `23514`, NULL bitmap `23514`, source length 64 `23514`, batch length 129 `23514`, generation 0 and -1 `22003`, epoch -1 `22003`, shard containing a NUL `22021`, uppercase shard `23514`.

Same-backend renew of `a085_renew` stayed at generation 1, state `adopted`. Same-backend call with a second owner token raised `OO017` and left generation 1 with the first token. A receipt with the other token, and a receipt with generation 99, raised `OO013`.

A committed `assert_target_fence` did not authorize a later transaction. The same session then rolled back `fail_target_generation` and the following receipt raised `OO011`. After that rollback the generation row was still `adopted`. On `a085_rel`, the rolled-back fail left the temp nonce in place and the session advisory count at 0, and the receipt raised `OO011`. The unlock survived the rollback. The session row came back with it.

Replay after fail and takeover: the receipt row stayed one row, payload `ab` repeated 32 times, `fence_generation` 1, `target_commit_at` `2026-10-02 02:07:47.080845+00`. The shard moved to generation 2 and owner `22222222-2222-4222-8222-222222222222`. A changed payload, codec, lineage, or hook digest raised `OO010`. The conflict transaction had inserted side rows first. Those side rows are absent and the stored payload is the original digest. A replay that follows an insert in the same transaction commits that insert. Replay is a successful return, so earlier statements in the caller's transaction remain the caller's obligation.

Receipt-only batch `batch-receipt-only` is `0|0|1` across the side data table, the side quality table, and `apply_receipts`. A second login then inserted into `fixture_future.note` with no fence. That grant is ordinary table DML.

`shard_0` lock key1 is `-1780614094`. Runtime adopt and receipt on that shard committed at generation 1. `SELECT ((-1)::integer)::oid` returned `4294967295`. The negative int4 key used by this shard did not fail the oid cast inside the product function.

Search-path poisoning created `a085_evil.clock_timestamp` (year 1999), `a085_evil.gen_random_uuid` (a fixed uuid), and `_oo.clock_timestamp` (year 1998). The runtime session set `search_path` to that schema first. The stored adoption year is 2026 and the nonce is `f977baca-392a-4f3a-aad9-9fbeac318478`.

An owner session created `pg_temp.oo_target_fence_session` with a BEFORE trigger that rewrites the nonce to `00000000-0000-4000-8000-00000000a085`. Adopt returned 1. The temp row shows the rewritten nonce. The stored nonce is `30c1e4fc-a3d4-4aa5-a3e1-018afb588e66`. The receipt in that same session raised `OO011`. A later owner connection, after `SET ROLE oo_target_owner`, also raised `OO011`. A `public.oo_target_fence_session` decoy did not stop adopt of another shard. An admin session that copied the known nonce into its own temp table and called the receipt raised `OO011`.

A superuser disabled the user trigger, inserted `a085_decoy` with the lock keys of `a085_victim`, and re-enabled the trigger. Runtime adopt of `a085_victim` raised `OO020`. The decoy was then removed the same way. Runtime cannot disable the trigger.

Generation forced to `9223372036854775807` made the next adopt raise `22003`. The row stayed at that value. Owner `DELETE` of `migration_lock` inside a rolled-back transaction succeeded: that table has no immutability trigger. Owner `_allow_write()` plus an INSERT of a generation row, inside one rolled-back transaction, succeeded and left no row. The same gate plus `SET generation = generation - 1` in one transaction raised `22003`, and the adopt in that transaction rolled back with it. An ungated UPDATE in a later autocommit statement raised `OO014`, because `set_config(..., true)` is transaction-local. A superuser `DISABLE TRIGGER USER` inserted a receipt inside a rolled-back transaction. After rollback the row was absent. Triggers are not a boundary against the table owner or a superuser.

`oo_a085_worker` adopted a free shard using the token `11111111-1111-4111-8111-111111111111`. The stored `owner_token` is that value. The login name is not the token. After the `a085_dead` session ended, takeover moved the shard from generation 1, pid 7020, owner token A, to generation 2, pid 7409, owner token B.

Follow-up, with `application_name` visible in `pg_stat_activity` while `pg_sleep` ran:

| Case | Result |
| --- | --- |
| Second login adopts the shard the first backend still holds | `55P03` |
| That same moment, adopt of a different shard | generation acquired, exit 0 |
| Second session `pg_advisory_xact_lock(6404202605)` while the first holds it, `lock_timeout` 2s | `55P03` in 2.047s |
| Terminate backend during `pg_sleep` before `COMMIT` of a receipt | `57P01`, receipt count 0 |
| Terminate backend after `COMMIT`, during a later sleep | `57P01`, receipt count 1 |
| Other shard, same receipt identity, while the first transaction holds the row, `lock_timeout` 2s | `55P03` in 2.086s, stored payload still the first digest, count 1 |

The first suite's comment markers never appeared in `pg_stat_activity` (`poll-a085left.stdout` count 0). Its same-shard, migration-waiter, and kill cases therefore ran after the sleep ended or matched no backend. Those sixteen summary failures are the marker misses, the stale-generation segment call, the ungated owner UPDATE, and the owner receipt without `SET ROLE`. The follow-up replaced the overlap and kill evidence. The segment result below is the product defect.

## Blocking defect

`commit_apply_receipt` rejects a segment only when this expression is boolean true:

```
(count IS NULL AND index IS NULL AND octet_length(bitmap) = 0)
OR (count >= 1 AND index >= 0 AND index < count AND octet_length(bitmap) BETWEEN 1 AND 8192)
```

The table CHECK has the same shape. A NULL comparison makes the OR unknown. `IF NOT (unknown)` does not raise. A CHECK constraint accepts unknown. Three fresh shards, generation 1, runtime login, each returned `inserted`:

| Batch | segment_index | segment_count | bitmap bytes |
| --- | --- | --- | --- |
| `seg-null-index` | null | 1 | 1 |
| `seg-null-count` | 0 | null | 1 |
| `seg-null-both` | null | null | 1 |

Evidence: `pg/out/sup_seg_null_index.stdout`, `sup_seg_null_index_row.stdout`, and the matching `sup_seg_null_count*` and `sup_seg_null_both*` files. A fully NULL bitmap still raises `23514` because that argument is tested with `IS NULL` before this expression. An 8193-byte bitmap raises `23514`. The hole is the mixed or both-null index with a non-empty bitmap.

The main-suite case `rej_segment` sent index NULL and count 1 on shard `a085_rej` after earlier rejects had incremented that shard, and it passed generation 1. The segment expression did not raise, and the fence then raised `OO013`. That SQLSTATE is the stale generation. The follow-up on fresh shards shows the row is stored when the generation matches.

## Residuals that stay open

- Arbitrary future-table DML is outside the fence. The receipt-only call records 0 data rows, 0 quality rows, and 1 receipt. Loader, CDC, reject events, control mirror, ACK, and XTRIM are not in this schema.
- The receipt primary key is `(source, table_generation, txn_or_batch_id)`. It is not shard-scoped. Two shards writing one identity share one row lock.
- Replay returns success, so earlier DML in the same transaction commits with it.
- `schema_ready` compares the caller argument to the stored expand checksum. A lie stored by the owner makes the function and the view report ready.
- `migration_lock` has no immutability trigger. Owner DELETE of the singleton can succeed.
- Owner `_allow_write()` opens generation and receipt writes for that transaction. Superuser `DISABLE TRIGGER` bypasses `OO014`.
- Runtime can `SET _oo.writer`. It still lacks INSERT on the tables.
- `owner_token` is a value the caller presents. A second LOGIN member stored the first member's token. The schema does not bind the token to an external worker identity.
- Renewal checks `backend_pid` together with the lock, the nonce, and the owner. Receipt load checks the lock and the nonce. There is no stored backend start time.
- A second database's bootstrap raises `42710` once the roles exist. Roles are cluster-global.
- The contract test's search_path check is the next source line. The catalog `proconfig` rows above are the installed configuration.
- Governance: `governance/check_governance.py` and `governance/ownership-map.md` exist on the working tree and are not in commit `83704982`. `git cat-file` exits 128 for both paths. Their absence is recorded here. It is not a statement about the schema grants.

## Author evidence, read and not reblessed

a072 `REPORT.md` and `HANDOFF.md` were read. The pre-smoke invocation pin is commit `0bba2a7993e6b53b1bb7c587e6e978bc6f8133dc`, tree `93aedfb3a6b126cbf0c9e7bb6833e41bb7d14f3e`. `pre-smoke/baseline-head.txt` is `2b68c89934d4ac1a73467ef563b14a895d6c3778`. That commit's tree is `97574ee62ac09d98e0512bae8bfecc31f1f0d879`, which is `baseline-tree.txt`. `0bba2a79` is an ancestor of `2b68c899`. The report says HEAD moved after the archive pin. The baseline files and the invocation pin disagree, and the report says so.

The post-smoke snapshot file is one line, `0bd518b93e23115ba12c428721531294808fc5dca8cbd40d094f31ab53684de0  -`, the hash of empty stdin. It is not a per-file inventory of commit `83704982`. This review compiled the archive of `83704982`.

First author container `oo-a072-pg1710-7c1a0e2b-4f63-4b0a-9c2d-11a072fence01`. `fixture/harness.log` keeps FAIL lines and later `resume=connect_without_reset` with an in-place function replace. That pass is not a from-empty apply of the final expand hash. The preserved `42703` is `fixture/attempts/fence-flow-new-field/fence_flow.exit` = 3. stderr is `record "new" has no field "generation"` in `reject_direct_write` during a receipt INSERT, plus notice `42P07` that `oo_target_fence_session` already exists. The exit file is still 3.

Second container `oo-a072-pg1710-2c451bc0-b35f-4f27-beac-d8d2d3987a40`. `fixture-fresh/harness.log` starts with the published expand hash and ends `fixture_result=pass`, including `ok reset` and `ok apply`. Receipt timestamps in the first fixture are `2026-10-02 01:21:02+00`. Fresh receipt timestamps are `2026-10-02 01:26:10+00`. No container `StartedAt` or `FinishedAt` survived in the evidence read here, so this review does not claim the first container was gone before the second started. The author report lists two fixtures. The max-1-per-task overlap is missing proof. This review ran one PostgreSQL container.

## Commands

```
evidence/eef831aa-dba4-4af2-a78b-541bc9e0a085/SP04/target-fence-review/linux/launch.sh
evidence/eef831aa-dba4-4af2-a78b-541bc9e0a085/SP04/target-fence-review/pg/provision.sh
python3 evidence/eef831aa-dba4-4af2-a78b-541bc9e0a085/SP04/target-fence-review/pg/run_probes.py
python3 evidence/eef831aa-dba4-4af2-a78b-541bc9e0a085/SP04/target-fence-review/pg/supplement.py
```

Linux exits are all 0, listed above. Probe process exit is 1 because the summary contains 16 harness failures. Supplement process exit is 0. The segment rows were still inserted. Cleanup of this review's containers, networks, volumes, and `/tmp/oo-a085-eef831aa-dba4-4af2-a78b-541bc9e0a085` is in `cleanup.txt`. No prune, no commit, no push, no pull.

## Pending

SP04-W05 as a whole, AT-PL-05, the Loader, CDC, the control mirror, ACK, 100 workers, physical name mapping, a real vertical, dual architecture, runsc, customer data, protected CI, and release are unsigned. No source file was edited. No dependency was added.
