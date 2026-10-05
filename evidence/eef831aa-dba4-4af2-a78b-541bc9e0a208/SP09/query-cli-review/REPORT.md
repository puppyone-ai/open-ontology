# Report

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a208`. Fixed commit `a661dcdf72ff91561ae6b7cb79916e582884d08a`, parent `84c03d01ce35728924100e42ca142c0ef650ac71`, subject `gateway: align the catalog live test with pre-guard binding`. Query CLI blobs are the same git objects as `4f3f329aa0dabb3f848e6040fdc5379df921c2f9`:

| Path | Git blob |
| --- | --- |
| `crates/oo-gateway/src/bin/query-pg.rs` | `b91238462eb5097de0059ec75e73776ccb2488d0` |
| `crates/oo-gateway/tests/query_pg_cli.rs` | `df797b5c1aa0cbe041222732e9f4d4ebd3094f7e` |
| `contracts/gateway/query-pg-cli-v1.md` | `18d63146589b35ac6192dbf08849ece852cd52e6` |
| `crates/oo-gateway/Cargo.toml` | `8a78503420468aa9f99aee83a5401c42f91ee40a` |
| `Cargo.lock` | `2cc7617c9e06a849de890b4ef28e5f1331988e15` |
| `crates/oo-gateway/src/query_executor.rs` | `90eaf126aefd877111f773d4b2a9012ddab9103b` |

`a661dcdf` changes three paths: `contracts/gateway/policy-catalog-v1.md`, `crates/oo-gateway/src/policy.rs`, and `crates/oo-gateway/tests/policy_catalog_pg_live.rs`. It does not change the executor or the query CLI.

Author evidence: a198 `33e5723e107407d267ab115ca86185145e4dcb6d`, a206 `90dfac17534983e3f3f93d5f437806193c24095b`. Prior catalog review: a200 `a587031c719a484830d78e493605f48e2056cae4`. Executor review a205 stays **REQUEST_CHANGES**. This directory does not reapprove that executor.

`AGENTS.md` is absent. The contract read for this boundary is `contracts/gateway/query-pg-cli-v1.md`. `docs/07-gateway.md` describes the production gateway collector. That collector is outside this CLI. No product source, manifest, lock, or ledger file was edited. a207 remains the connector manifest and root-lock writer.

## What this approval covers

The shipping `query-pg` binary, built from the fixed archive, against one private PostgreSQL 17.10. One fresh fixture (`studio.plate` on `oo_a208`, roles `oo_r_keeper` and `oo_r_reader`). Single request and a stdin session of two turns. TLS to `oo-a208-pg`. Live policy catalog and column ACL. Exact typed cells, a hidden logical column refused before an audit file exists, a physical-column `42501` through the compiled CLI, local audit file order, the contract exit tokens that were asserted, redaction of passwords and physical names, and the trusted input limits in the fresh oracle.

The same proof runs `OO_A190_LIVE=1` for `policy_catalog_pg_live`. That run checks the a206 alignment already in the fixed tree: bound role `clerk`, audit delta `0`, role reuse refused, the next role a fresh `auditor` with one read audit, and digest-scoped generation. It is the alignment check. It is not a new component acceptance.

## What stays open

The executor still binds `oo_r_` before `guard_select`, and a reserved cancel still leaves `cancel_reserved` set (`query_executor.rs` lines 639–644 and 659–666). The CLI does not expose `CancelHandle`. Approving the CLI does not approve the executor.

`PolicyError::StaleGeneration` still documents `current` as the maximum `run_generation` stored for the project (`policy.rs` lines 159–164). The SQL and the `ColumnAclPolicy.generation` comment are source and config digest (`policy.rs` lines 91–94 and 598–603, `policy_catalog.rs` lines 447–461). The fresh fixture behaved as source and digest.

The fixture grants the review login `USAGE` on `_oo`, `SELECT` on `_oo.physical_mappings`, and `EXECUTE` on `_oo.lookup_physical_mapping`. Those grants are fixture setup. The CLI source issues no `GRANT`.

The reader audit observation recorded file order. It did not sample a `Lock` wait. See `FINDINGS.md`.

Natural language, authentication, a production WAL, the full harness, a parent ledger item, and a release stay out of this verdict.

## Builds

Pinned images, both already local, `linux/arm64`, `--pull never`:

- Rust 1.94: `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`.
- PostgreSQL 17.10: `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`.

Smoke container `2809f34a928eebff1596eed83f6a84cff399986d8e11aeb44512c69f82b97a4e`, exit 0, `oom=false`, `privileged=false`, `capdrop=["ALL"]`, network `oo-a208-eef831aa-net`. `linux/smoke/smoke.exit` is 0. Every smoke phase exit is 0, including overlay-absent, prebind (`661: guard.bound_role = Some(request.role.to_string());`), fetch, gateway lib tests, offline `query_pg_cli`, clippy on lib and `query-pg`, and the `OO_A190_LIVE` unset gate. `Cargo.lock` stayed `9608f88b7dbe46674e064f005280768cf9c383cce18827a57a8241056fa03f08`. `cargo update` was not invoked.

Decisive proof container `56575ddbdb1220f03c2343f4fdb13921447a8f855e380f4b7fc6b3b5ef413efd`, exit 0, `oom=false`, `privileged=false`, `capdrop=["ALL"]`, same network. `linux/proof/proof.exit`, `docker-run.exit`, and `launch.exit` are 0. Capacity before that container: `host_cargo=0 rust_builders=1 global=1 cap=6`. This task had one Rust builder. PostgreSQL was the already running `oo-a208-pg` (`pg/up.exit` 0). No host port, no host network, no docker socket, no host `HOME`, no shared cargo cache.

Product bytes in the proof matched the pre-overlay smoke hashes:

```
431424a068a7184e71e9aa18ebf07240a03370e85e3626af2476946c3e196d11  crates/oo-gateway/src/bin/query-pg.rs
db803eb9f8844982a352b6d9148d0cea54d6f81f027988552f6966007cd0a8dc  crates/oo-gateway/tests/query_pg_cli.rs
73f2b9d364622419736d13d6574dde336365b5dc169403f7aeec3669e484ca46  contracts/gateway/query-pg-cli-v1.md
0b04620fe3e65760500e8fb907b49e3fb4689dc5e10e392fed269747e9ddeee3  crates/oo-gateway/Cargo.toml
9608f88b7dbe46674e064f005280768cf9c383cce18827a57a8241056fa03f08  Cargo.lock
2ad166e91a407f5bff39d814d7beb4a1bc69710e30851260528184f3e8558925  crates/oo-gateway/src/query_executor.rs
7557d789219bb0b86a8a071478e2e757df671f0b2636757014c3620c67d2fe60  crates/oo-gateway/src/policy.rs
2920211ec2278d7d74a8022cdad166a11cf97b3eea3d258bb8cf91c131b0bd9d  crates/oo-gateway/src/policy_catalog.rs
8f369ccf19ddbf05853462f2c92c6a615e45e8536fd375ed524f5faae91114e2  crates/oo-gateway/tests/policy_catalog_pg_live.rs
723538406376c9c1a29d1280b7d3560b73adf4b53d04d5f9b0a54361fb4ab2e6  contracts/gateway/policy-catalog-v1.md
```

Proof phases, all 0: toolchain, sha256-tool, secret-scan-tool, overlay-present, product-hash, libclang-reuse, a190-live, a190-oracle, a208-gate, a208-gate-message, a208-live, a208-oracle, clippy-component, clippy, lock-hash, secret-scan.

`a190-live.log`: `policy_catalog_loads_from_postgres_and_executes ... ok`, `1 passed`, `finished in 0.77s`. `a190-oracle.txt` is `keys=present`. `live/oracle.txt` contains `guard_bound_role=clerk`, `guard_audit_delta=0`, `role_reuse=refused`, `fresh_other_role=auditor`, `fresh_read_audit=1`, `done=ok`.

`a208-gate.log`: `OO_A208_LIVE is unset; the review comparison was not run`, `ok`, `finished in 0.00s`.

`a208-live.log`: `review_query_pg_on_a_fresh_fixture ... ok`, `1 passed`, `finished in 11.48s`. `a208-oracle.txt` is `done=ok`.

Clippy: `cargo clippy --locked --offline -p oo-gateway --lib --bin query-pg --test query_pg_cli_a208_review -- -D warnings`. Log ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.36s`. The fresh container installed the clippy component (`info: downloading component clippy`). Lock after the proof is `unchanged 9608f88b7dbe46674e064f005280768cf9c383cce18827a57a8241056fa03f08`.

