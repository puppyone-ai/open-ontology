# a249 review of the provision-mapping-reader command

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a249`. Fixed archive `e0704257a1509b968a80a9ccd2f200319d5483ad`. Product commit `9df074813572569950d85bbe951069b4db83facc`.

Verdict: **APPROVE_LIMITED**.

The committed command was copied onto a fresh tree of the fixed archive and checked by hash before the test. On a private TLS PostgreSQL 17.10 cluster, public `init-pg` and `import-pg` installed two tables. `install_column_acl_set` installed `oo_r_clerk` and `oo_r_peek` and left mapping privileges absent. `load_policy_catalog` as `oo_a249_login` returned SQLSTATE 42501. The command then wrote an `installed` receipt for `oo_r_clerk`, a `replay` receipt for that role, and a `replay` receipt for `oo_r_peek`. Catalog loads for both roles and the compiled clerk and peek queries returned the seeded rows. Stale generation 3, unknown login `oo_a249_missing`, and a login that held only `USAGE` were refused with `policy stale`, `policy login`, and `policy privilege`. The partial `USAGE` was not revoked. Shadow column privileges stayed 0. `PUBLIC` stayed without the mapping privileges. The login was not a member of `oo_target_owner`. Source reader `INSERT` stayed 42501.

This review does not accept M1-D14F, the parent goal, or a release. No product, library, manifest, or lock file was edited during the review. There is no `AGENTS.md` at the repository root.

## Owned product at 9df07481

| Path | Git blob | SHA-256 |
|---|---|---|
| `crates/oo-gateway/src/bin/provision-mapping-reader.rs` | `ad3c4b8a2f57614d2a490ca68431cbac135d19b0` | `7ba7b3320e3a63a61937ed4ecbb00984427cfdeb19ea14f372f2f1104162bed8` |
| `crates/oo-gateway/tests/provision_mapping_reader_cli.rs` | `2093d4b734d2ef87908a9cda1fce89408b5af470` | `9336b7cd38f3fcec33bea4a85b7140458a4a205c0bab5ee913c23e6adc808898` |
| `contracts/gateway/provision-mapping-reader-cli-v1.md` | `5a39870305e5e18595b178a7fd2ae24ee200d483` | `d0391a12777fd33d1e51853fa3e89825e451939c1adac95586403f9869186d0e` |

`review/phases/product-bytes.exit` is 0. `review/phases/overlay-unchanged.exit` is 0. `lib.rs`, `mapping_reader_access.rs`, `policy.rs`, `crates/oo-gateway/Cargo.toml`, and `Cargo.lock` matched the fixed archive. Lock content hash `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

## Independent raw proof

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, arm64, `--pull never`. Container `oo-a249-review` id `de93677a4a0bcfe81aa95ef723f460e40266042fca8292665a033bc86b1f0a96`. Memory 6442450944, NanoCpus 1000000000, CapDrop ALL, privileged false, network `oo-a249-eef831aa-net`. `docker-run.exit` 0. `review.exit` 0. `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

`cargo test --locked --offline -p oo-gateway --test provision_mapping_reader_cli -- --test-threads=1 --nocapture` wrote `test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.06s`.

`cargo clippy --locked --offline -p oo-gateway --bin provision-mapping-reader --test provision_mapping_reader_cli -- -D warnings` checked `oo-gateway` and finished in 0.48s with phase exit 0. The dependency graph was already built by the test compile in this same target volume. The development chain's cold clippy finished in 1m 27s with phase exit 0.

`review/secret-scan-hits.txt` is `clean`. `review/cargo-update-invoked.txt` is `no`. `review/lock-registry-diff.txt` is `unchanged`. The oracle copy is `review/live-record/oracle.txt`.

PostgreSQL stayed the a249 instance `99aa39aac9fe45c5f33e75c57be25a0c03ee0057c324b977277a63df3e7e3ca7`, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, SAN `DNS:oo-a249-pg`, no host port. The test reset that cluster and created its own databases.

## Attempt 1

`review/attempt-1` is the first review container, id `683ee2bd2e9979ad5eea29f41c00b19080cede965f6077f55fe083e23c3668d6`. The same 14 tests passed in 2.09s. Clippy then exited 1: `cargo-clippy` was not installed for toolchain `1.94.0-aarch64-unknown-linux-gnu`. The review script had skipped `rustup component add clippy`, which the development chain performs because `RUSTUP_HOME` does not survive the container. The script was corrected and the run above was repeated. No product file changed between the two runs. `review/phases/product-bytes.exit` on the successful run is 0 against commit `9df07481`.

## Limits of this approval

The grant is database-wide mapping metadata exposure. `SELECT` on `_oo.physical_mappings` and `EXECUTE` on the `SECURITY DEFINER` lookup let the login read every mapping row in the database, including rows outside the policy set. `information_schema.column_privileges` lists that table once per mapping column. Shadow column privileges for the login were 0. That is not scoped isolation.

The command does not authenticate the caller, the role, or the login. It does not decide tenancy. It does not grant a business column, `PUBLIC`, a source object, or membership in `oo_target_owner`. The tested grantor is fixture superuser `oo_fixture_admin`. A non-superuser grantor is not proven here.

The live path is the essential set: two roles, catalog 42501 before the command, installed, replay, partial privilege, stale generation, and an unknown login. The offline path covers usage, scan, budget, request, tls, filter, empty, and login binding. The wider library matrix from a240, including `float8`, a one-table grant mismatch, a superuser login, and direct `SELECT *` on a shadow table, was not repeated by this CLI test. Audit WAL, row-level security, and publication stay open.

M1-D14F stays open. `parent_acceptance` and `release` in the outcome JSON are false.
