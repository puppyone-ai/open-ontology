# a246 review of mapping reader access

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a246`. Fixed commit `1f1028f6161783d6e3bf1e9beb3a23a1161deca0`. Parent `a21afcdd230a9769aee4b05e68c8c7488d6eb05f`.

Verdict: **APPROVE_LIMITED**.

On a fresh private TLS PostgreSQL 17.10 cluster, public `init-pg` and `import-pg` installed two tables. `install_column_acl_set` installed `oo_r_clerk` and left mapping privileges absent. `load_policy_catalog` as the login returned SQLSTATE 42501. `provision_mapping_reader` then returned `Installed`, a second call returned `Replay`, the catalog load and one compiled customer query returned the three seeded rows, and a direct read of `private_note` and `SELECT *` returned 42501. The login could read all 14 mapping rows, including `private_note` and `ratio`, which are outside the clerk policy. Source inserts stayed 42501 and `oo_pipeline_runtime` was not a member of `oo_target_owner`.

This review does not accept M1-D14E, the parent goal, or a release. No product, library, manifest, or lock file was edited. There is no `AGENTS.md` at the repository root.

## Owned product at the fixed commit

| Path | Git blob | SHA-256 |
|---|---|---|
| `crates/oo-gateway/src/mapping_reader_access.rs` | `77be2fda49c29a6643e1f3ffe7c03a24130b9fb0` | `5d03f40d4f35cf75b111b22b716dfdacb6748758c967281c0cb16be1a2c9176e` |
| `crates/oo-gateway/src/lib.rs` | `d7a6fe8a8688277a917e08ad2f8a63cce91e9ba1` | `9a3b922ad440411489407d06496b1977ed88033333e90ad56928729b11d0f6ea` |
| `crates/oo-gateway/tests/mapping_reader_access_pg_live.rs` | `de6334a44124f5804e37542360202a9ed42322c4` | `7d2fa0bb50d55628f7af05e484a2a96dc2603480e4d8d76c0d7c21d29fa8fde1` |
| `contracts/gateway/mapping-reader-access-v1.md` | `2a0d987a73f41f240a1755226f4b3b7ed1df91e8` | `20a69568214f23926761dfabfb87c015774181952f002450b177668a91378e02` |

`linux/smoke/archive-blobs.txt` matches those git blobs, plus unchanged `policy.rs` `84a86e85ef82bc2a459a5e2faef5827d077ca6cc`, `Cargo.lock` `2990f901ea98a7b465f462cce8742bc5c51c13aa`, `init-pg.rs` `645833505a0e92525393b13d262ca0213c492e52`, and `oo-pipeline.rs` `cf8416936aec7f964833196a71bb80ebba75be9d`. SHA-256 values are `linux/smoke/owned-sha256.txt`. `linux/overlay/phases/frozen-match.exit` is 0. `Cargo.lock` content hash stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

The review test is not in the commit. Its git blob is `cb2b438218da6f892c8cfffe4e8459fd90073142`. Its proof SHA-256 is `b64120fe30baa1a4b9833772c39ccf288f5600962db900c78f2e205f63fa1ac4`.

## Archive and recipes

Partial archive of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `contracts` from `1f1028f6161783d6e3bf1e9beb3a23a1161deca0`. Listed files 965, extracted files 965. `rust-toolchain.toml` was moved aside on the private copy. The git object stayed `c9901407d5fdc851007a5fd87852ec1efa418e52`. `RUSTUP_TOOLCHAIN=1.94.0`.

Recipe origins and the a246 deltas are in `origin/recipes.txt`. Launch shape follows a204, a200, a199, and the a241 fixed review that already used those recipes. The smoke tree does not contain `mapping_reader_access_a246_review.rs`. The overlay copies only that file.

## Linux smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, arm64, `--pull never`. Container `oo-a246-smoke` id `c49ca058da9e11f0f9c7ef319e4eb96119f24ba8a1dbb567e9613860209786e0`. Memory 6442450944, NanoCpus 1000000000, CapDrop ALL, privileged false, port bindings `{}`, network `oo-a246-eef831aa-net`. The launch script removed the container. `docker-run.exit` 0. `smoke.exit` 0. `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