Secret scan of the live oracles and the proof logs: `passwords=clean`, `private_keys=clean`, `oracle_physical=clean`, `banned_values=clean`, `missing=none`.

## Fresh oracle

`overlay/query_pg_cli_a208_review.rs` is the review oracle. It is copied onto a private archive of `a661dcdf` for the proof. The smoke archive stays without that file. The oracle is Safe Rust. It does not rerun the a198 `OO_A198_LIVE` test.

`live/cli-oracle.txt` is append-only. Lines 1–37 are the prefix left when attempt 003 panicked inside the lock observation, after the guard and limit facts and before any audit fact. The decisive run appended lines 38–97. Those lines end at `done=ok`. Read the second copy.

Decisive facts from that second copy:

- Keeper catalog columns `hue,score,frame,caption,shown`, pending `2`, three rows. JSON string cells for int16 min `-32768`, int32 min `-2147483648`, int64 min `-9223372036854775808`, int64 max `9223372036854775807`. `SELECT *` returns the same three business rows and hides `token`.
- `PGPASSWORD=env-not-used-a208` is ignored. `count(*)` is `3`.
- Reader returns 3 caption rows. Stdin runs 2 turns, 4 audit lines, one backend pid.
- Truncation keeps 1 row and 55 UTF-8 bytes.
- Logical hidden name `token`, a write, and `pg_sleep` each exit 6 with no audit file.
- Password bytes in the request text exit 3 before a new session. A row filter exits 5. A role mismatch exits 5. `max_operators` 53 exits 3. A ninth stdin turn exits 3. An existing audit file exits 7. A non-directory audit parent exits 7. A garbage CA and the wrong CA exit 4. Bare argv exits 2.
- Reader audit: `audit_gap=0`, `audit_admit_seen=1`, `audit_lock_seen=0`, `audit_activity=none/none/active/1`, `audit_exit=0`, `audit_before_query=file_order_only`, `audit_completion=after_unlock`. The success branch required an empty stderr, summary `succeeded`, `end_audit_ok`, and audit records `admit` then `complete`.
- Direct `SET ROLE oo_r_reader` of the physical token column is SQLSTATE `42501`.
- Keeper `SELECT score`, exclusive lock, revoke `SELECT` on the physical score column while admit was present and completion was absent and a Lock wait was visible: `revoke_wait=1`, `hidden_db_exit=8`, `hidden_db_status=failed`, `hidden_db_sqlstate=42501`, `hidden_db_audit_sqlstate=42501`, `hidden_db_42501=1`. Exit 8 is `Fail::Query` (`query-pg.rs` lines 66 and 777–790), whose stderr is `query-pg: query`. The oracle quiet-scanned that stderr and did not store the bytes.
- Generation: digest `a208…` at generation 6 loads; generation 21 on that digest is `StaleGeneration`; other digest `b208…` at generation 6 is `StaleGeneration`; `b208…` at its own maximum 21 is `MappingAbsent`. `max_generation_scope=source_and_digest`.
- Business row digest unchanged: `27fe2ad79d54cb2d9c8f37b73f7dfb183cedc340cbb201ad9483a90a27d8cc3e`. Tuple counts `3,0,0`.

## Preserved attempts

`linux/proof/attempts/` keeps the failed proof runs. The decisive run is the `linux/proof/` directory itself.

- 001: review test did not compile (`Business` would have been `Debug`; secret-scan temporary lifetime). a190 live passed. Docker exit 1.
- 002: a190 passed. Fresh live hit `2BP01` dropping roles while `oo_a190` still held them. Clippy denied `cloned_ref_to_slice_refs`. Docker exit 101.
- 003: a190 passed, gate passed, clippy passed. Live panicked on the conjunction "admit was not visible while the user query waited on the lock". Docker exit 101. That panic wrote the first 37 oracle lines.
- 004: a190 panicked at `policy_catalog_pg_live.rs:1626` because leftover database `oo_a208` still held `oo_r_keeper`. Review test `E0597` on a borrowed audit sqlstate. Docker exit 101.
- 005: a190 passed and the oracle keys were present. Review test `E0308` (`fact_token` received `String`) and `E0507` (`wait_with_output` takes `Child` by value). Docker exit 101.

Those five exits stay in the tree. The later proof exit 0 does not rewrite them.
