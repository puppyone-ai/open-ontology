# Catalog live test alignment

Session `eef831aa-dba4-4af2-a78b-541bc9e0a206`. Source commit `a661dcdf72ff91561ae6b7cb79916e582884d08a`. Parent `84c03d01ce35728924100e42ca142c0ef650ac71`. The live proof compiled the product tree of `5e3ba29f8d460a1aa6919733100c34a68a2fdb75`. Between that tree and the source commit, `query_executor.rs`, `policy_catalog.rs`, and `Cargo.lock` did not change. This commit changes the catalog live test, one generation comment, and the catalog contract.

The old a190 archive executor was not run again. Production binding was not moved. Role checks were not weakened to `None`. `ensure_committed` was not rewritten. No release is claimed.

## What changed

`policy_catalog_pg_live.rs` `deny_hidden_write_and_injection` used to require `bound_role()` to be empty after each guard refusal. On the current executor a valid `oo_r_` role is stored before `guard_select`. The test now requires `oo_r_clerk` after those refusals, requires the in-memory audit counters to stay put, requires an auditor statement on that session to return `RoleReuse`, and requires a new session to bind `oo_r_auditor` and read `amount`.

`ColumnAclPolicy.generation` now says the value must equal the maximum `run_generation` for that source and config digest. `contracts/gateway/policy-catalog-v1.md` says the same scope, and it says a null `pg_current_xact_id_if_assigned()` is not proof the session is idle.

## Host and images

Linux arm64 Rust 1.94.0 (`4a4ef493e 2026-03-02`), image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, container `oo-a206-pg`, network alias `oo-a190-pg`, database `oo_a190`. No host port. Secrets stayed under `/tmp/oo-a206-eef831aa-dba4-4af2-a78b-541bc9e0a206/secrets`.

Each rust container had 8 GiB, all capabilities dropped, bridge network `oo-a206-eef831aa-net`, and private cargo and target volumes. Postgres had 1 GiB and the capabilities CHOWN, DAC_OVERRIDE, FOWNER, SETUID, and SETGID. One rust builder at a time. One postgres. Global builder count at smoke start was 0, under the cap of 6. `cargo update` was not run. Lock content hash stayed `9608f88b7dbe46674e064f005280768cf9c383cce18827a57a8241056fa03f08`.

## Commands and exits

Pre-edit smoke, container `oo-a206-smoke`, docker exit 0. `linux/smoke/phases/`.

| Phase | Exit |
| --- | --- |
| `cargo test --locked --offline -p oo-gateway --lib` | 0, 17 passed |
| `cargo clippy --locked --offline -p oo-gateway --lib -- -D warnings` | 0 |
| `cargo test --test policy_catalog_pg_live` with `OO_A190_LIVE` unset | 0 |

The unset run printed `OO_A190_LIVE is unset; the policy catalog comparison was not run` and did not enter the fixture. That cargo `ok` is the gate returning. It is not a live pass. `linux/smoke/gated-note.txt`.

Pre-edit live, container `oo-a206-red`, `OO_A190_LIVE=1`. Cargo exit 101. `linux/red/phases/live-test.exit`. The harness recorded the expected failure and its own exit is 0. `linux/red/reproduced.txt` says `yes line=575 left=Some(oo_r_clerk) right=None exit=101`. Partial facts are `live/red-oracle.txt`. They stop at `auditor_rows=3` and do not contain `done=ok`.

Post-edit attempt 1, docker exit 1. `rustfmt --check` on `policy.rs` wanted to rewrite pre-existing lines. Those lines were left as committed. The log is `linux/green/attempt-1/`.

Post-edit attempt 2, container `oo-a206-green`, docker exit 0. `linux/green/phases/`.

| Phase | Exit |
| --- | --- |
| `rustfmt --check` on the live test | 0 |
| `cargo test -p oo-gateway --lib` | 0, 17 passed |
| `cargo clippy -p oo-gateway --lib -- -D warnings` | 0 |
| `cargo clippy --test policy_catalog_pg_live -- -D warnings` | 0 |
| gated test, `OO_A190_LIVE` unset | 0, comparison not run |
| `OO_A190_LIVE=1` `policy_catalog_pg_live` | 0, 1.09s |

Green oracle `live/green-oracle.txt` and `live/oracle.txt`. Keys `guard_bound_role=clerk`, `guard_audit_delta=0`, `role_reuse=refused`, `fresh_other_role=auditor`, `fresh_read_audit=1`, `done=ok`. Direct secret, insert, and extra returned SQLSTATE `42501`. Mapping count stayed 16.

## Evidence

Raw exits are the `phases/*.exit` files and the docker exit files. Images and capability sets are under `linux/smoke/`, `pg/`, and `linux/green/inspect.txt`. Oracles are under `live/`. Source bytes are `source/blobs/`. `SHA256SUMS` is the Rust tool `tools/sha256.rs` over this directory. Cleanup is `cleanup/run.log`.
