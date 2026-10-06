# Provision mapping reader command

Development record for UUID `eef831aa-dba4-4af2-a78b-541bc9e0a249`. Fixed archive `e0704257a1509b968a80a9ccd2f200319d5483ad`. Product commit `9df074813572569950d85bbe951069b4db83facc`. This run adds the operator command that calls `provision_mapping_reader`. It does not accept a parent plan item, does not close M1-D14F, and does not release.

There is no `AGENTS.md` at the repository root.

## Result

`provision-mapping-reader` reads one stdin document: the reviewed-policy policy objects, an explicit login, and TLS password and CA file paths. It compiles with `compile_column_acl_set` before opening those files or connecting. It then calls public `provision_mapping_reader` and writes one compact receipt.

The development chain recorded:

- `init-pg` exit 0, `install=bootstrapped`, `full_import=installed`, `historical_v2=false`
- `import-pg` exit 0, `state=awaiting_publish`, `table_count=2`, `source_generation=7`
- column ACL for `oo_r_clerk` and `oo_r_peek` installed, mapping privileges still absent
- `load_policy_catalog` as `oo_a249_login` returned SQLSTATE `42501` before the command
- stale generation 3 exited 7, `provision-mapping-reader: policy stale`, mapping still absent
- unknown login `oo_a249_missing` exited 7, `provision-mapping-reader: policy login`
- `oo_a249_extra` with only `USAGE` on `_oo` exited 7, `provision-mapping-reader: policy privilege`; `SELECT` and lookup `EXECUTE` stayed false
- installed receipt `{"protocol":"oo.gateway.provision-mapping-reader-cli/v1","effect":"installed","login":"oo_a249_login","role":"oo_r_clerk"}`
- replay receipts for `oo_r_clerk` and `oo_r_peek` on the same login
- role column-grant text unchanged; login shadow column privileges 0; mapping-column `SELECT` count equal to the mapping column count
- catalog load of 2 tables for 2 roles, then compiled clerk customer rows and peek order rows
- source reader `INSERT` returned `42501` and the customer count stayed 3
- `PUBLIC` mapping privileges stayed false, and the login was not a member of `oo_target_owner`

`dev-live/outcome.json` sets `parent_acceptance` and `release` to false. The same oracle was produced again by the review container and is kept at `review/live-record/`.

## What the grant exposes

`SELECT` on `_oo.physical_mappings` is table-level, and the lookup function is `SECURITY DEFINER`. After a successful call the login can read every mapping row in that database. `information_schema.column_privileges` lists that table `SELECT` once per mapping column. The live oracle names this `database_wide_catalog_listing`. It is administrative metadata exposure. The contract is `contracts/gateway/provision-mapping-reader-cli-v1.md`.

## Scope

Owned product paths, committed in `9df074813572569950d85bbe951069b4db83facc`:

| Path | Git blob | SHA-256 |
|---|---|---|
| `crates/oo-gateway/src/bin/provision-mapping-reader.rs` | `ad3c4b8a2f57614d2a490ca68431cbac135d19b0` | `7ba7b3320e3a63a61937ed4ecbb00984427cfdeb19ea14f372f2f1104162bed8` |
| `crates/oo-gateway/tests/provision_mapping_reader_cli.rs` | `2093d4b734d2ef87908a9cda1fce89408b5af470` | `9336b7cd38f3fcec33bea4a85b7140458a4a205c0bab5ee913c23e6adc808898` |
| `contracts/gateway/provision-mapping-reader-cli-v1.md` | `5a39870305e5e18595b178a7fd2ae24ee200d483` | `d0391a12777fd33d1e51853fa3e89825e451939c1adac95586403f9869186d0e` |

Evidence is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a249/SP06/mapping-reader-cli/**`.

`lib.rs`, `mapping_reader_access.rs`, `policy.rs`, other binaries, manifests, and `Cargo.lock` were not edited. The fixed archive blob of `Cargo.lock` is `2990f901ea98a7b465f462cce8742bc5c51c13aa`. Its content hash stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. `a207` remains the connector manifest and root lock writer. `cargo update` was not invoked.

## Builders

Pinned images, both already local and `linux/arm64`, `--pull never`:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

Each Rust builder was 6 GiB, 1 CPU, `--cap-drop ALL`, no privileged mode, no host network, no docker socket, `HOME=/tmp`. PostgreSQL was started at 1 GiB and 1 CPU, no host port, SAN `DNS:oo-a249-pg`. Rustc in the containers was `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `RUSTUP_TOOLCHAIN=1.94.0` overrode `rust-toolchain.toml` on the overlay copy.

Recipe origin is `origin/recipes.txt`. Unchanged blobs: `tools/sha256.rs` `b69a2af13099ce4baf2d04e79c5692d9988ab67d`, `tools/select-debs.rs` `565787c25c25bf451343cf4bc0431619c0611899`, `pg/pg-wrap.sh` `e12a368394c724f8f9d3436ad5f4873e139baa66`. `extract-libclang.sh` differs from a240 blob `20bc9ae212cc80915ae2130fc3dfac6849ec56a3` only by the private `a249-*` clang paths.

## Commands and exits

Smoke, before any overlay, archive of `e0704257a1509b968a80a9ccd2f200319d5483ad`:

- `linux/smoke/docker-run.exit` 0, `smoke.exit` 0, `OOMKilled=false`, `Privileged=false`, `Memory=6442450944`, `NanoCpus=1000000000`
- container `2f2fd0567909fd70556d2aaea0fadeab71551c917925f958020b1a9f98a9c4c1`
- `cargo test -p oo-gateway --lib mapping_reader_access` produced `test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 21 filtered out`
- `overlay-absent.txt` is `overlay_absent`
- capacity at launch: `own=0 global=3 host_cargo=0`
- `cargo-update-invoked.txt` is `no`

Development chain, overlay of the three owned paths only:

- `linux/chain/docker-run.exit` 0, `chain.exit` 0, same memory, CPU, cap-drop, and privilege readings
- container `d2f95b031562f3af9eb0708d32010d5b5353e04d3f8e2ea8c8271ac3fc4454ae`
- capacity at launch: `own=0 global=1 host_cargo=0`
- `cargo test -p oo-gateway --test provision_mapping_reader_cli` produced `test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.16s`
- `cargo clippy -p oo-gateway --bin provision-mapping-reader --test provision_mapping_reader_cli -- -D warnings` finished in 1m 27s, phase exit 0
- `secret-scan-hits.txt` is `clean`
- lock registry diff is `unchanged`
- `cargo-update-invoked.txt` is `no`

Live step exits in `dev-live/steps`: `init-pg` 0, `import-pg` 0, `cli-stale` 7, `cli-unknown` 7, `cli-partial` 7, `cli-installed` 0, `cli-replay` 0, `cli-peek-replay` 0.

The development chain had no failed attempt. The review container's first run is preserved under `review/attempt-1`.

## Review

A separate container re-ran the committed test. The verdict is in `review/REPORT.md`.

## Cleanup

`cleanup/CLEANUP.txt` records removal of `oo-a249-pg`, volumes `oo-a249-eef831aa-cargo`, `oo-a249-eef831aa-target`, and `oo-a249-eef831aa-pgdata`, network `oo-a249-eef831aa-net`, and `/tmp/oo-a249-eef831aa-dba4-4af2-a78b-541bc9e0a249`. Smoke, chain, and review containers were already absent. Images were not pruned. Other owners were not touched.
