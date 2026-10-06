# a252 review of the provision-mapping-reader command

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a252`. Fixed commit `9df074813572569950d85bbe951069b4db83facc`. Parent `8bd672c559e4b1a8de7dc478f4d50327203f8d80`.

Verdict: **APPROVE_LIMITED**.

On a fresh private TLS PostgreSQL 17.10 cluster, public `init-pg` and `import-pg` installed two tables. `install_column_acl_set` installed `oo_r_clerk` and `oo_r_peek` and left mapping privileges absent. `load_policy_catalog` as `oo_a249_login` returned SQLSTATE 42501. The compiled `provision-mapping-reader` command then wrote an `installed` receipt for `oo_r_clerk`, a `replay` receipt for that role, and a `replay` receipt for `oo_r_peek`. Catalog loads for both roles and the compiled clerk and peek queries returned the seeded rows. Stale generation 3, unknown login `oo_a249_missing`, and a login that held only `USAGE` were refused with `policy stale`, `policy login`, and `policy privilege`. The partial `USAGE` was not revoked. Shadow column privileges stayed 0. `PUBLIC` stayed without the mapping privileges. The login was not a member of `oo_target_owner`. Source reader `INSERT` stayed 42501.

This review does not accept M1-D14F, the parent goal, or a release. No product, library, manifest, or lock file was edited. There is no `AGENTS.md` at the repository root.

## Same-session check

The a249 directory `review/` is the author's own session. Its proof reused PostgreSQL container `99aa39aac9fe45c5f33e75c57be25a0c03ee0057c324b977277a63df3e7e3ca7` and the a249 cargo volume. That run is not this review. This session used a new archive, a new cargo volume, a new target volume, and PostgreSQL container `ef828f3d83697168943b975cb16ccad302abb906f904cfb99755c26d69d6cc62`.

## Owned product at the fixed commit

| Path | Git blob | SHA-256 |
|---|---|---|
| `crates/oo-gateway/src/bin/provision-mapping-reader.rs` | `ad3c4b8a2f57614d2a490ca68431cbac135d19b0` | `7ba7b3320e3a63a61937ed4ecbb00984427cfdeb19ea14f372f2f1104162bed8` |
| `crates/oo-gateway/tests/provision_mapping_reader_cli.rs` | `2093d4b734d2ef87908a9cda1fce89408b5af470` | `9336b7cd38f3fcec33bea4a85b7140458a4a205c0bab5ee913c23e6adc808898` |
| `contracts/gateway/provision-mapping-reader-cli-v1.md` | `5a39870305e5e18595b178a7fd2ae24ee200d483` | `d0391a12777fd33d1e51853fa3e89825e451939c1adac95586403f9869186d0e` |

`linux/smoke/archive-blobs.txt` matches those git blobs. `lib.rs` is `d7a6fe8a8688277a917e08ad2f8a63cce91e9ba1`. `mapping_reader_access.rs`, `policy.rs`, both Cargo manifests, `init-pg.rs`, and `oo-pipeline.rs` matched the same commit. `Cargo.lock` content hash stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Lock blob `2990f901ea98a7b465f462cce8742bc5c51c13aa`. `linux/proof/phases/source.exit` is 0. `linux/proof/lock-registry-diff.txt` is `unchanged`. `cargo update` was not invoked.

The commit adds only those three paths. The archive was a partial tree of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `contracts`: listed 968, extracted 968. `rust-toolchain.toml` was moved aside on the private copy. Its git object stayed `c9901407d5fdc851007a5fd87852ec1efa418e52`. `RUSTUP_TOOLCHAIN=1.94.0`.

## Recipes

Origins and the a252 deltas are in `origin/recipes.txt`. Launch shape follows a204 blob `3737c1a8321814ff3b1070ddf8b89d6864c63f6f`, a200 blob `20b391304358e474c189203eeb525b46fc2ea194`, a199 blob `7a706f544e6d44dfd46f0c7a1095f72ba582afdf`, and the a246 review that already used them. `tools/sha256.rs` blob `b69a2af13099ce4baf2d04e79c5692d9988ab67d`, `tools/select-debs.rs` blob `565787c25c25bf451343cf4bc0431619c0611899`, and `pg/pg-wrap.sh` blob `e12a368394c724f8f9d3436ad5f4873e139baa66` are byte-identical. No product overlay was applied. `linux/smoke/overlay-absent.txt` and `linux/proof/overlay-absent.txt` are `overlay_absent`.

## Linux smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, arm64, `--pull never`. Container `oo-a252-smoke` id `706134e1d91f991dbe9eeb6d576da89a3c3fb3fb9540030871db9e6ec530d719`. Memory 6442450944, NanoCpus 1000000000, CapDrop ALL, privileged false, network `oo-a252-eef831aa-net`. The launch script removed the container. `docker-run.exit` 0. `smoke.exit` 0. `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

`OO_A249_LIVE` was unset. `cargo test --locked --offline -p oo-gateway --test provision_mapping_reader_cli` wrote `test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s`. The live test returned before a connection. No `live/oracle.txt` was written. Capacity before launch: `host_cargo=0 rust_builders=0 global=0 cap=10`.

