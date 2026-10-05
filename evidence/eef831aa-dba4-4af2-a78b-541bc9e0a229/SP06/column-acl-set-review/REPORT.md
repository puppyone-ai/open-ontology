# Column ACL set review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a229`. Review of frozen `543580768dec60d127eb428c09e0a9a400b18272`. Verdict: APPROVE_LIMITED.

This review covers the existing column-ACL installer extension `install_column_acl_set`. It does not open a new component and it does not count as a first review. M1-D14A stays `未验收a218`. The acceptance table and the plan were not edited. SP06-W01 stays open. This file does not accept a parent item and it does not release.

## Commit

Reviewed commit `543580768dec60d127eb428c09e0a9a400b18272`, tree `ce4bb1a38305c672c906801a037af1bcd7fb9fcc`. Subject `gateway: install one column ACL set across mapped tables`. Author evidence is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a218/SP06/column-acl-set/`, including cleanup `819bfade1766ee55c9b35fbefca4bcf3e9c09525`.

Archive blobs matched `git rev-parse` to `git hash-object`:

| Path | Blob | SHA-256 |
|---|---|---|
| `crates/oo-gateway/src/policy.rs` | `84a86e85ef82bc2a459a5e2faef5827d077ca6cc` | `68d6a1639263f6e7083eb44244360dfd22f73897fb3eff2570052da6b508188d` |
| `crates/oo-gateway/src/lib.rs` | `db17b187a886ffbb12642401e1cc2e0fe96a5029` | `3e26f7495f3299e979b33e8cddd820951506afe3155d9c6c569037fa12c07dc7` |
| `crates/oo-gateway/tests/column_acl_set_pg_live.rs` | `69897c46a1f3c5607936b0dffc3a63d3824ce2e4` | `68f96b3ce11c01f612329d150f64c88e1b087982bdd9a5a6e772156fad24d53b` |
| `contracts/gateway/column-acl-set-v1.md` | `1bc75704e7930fea79b743c55a9f0721697fa3e2` | `476cfe39d70b53bd9cf1f62f5047c757e36aa276ff13ee0b62b1600ac17b3366` |
| `Cargo.lock` | `2990f901ea98a7b465f462cce8742bc5c51c13aa` | `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` |
| `crates/oo-gateway/Cargo.toml` | `8a78503420468aa9f99aee83a5401c42f91ee40a` | `0b04620fe3e65760500e8fb907b49e3fb4689dc5e10e392fed269747e9ddeee3` |

`lib.rs` does not export the set API. The proof reached it through `oo_gateway::policy`. Product bytes in the proof copy matched the smoke hashes. `Cargo.lock` was unchanged. No source, library, dependency, or lock edit was made in this review.

## Recipe

Launch and hash tools start from the reviewed a200, a204, and a199 recipes recorded in `origin/recipes.txt`. The a229 copies change the names, the fixed commit, the 6 GiB and 1 CPU builder limits, the global cap of 10, and the review test. `pg/pg-wrap.sh`, `tools/sha256.rs`, and `tools/select-debs.rs` are byte-identical to a200. Harness commit `d7f640d01409b7b476f34bd14b6504f687cacc55`.

## Builders

One Rust builder at a time and one private PostgreSQL. Pinned images, `--pull never`:

- Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)` with `RUSTUP_TOOLCHAIN=1.94.0`.
- PostgreSQL `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, linux/arm64.

Smoke container `e17e4fd92441010a5b2226f738da55b895b6f36d96f656a77665229bd8277db6`. Exit 0. `OOMKilled=false`. `Privileged=false`. `CapDrop=["ALL"]`. `Memory=6442450944`. `NanoCpus=1000000000`. `PortBindings={}`. Network `oo-a229-eef831aa-net`. `policy::tests`: `ok. 6 passed`. Clippy was deferred. The review test was absent from the archive.

Proof container `0a0d07d8b59945291672226f3d54275ec20b06adc1c5fa11cdd8400699ac2f4c`. Exit 0. Same memory, CPU, capability, privilege, and port limits. `policy::tests` again `ok. 6 passed`. Live test `ok. 1 passed` in 0.93s. Clippy `-D warnings` on `--lib`, `--test column_acl_set_a229_review`, and `--test column_acl_set_pg_live` finished in 1m 27s, exit 0. The author live test was linted and was not run.

PostgreSQL container `b306c0a3a7f34b56e8763b027d852d33eaab5ceb52201308a3ade3940f09d688`, name `oo-a229-pg`, 1 GiB, 1 CPU, no host port, certificate DNS `oo-a229-pg`. The same database was reused for every proof attempt.

## Fixture

Independent of the author fixture. Database `oo_a229`. Tables `depot.counter` and `depot.slip`. Roles `scribe` and `glance`, plus the refusal roles in `live/oracle.txt`. Config digest is `a229` repeated, other digest `b229` repeated. Current generation 7. Other-digest generation 11. Passwords stayed in `/tmp/oo-a229-eef831aa-dba4-4af2-a78b-541bc9e0a229/secrets`. Request JSON uses `password_file` and does not contain the password.

The login `USAGE` / `SELECT` / `EXECUTE` grant on `_oo.physical_mappings` and `lookup_physical_mapping` is a fixture grant applied after the column-grant checks, so `query-pg` can load the catalog as the `NOINHERIT` login. Oracle `loader_mapping_grant=fixture_login_not_installer`. `install_column_acl_set` does not issue that grant.

## Live result

`live/oracle.txt` is the green run.

Pure refusals recorded `empty_duplicate_mixed_filter_metadata_unknown_bounds` with `pure_grant_events=0`. A second policy with no mapping returned `MappingAbsent`, role absent, zero grants, zero grant events, timing `pre_create_role`. A second policy whose column is `float4` returned `TypeRefused` on `ratio`, with the same absence and `pre_create_role`. Those two cases return before `CREATE ROLE`. They are not the post-grant rollback.

Digest A stayed at generation 7 while digest B was 11. A stale generation on digest A returned `StaleGeneration` with zero grant events. A missing login returned `LoginAbsent` with zero grant events.

The old single-table API on an existing role and the other physical table returned `GrantMismatch`. A set install onto that partial role returned `GrantMismatch`. The first table stayed at one granted column.

`scribe` and `glance` recorded `Installed`. Exact scribe replay recorded `Replay`. A later one-table `install_column_acl` replay left the slip grants unchanged. A partial list and a changed column returned `GrantMismatch`. An extra `ratio` grant was still present after `GrantMismatch` (`unexpected_grant=left_in_place`); the review test then revoked it. A second member returned `MembershipMismatch` and the grant text matched the earlier snapshot.

Role shape was `NOLOGIN`. Membership was inherit false, set true, admin false, grantor `oo_fixture_admin`. Table privileges were 0. `_oo_pos` was not selectable. Schema `_oo` had `USAGE` without `CREATE`. Publication count was 0. `reltuples` was unknown until the test ran `ANALYZE`.

`query-pg` scribe and glance exited 0, including `count(*)` and star. Glance output omitted the hidden values. The guard exited 6 with stderr `query-pg: guard` and no audit file. Direct hidden reads, writes, and raw `SELECT *` were `42501`. The reader backend pid stayed the same across those denials.

## Post-grant fault

A public-schema event trigger on `GRANT` counts commands with `nextval`, which survives rollback. The successful scribe install produced `installed_grant_events=4`. That is two tables, each `GRANT USAGE` on `_oo` plus one column `SELECT`. The membership `GRANT role TO login` did not increment the counter.

The fault install used a new role and `fail_at=4`. The server raised `P0001` on grant 4. `PolicyError::Database.message` is `db error`, which is `tokio_postgres::Error`'s display text and does not include the server exception. `pg_event_trigger_ddl_commands()` reported `object_type=TABLE` and left `objid`, `object_identity`, and `relname` empty. `current_query()` was the simple-query batch:

```text
GRANT USAGE ON SCHEMA "_oo" TO "oo_r_fault";
GRANT SELECT ("pc_…") ON TABLE "_oo"."pt_…" TO "oo_r_fault"
```

That `pt_` name is the second table, so `a229_second_hit` advanced to 1. After the error, `oo_r_fault` was absent, its column privileges were 0, the scribe grants were unchanged, and the admin backend still answered `SELECT 1`. Oracle: `post_grant_events=4`, `post_grant_role=absent`, `post_grant_grants=0`, `post_grant_timing=after_earlier_grants`, `same_backend_after_fault=true`. The raw line is `linux/proof/postgres-fault.log`.

## Preserved attempts

| Path | Exit | What stopped |
|---|---:|---|
| `linux/proof/attempt-1/proof.exit` | 101 | Panic at the message check. Sqlstate was already `P0001`. The product message is `db error`, so the server text was not visible to the test. Container `21617e11178dbd43655410ea3995b3ec6ec82a57701a9b6dbb4593484b2fb42b`. |
| `linux/proof/attempt-2/proof.exit` | 101 | `P0001` and `db error` passed. `a229_second_hit` stayed 0 because the event row did not carry `relname`. Rollback assertions had not run. Container `3e30fb467916`. |
| `linux/proof/proof.exit` | 0 | Green run above. |

Both failed attempts kept the pre-create-role mapping and type facts at zero grant events. Neither attempt is a surviving role.

## Open

Authentication, row-level security, publication, and the audit WAL remain open. The observed grantor is the fixture superuser `oo_fixture_admin`. A non-superuser grantor was not exercised. That gap is recorded as `nonsuperuser_grantor=not_proven` and is not a mandatory gate for a later acceptance. The installer does not write `pg_publication`, does not write an audit row, and does not `ANALYZE`.
