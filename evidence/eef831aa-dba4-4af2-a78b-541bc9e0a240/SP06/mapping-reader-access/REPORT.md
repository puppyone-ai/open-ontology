# Mapping reader access

Development record for UUID `eef831aa-dba4-4af2-a78b-541bc9e0a240`. Fixed archive `df0bce2e235babc0944b68350921c13038741ab9`. This run provisioned mapping read for one explicit login. It does not accept a parent plan item, does not close SP06-W01, and does not release.

## Result

`provision_mapping_reader` grants one login `USAGE` on `_oo`, `SELECT` on `_oo.physical_mappings`, and `EXECUTE` on `_oo.lookup_physical_mapping(text, text, bigint, text, text, text, text)`. It does not grant a shadow column, `EXECUTE` on `register_physical_mapping`, `PUBLIC`, a source object, or membership in `oo_target_owner`.

The final private cluster recorded:

- `init-pg` exit 0, `install=bootstrapped`, `full_import=installed`, `historical_v2=false`
- import before init exit 1, stderr `target schema is not ready`
- `import-pg` exit 0, `state=awaiting_publish`, `table_count=2`, `source_generation=7`
- column ACL for `oo_r_clerk` and `oo_r_peek` installed with mapping privileges still absent
- `load_policy_catalog` as the login returned SQLSTATE `42501` before provisioning
- provisioning effect `Installed`, replay effect `Replay` for both roles
- catalog load of 2 tables for 2 roles, then compiled queries for the clerk customer rows and the peek order rows
- direct `SELECT` of hidden columns and `SELECT *` returned `42501`
- source reader `INSERT` returned `42501` before and after, and the source audit has no row for the reader, the runtime login, the product login, or `oo_pipeline_runtime`
- `pg_has_role(oo_pipeline_runtime, oo_target_owner, member)` is false
- publications stayed 0

`live/outcome.json` sets `parent_acceptance` and `release` to false.

## What the grant exposes

Table `SELECT` on `_oo.physical_mappings` and the `SECURITY DEFINER` lookup are not row-scoped. After a successful call the login can read every mapping row in that database, including rows outside the policy set. Those rows carry source, digest, generation, logical names, and physical `pt_` / `pc_` names. `information_schema.column_privileges` lists that table `SELECT` once per column of `_oo.physical_mappings`. The live test recorded zero column privileges for the login on any other table. This is not authenticated tenancy and not scoped database isolation. The contract is `contracts/gateway/mapping-reader-access-v1.md`.

## Scope

Owned paths:

- `crates/oo-gateway/src/mapping_reader_access.rs`
- `crates/oo-gateway/src/lib.rs` adds `pub mod mapping_reader_access;`
- `crates/oo-gateway/tests/mapping_reader_access_pg_live.rs`
- `contracts/gateway/mapping-reader-access-v1.md`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a240/SP06/mapping-reader-access/**`

`policy.rs`, migrations, root manifests, the lock, and other binaries were not edited. `a207` remains the connector manifest and root lock writer. `cargo update` was not invoked. Registry triples were unchanged. Smoke lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

## Builders

Pinned images, both already local and `linux/arm64`:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

The builder was 6 GiB, 1 CPU, `--cap-drop ALL`, no privileged mode, no host network, no docker socket, `HOME=/tmp`, private cargo and target volumes. PostgreSQL was 1 GiB, 1 CPU, no host port, SAN `DNS:oo-a240-pg`. Rustc inside both containers was `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `RUSTUP_TOOLCHAIN=1.94.0` overrode `rust-toolchain.toml`.

Recipe origin is `origin/recipes.txt`. Unchanged blobs: `tools/sha256.rs` `b69a2af13099ce4baf2d04e79c5692d9988ab67d`, `tools/select-debs.rs` `565787c25c25bf451343cf4bc0431619c0611899`, `pg/pg-wrap.sh` `e12a368394c724f8f9d3436ad5f4873e139baa66`. `extract-libclang.sh` differs from a200 blob `1c23dbd6ab58cb0f42d218075fd55c0a8c4fe5ea` only by the private `a240-*` clang paths. a240 blob `20bc9ae212cc80915ae2130fc3dfac6849ec56a3`.

## Commands and exits

Smoke, before any overlay, archive of `df0bce2e235babc0944b68350921c13038741ab9`:

- `linux/smoke/docker-run.exit` 0, `smoke.exit` 0, `OOMKilled=false`, `Privileged=false`, `Memory=6442450944`, `NanoCpus=1000000000`
- `cargo test -p oo-gateway --lib policy::tests` produced `test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out`
- `overlay-absent.txt` is `overlay_absent`
- capacity at launch: `own=0 global=0 host_cargo=0`

Final chain, overlay of the four owned paths only:

- `linux/chain/docker-run.exit` 0, `chain.exit` 0, same memory, CPU, cap-drop, and privilege readings, container `38418d7c7a380df743e0ab46c3bc68ee8b6127dd7bafd88a131b9e6006998b16`
- unit tests: `test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 21 filtered out`
- live test: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.02s`
- `cargo clippy -p oo-gateway --lib --test mapping_reader_access_pg_live -- -D warnings` finished in 1m 25s, phase exit 0
- `secret-scan-hits.txt` is `clean`
- `cargo-update-invoked.txt` is `no`

Live step exits: `import-pg-not-ready.exit` 1, `init-pg.exit` 0, `import-pg.exit` 0.

Failed attempts are preserved under `linux/chain/attempt-1`, `attempt-2`, and `attempt-3`. Their exits are 101. Findings records the cause of each.

## Cleanup

`cleanup/CLEANUP.txt` records removal of `oo-a240-pg`, volumes `oo-a240-eef831aa-cargo`, `oo-a240-eef831aa-target`, and `oo-a240-eef831aa-pgdata`, network `oo-a240-eef831aa-net`, and `/tmp/oo-a240-eef831aa-dba4-4af2-a78b-541bc9e0a240`. Smoke and chain containers were already absent. Images were not pruned. Other owners were not touched.