## Private PostgreSQL

Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, arm64. Container `oo-a252-pg` id `ef828f3d83697168943b975cb16ccad302abb906f904cfb99755c26d69d6cc62`. `up.exit` 0. Privileged false. Host port bindings `{}`. Memory 1073741824. NanoCpus 1000000000. `autovacuum=off`. The frozen test connects to `oo-a249-pg`, so this container carries that alias only on network `oo-a252-eef831aa-net`. SAN `DNS:oo-a249-pg`. The builder mount listed `ca.crt` and six password files. It did not list `server.key` or `ca.key`.

## Live proof

Container `oo-a252-proof` id `1c507107d9b65aea0fae4ec879e4c772effdb9ad48240368f544202d75fde595`. Same Rust limits. `OO_A249_LIVE=1` because that is the frozen test's gate. `docker-run.exit` 0. `proof.exit` 0. Capacity before this launch: `host_cargo=0 rust_builders=1 global=1 cap=10`. The other builder was `oo-a251-preoverlay`. It was not stopped or resized.

`cargo test --locked --offline -p oo-gateway --test provision_mapping_reader_cli -- --test-threads=1 --nocapture` wrote `test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.18s`.

`cargo clippy --locked --offline -p oo-gateway --bin provision-mapping-reader --test provision_mapping_reader_cli -- -D warnings` finished in 1m 26s with no warning. `clippy.exit` 0. Clippy was downloaded for toolchain 1.94.0 in this container. `cargo update` was not invoked.

`live/oracle.txt` records `server` as `ssl=true` and `server_version=17.10 (Debian 17.10-1.pgdg12+1)`, then `init=bootstrapped`, `import_state=awaiting_publish`, `import_tables=2`, `mapping_generation=7`, `column_acl=installed`, `mapping_after_column_acl=absent`, `catalog_before=42501`, `stale=policy stale`, `unknown_login=policy login`, `partial_privilege=usage_only_refused`, `provision=installed`, `mapping_exposure=database_wide_catalog_listing`, `replay=replay`, `peek_replay=replay`, `catalog_tables=2`, `catalog_roles=2`, `compiled_query=clerk_customer_peek_order`, `public=false`, `owner_membership=false`, `source_reader_insert=42501`.

The installed receipt is `{"protocol":"oo.gateway.provision-mapping-reader-cli/v1","effect":"installed","login":"oo_a249_login","role":"oo_r_clerk"}` with no trailing newline. Replay receipts use `effect` `replay` for `oo_r_clerk` and `oo_r_peek`. Refusals are `provision-mapping-reader: policy stale`, `policy login`, and `policy privilege`, each with empty stdout and exit 7. `init-pg` exit 0, `install=bootstrapped`, `full_import=installed`, `historical_v2=false`. `import-pg` exit 0, `state=awaiting_publish`, `table_count=2`, `source_generation=7`. Server text is `ssl=true` and `server_version=17.10 (Debian 17.10-1.pgdg12+1)`.

`live/outcome.json` sets `parent_acceptance` and `release` to false. `secret-scan-hits.txt` and `pem-scan-hits.txt` are `clean`.

## Limits of this approval

The grant is database-wide mapping metadata exposure. `SELECT` on `_oo.physical_mappings` and `EXECUTE` on the `SECURITY DEFINER` lookup let the login read every mapping row in the database, including rows outside the policy set. The import stored `private_note` and `ratio` on `shop.customer`, and those columns are outside the clerk allowlist. `information_schema.column_privileges` lists the mapping table once per mapping column. This CLI proof records that listing. It does not itself `SELECT` the mapping rows. That is metadata exposure. It is not tenant isolation.

The command does not authenticate the caller, the role, or the login. It does not decide tenancy. It does not grant a business column, `PUBLIC`, a source object, or membership in `oo_target_owner`. The tested grantor is fixture superuser `oo_fixture_admin`. A non-superuser grantor is not proven here. The refused partial `USAGE` on `oo_a249_extra` was left in place.

The live path is the essential set: two roles, catalog 42501 before the command, installed, replay, partial privilege, stale generation, and an unknown login. The offline path covers usage, scan, budget, request, tls, filter, empty, producer DTO, and login binding. The wider library matrix from a240 was not repeated. `query-pg` was not invoked. The catalog load used `UnknownRows::Zero`. Audit WAL, row-level security, and publication stay open.

M1-D14F stays open. `parent_acceptance` and `release` in the outcome JSON are false.

## Cleanup

`cleanup/run.sh` removes `oo-a252-smoke`, `oo-a252-proof`, `oo-a252-pg`, `oo-a252-hash`, the a252 cargo, target, and pgdata volumes, network `oo-a252-eef831aa-net`, and `/tmp/oo-a252-eef831aa-dba4-4af2-a78b-541bc9e0a252`. Label checks use `index .Config.Labels "oo.owner"`. It does not prune and does not touch another owner. The transcript is `cleanup/CLEANUP.txt`. The inventory container writes `SHA256SUMS` and is removed after that write. The inventory has 167 files. `SHA256SUMS.sha256` is the hash of that manifest.