`cargo test --locked --offline -p oo-gateway --lib mapping_reader_access` wrote `test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 21 filtered out`. `OO_A240_LIVE` and `OO_A246_LIVE` were unset. The committed live test returned before opening a connection: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`. No `live/oracle.txt` was written. Capacity before launch: `host_cargo=0 rust_builders=1 global=1 cap=10`.

## Private PostgreSQL

Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, arm64. Container `oo-a246-pg` id `13681fae9e40fa255e2875fdd159536150addfe0b46fadcf0a027f35860dc194`, alias `oo-a246-pg`. `up.exit` 0. Privileged false. Host port bindings `{}`. `autovacuum=off`. SAN `DNS:oo-a246-pg`. The builder mount listed `ca.crt` and five password files. It did not list `server.key` or `ca.key`.

## Overlay proof

Container `oo-a246-proof` id `bf89f04556613d9c0490a02f905cb2c8a0278b5fac1d6ce650feb80c7ae82263`. Same Rust limits. `OO_A246_LIVE=1`. `docker-run.exit` 0. `overlay.exit` 0. Capacity before this launch: `host_cargo=0 rust_builders=1 global=1 cap=10`.

`cargo test --locked --offline -p oo-gateway --test mapping_reader_access_a246_review -- --test-threads=1 --nocapture` wrote `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.84s`.

`cargo clippy --locked --offline -p oo-gateway --lib --test mapping_reader_access_a246_review --test mapping_reader_access_pg_live -- -D warnings` finished in 1m 26s with no warning. `clippy.exit` 0.

Live oracle `live/oracle.txt`:

```text
server=tls_17.10
source_reader_insert_before=42501
import_before_init=target schema is not ready
init=bootstrapped
import_state=awaiting_publish
import_tables=2
mapping_generation=7
publication=0
runtime_owner=false
column_acl=installed
mapping_after_column_acl=absent
catalog_before=42501
mapping_select_before=42501
login_mismatch=LoginMismatch
stale=StaleGeneration
mapping_absent=MappingAbsent
login_absent=LoginAbsent
partial_privilege=usage_only_refused
provision=Installed
business_grants=unchanged
shadow_columns=0
public=absent
replay=Replay
catalog_tables=2
typed_query=clerk_customer_3
hidden=42501
mapping_rows=all
mapping_row_count=14
outside_policy=private_note,ratio
tenant_isolation=not_provided
source_writes=reader_runtime_login_zero
runtime_owner_after=false
```

`init-pg` exit 0, `install=bootstrapped`, `full_import=installed`, `historical_v2=false`. Import before init exit 1, stderr `oo-pipeline: target schema is not ready`. `import-pg` exit 0, `state=awaiting_publish`, `table_count=2`, `source_generation=7`. Server text is `ssl=true` and `server_version=17.10 (Debian 17.10-1.pgdg12+1)`. The compiled customer query returned rows 41 `丙号` true, 42 null false, and 43 `丁铺` true.

The 14 mapping rows are the namespace row, two table rows, and 11 column rows. The clerk policy names 9 of those columns. `private_note` and `ratio` are stored mapping rows and were visible to the login. That is metadata exposure of the whole mapping table. It is not tenant isolation.

`live/outcome.json` sets `parent_acceptance` and `release` to false. `secret-scan-hits.txt` is `clean`.

## Failed attempt

Attempt 1 (`attempt-1/`) compiled `init-pg` and `oo-pipeline` (`bins.exit` 0) and stopped in the review harness. `clerk_columns` returns `Vec<&str>`, and `iter()` yields `&&str`, which does not compare with `&str`. Exit 101. No oracle. The product blobs were unchanged. The corrected comparison is the file that the green run hashed.

## Cleanup

`cleanup/run.sh` removes `oo-a246-smoke`, `oo-a246-proof`, `oo-a246-pg`, `oo-a246-hash`, the a246 cargo, target, and pgdata volumes, network `oo-a246-eef831aa-net`, and `/tmp/oo-a246-eef831aa-dba4-4af2-a78b-541bc9e0a246`. Label checks use `index .Config.Labels "oo.owner"`. It does not prune and does not touch another owner. The transcript is `CLEANUP.txt`. The inventory container writes `SHA256SUMS` and is removed after that write.
