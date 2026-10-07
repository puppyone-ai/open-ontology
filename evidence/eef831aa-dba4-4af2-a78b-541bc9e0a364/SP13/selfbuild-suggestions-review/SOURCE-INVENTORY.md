# Source inventory

Reviewed product: `tools/selfbuild-suggestions` at `d22187ea0fce8e420323660841f2da3b447d5de8`. Working tree matches that commit for this path. Root `Cargo.toml` and `Cargo.lock` were not edited. No `cargo update` and no `cargo generate-lockfile`.

Query-pg and the shadow migrations come from `7a6f7eac8b8041eae215c087d1a80930437871ce`, archived to a UUID-private temp directory and mounted read-only. The archive is removed in owned cleanup. Hashes below were taken from that archive and from the built binary before removal.

## Images

| Role | Reference | Observed id and platform |
| --- | --- | --- |
| Rust 1.94 builder | `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` | `smoke-before/image.txt`, `querypg/image.txt`: linux/arm64, same digest |
| PostgreSQL 17.10 | `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` | `pg/image.txt`: linux/arm64, same digest |

`--pull never` on every `docker run`.

## Recipe files and hashes

| Origin | Path | SHA-256 |
| --- | --- | --- |
| a326 smoke-2 | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/linux/smoke-2/extract-libclang.sh` | `1c2f68d652f667eee5d861ec11bb0ee5654ade6908826e3f6dcb4c7e1c1f3024` |
| a326 tools | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| a326 tools | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |
| testing recipe, read-only mount | `testing/real-db/gateway-tls/pg/pg-wrap.sh` | `238d2d9828dab0e2f8402cc935adf324890b4802aeba842679bd3473e49ebbf6` |
| flag source cited by `build/launch.sh` | `testing/real-db/gateway-tls/linux/proof/launch.sh` | `cda80b68b1c2a38ba7d9ba97e84c90251fc8a16a5c999bbac303546a476d1018` |

`querypg/origin-sha256.txt` records the three a326 hashes and the archived `query-pg.rs` hash. Those a326 hashes match the fresh `sha256sum` above.

## Archived gateway sources at 7a6f7eac

| File | SHA-256 |
| --- | --- |
| `crates/oo-gateway/src/bin/query-pg.rs` | `b6cd2f4e5a5af9d895a6c2ab9020ca6956afc4f61dd0b799a9cc297f54c13a22` |
| built `/target/debug/query-pg` | `68ddb8ecdb031cff8db5b0764f3916c0593aad43eda2158257b1c5667a365c30` |
| archived `Cargo.lock` after `cargo build --locked` | `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474` |
| `0000_bootstrap_roles.sql` | `37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208` |
| `0001_target_fence_expand.sql` | `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9` |
| `0003_target_segment_preflight_expand.sql` | `d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3` |
| `0004_physical_mapping.sql` | `844de6f645f8f1ebf29d05118ae25d4669d8dc870eed1fa7697d4b98456eaf95` |
| `0005_full_import_expand.sql` | `a9cef2013019784857353ff4fe060350d71a4e9ac4ab18551b992a7a9b52ecb6` |

`querypg/inside.sh` copies `Cargo.lock`, builds with `--offline --locked`, and exits 1 if the lock differs from the copy. `querypg/build.exit` and `querypg/docker-run.exit` are 0.

Producer lock copy `build/Cargo.lock.frozen` matches `tools/selfbuild-suggestions/Cargo.lock`, SHA-256 `f29a2691f8e214a389c7221b2040c48a5558eef6d1f10e72bca21dd217864724`. Crate pins: serde `=1.0.229` derive, serde_json `=1.0.151`, sha2 `=0.10.9`.

## Evidence written by this review

Checker and probes, safe Rust only: `probe/oracle.rs`, `probe/independent.rs`. Gold literals: `gold/`. Scripts: `smoke-before/`, `build/launch.sh`, `build/inside.sh`, `querypg/launch.sh`, `querypg/inside.sh`, `pg/up.sh`, `pg/apply.sh`, `live/launch.sh`, `live/inside.sh`, `live/wrapper.sh`. The wrapper is capture-only.

Compiler recorded in `querypg/rustc.txt`: rustc 1.94.0 (`4a4ef493e3a1488c6e321570238084b38948f6db`, 2026-03-02), host `aarch64-unknown-linux-gnu`, LLVM 21.1.8. The same version string is in `smoke-before/rustc.txt` and `build/rustc.txt`.

## Private names

Network `oo-a364-sbs-net`. Containers `oo-a364-sbs-smoke`, `oo-a364-sbs-test`, `oo-a364-sbs-querypg`, `oo-a364-sbs-live` (removed after each run), `oo-a364-pg` (removed in CLEANUP.md). Volumes `oo-a364-sbs-cargo`, `oo-a364-sbs-target`, `oo-a364-gw-target`, `oo-a364-pgdata`. Database `oo_a364`. No host port. No shared cargo cache with another UUID.
