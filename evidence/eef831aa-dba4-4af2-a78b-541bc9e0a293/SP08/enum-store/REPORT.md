# SP08-W03 enum store

## Scope and verdict

This child adds a standalone Rust workspace at `tools/enum-store`. It is one single-project Agent DB deployment of a reviewed `enum-history/v1` report. Install binds one project. The library re-executes `enum_history::compile` on the original enum YAML, the explicit civil `as_of`, the observations, and the threshold spelling, and it refuses a caller report digest that disagrees. Snapshot, notice, and published rows stay in the private schema `oo_enum_control`. The view `oo_dict.enums` exposes enum id, code, description, class, valid from, valid to, version, `as_of`, project id, and status.

`finance_certified`, `semantic_certified`, `owner_authenticated`, `metric_certified`, and `clock_used` stay false. Activation is an explicit compare-and-swap after a complete validated materialization. There is no background clock update and no new enum-class inference.

This report does not claim parent acceptance of SEM-D04, full SP08, or a release. Shared-project authorization remains a parent requirement. a278 reviewed `enum-history`. a278 did not review this store.

Fixed commit `c8588faab992af1193ff58dbc489a0b117796aa0`, tree `f63d5ce2dcfbc8de759510280aa3761acf0b3e15`. Helper tree `701743f204495101b5efd20104bccb8d104df467`. The product proof archives that commit and overlays only `tools/enum-store`. The root `Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. a207 remains the connector manifest and root lock writer. No root manifest or root lock was edited. `cargo update` was not invoked. `cargo generate-lockfile` was not invoked. The registry subset reported `registry=208 path=3 missing=0 mismatch=0`.

Contract: `tools/enum-store/CONTRACT.md`, name `enum-store-v1`.

## Recipes

Launch, TLS, lock subset, and the manifest tool follow reviewed recipes. The UUID, fixed commit, owned paths, 6 GiB limit, and 1 CPU limit are this child's values. Origins, paths, and hashes are in `recipe-origin.txt`.

`linux/tools/sha256_manifest.rs` matches the a204 blob `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. `linux/tools/lock_subset.rs` checks the three path packages `enum-store`, `enum-history`, and `oo-core`.

## Pre-edit smoke

`linux/precode` on the pinned Linux arm64 Rust 1.94 image, before the enum-store overlay. It ran the existing enum-history tests from the fixed archive. `test-count.txt` reports `passed=23 failed=0`. `smoke.exit` 0. The enum-history lock stayed `32ff28450510cd4cc375cc49ed52fe4be3a6a09af3d2a17b80c94c9ee90fa7a8`. `cargo-update-invoked.txt` is `no`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Container name `oo-a293-precode`, removed by the launch script.

`console.log` line 13 is `mv: cannot move 'rust-toolchain.toml' to 'rust-toolchain.toml.aside': Read-only file system`. The archive bind is read-only, `set -u` was on without `set -e`, and the script continued. `toolchain-aside.txt` says `moved-aside`, which does not match that line. The image compiler was already 1.94.0, and the smoke exit stayed 0. That log was not rewritten. Later inside scripts copy the archive onto the writable cargo volume before moving the toolchain file.

## Decisive verification

`linux/impl-1` fetched the child lock from a copy of the root lock and passed the PostgreSQL contract. `lock-subset.txt` reports `registry=208 path=3 missing=0 mismatch=0 names=true`. The root lock hash after the run was still `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. The child lock is `19619c1de9ad8fec8a11b62e4464d93c3789a5ecfdc519281b2f0bc45970c923` and is copied to `tools/enum-store/Cargo.lock`. The same bytes are in `linux/impl-1/enum-store-Cargo.lock` and `linux/impl-3/enum-store-Cargo.lock`.

`linux/impl-1/test.log`:

- library: 5 passed, including the handwritten historical digest
- `tests/contract.rs`: 2 passed
- `tests/live.rs`: `fresh_pg_contract` passed in 1.38s; `restart_snapshot_persists` filtered out
- doctests: 0 passed

Clippy then exited 101 on two `needless_borrow` sites in `tests/live.rs`. `smoke.exit` 101. The database left by the passing live test was kept. Container name `oo-a293-impl-1`, removed by the launch script. OOMKilled false. Memory 6442450944, NanoCpus 1000000000, CapDrop ALL.

`linux/impl-2` reused that child lock, did not fetch from the network, skipped `fresh_pg_contract`, and failed clippy on a single-argument `concat!` in `src/publish.rs` tests. Library and contract tests passed. `smoke.exit` 101. That log stays.

`linux/impl-3` is the passing clippy run on the same lock and the same database. It skipped `fresh_pg_contract` and `restart_snapshot_persists`. Library 5 passed, contract 2 passed, live tests filtered 2, doctests 0. `cargo clippy --locked --offline --all-targets -- -D warnings` exited 0. `smoke.exit` 0, `docker-run.exit` 0, OOMKilled false, privileged false. `cargo-update-invoked.txt` is `no`. The root lock stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Container name `oo-a293-impl-3`, removed by the launch script. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, cargo `1.94.0 (85eff7c80 2026-01-15)`.

`fresh_pg_contract` in `linux/impl-1/test.log` is the live test against private PostgreSQL 17.10. Its pass covers revoked `CREATE` returning SQLSTATE `42501` with no schema left behind, a foreign `oo_dict.enums` collision that is not dropped by the adapter, install and install replay, a second project, an allowlist change, SQL-hash drift, an extra snapshot column, a wrong report digest, `invalid_as_of`, `inferred_unapproved`, a wrong CA, runtime `42501` before activate, historical insert and exact replay, `Some("0.95")` snapshot drift with `threshold_defaulted` still true, a tampered published status that stays `approved` on the drift snapshot while the historical row stays `deprecated`, a stale compare-and-swap, a wrong config digest, activation `42501` after revoking view-owner membership with the served table still empty, six snapshots, two competing activations with one winner and one `CasConflict`, a coverage view row for unknown code `9` with status `unexplained` and a null class, the final historical view at `as_of` `2020-01-01`, runtime `UPDATE`/`DELETE`/`INSERT` `42501`, runtime and view-owner refusal to read source and `biz.receipts`, a non-allowlisted reader, and admin `read_view` refused as `LoginRole`.

The historical report digest pinned in source and matched by that run is `63fb59e7431cd131c512c3bfa1c61987c717d13bfe902552925d2b782649d9f6`. Coverage `95/100` compared `equal` with `threshold_met` true and `review_needed` true. Coverage `94/100` compared `below` with `threshold_met` false. Retroactive published statuses stayed `superseded` then `approved`, and the 2024-01-01 classes stayed `open` then `closed`.

`linux/restart` restarted `oo-a293-pg` on the host. `pg_postmaster_start_time` changed from `2026-10-06 11:05:37.544198+00` to `2026-10-06 11:13:18.017698+00`. `restart_snapshot_persists` passed in 0.23s on the impl-3 work tree with `--locked --offline`. `smoke.exit` 0. The same 6 GiB and 1 CPU limits. Container name `oo-a293-restart`, removed by the launch script. The restarted read still had one historical view row, six snapshots, the damaged drift status `approved`, the coverage notice `unknown_code`, ratio `95/100`, the pinned historical digest, snapshot replay, and install replay.

PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. No image was pulled. The database ACL was `{oo_fixture_admin=CTc/oo_fixture_admin,oo_enum_admin=Cc/oo_fixture_admin,oo_r_enum=c/oo_fixture_admin,oo_r_other=c/oo_fixture_admin}`. No host port was published.

Repo HEAD at the impl-1 launch was `795b206729c212b319dfbaae85b24f740f64be39`. Repo HEAD at the impl-3 launch was `904e9e45420dede8acfcd9434ebaabeb5a9ae594`. Both launches archived the same fixed commit. The overlay was the working tree's `tools/enum-store`.

## What the store does

Install is one transaction. The SQL file checksum and the catalog shape must match on replay. A foreign `oo_dict` schema or `oo_dict.enums` is a collision and is left in place. Snapshot identity is the project, enum version, config digest, `as_of`, observations digest, and threshold spelling. An equal identity and equal payload replays. Any stored difference is drift and does not overwrite. The report digest is checked before connect and is not part of the identity.

Unknown observation codes are published as status `unexplained` with a null class, description, and dates, and they are also stored as notices. `inferred_unapproved` inserts nothing. Superseded and deprecated statuses are copied from the compiler. `historical_business_class` does not rewrite them to `approved`.

`oo_dict.enums` is a simple select of the served projection, owned by `oo_enum_view` (NOLOGIN). The live `UPDATE` returned SQLSTATE `42501`. Allowlisted runtime roles receive `SELECT` on the view only inside the activation transaction. The view has no project argument. The view definition contains no clock call and does not project YAML, report JSON, or `row_kind`.

## Product hashes

| File | SHA-256 |
|---|---|
| `CONTRACT.md` | `857f8d8769744d4eb9d22c1d297990ef30eee5f0567f4166b7d9e0a9a15e1fa8` |
| `Cargo.lock` | `19619c1de9ad8fec8a11b62e4464d93c3789a5ecfdc519281b2f0bc45970c923` |
| `Cargo.toml` | `f401c5a64ee011c90c4d84658e0b5660fc2713fb8864878c6a804c581157ec95` |
| `migrations/oo_enum_control_v1.sql` | `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd` |
| `src/error.rs` | `881f283bea36033834413ddb13682d12e4f26f14b1574a426a2375ea875d868a` |
| `src/lib.rs` | `2c65c7379082c105b985764e7f09f34034d24ef303328fdc1437b1339736709f` |
| `src/publish.rs` | `3e878ca6a6c4c5e41b4406e4aa4f0bab3d350238db04af64df473d09cc6029db` |
| `src/role.rs` | `f89272fc39efeafd4358ddd9c1a5d8e8f684faf239213b7a281d00f1d4efc545` |
| `src/sha256.rs` | `6a23b9a173d79490fd74daeff81608fc3b7b4d03c3851a5b41e70b055d525c4d` |
| `src/sql.rs` | `b2e802febd213310665646da1c21965f6d0f4c089fbb569aabd59816e19040a2` |
| `src/store.rs` | `bf5bdf8ce3589a08d1182eee0e7d02297b3abaa54d08c2bcf06e9fd56a2e1b6d` |
| `src/tls.rs` | `ff542f33471cbcac15331193de8038482c5e978504facf872201b684f63aa7a5` |
| `tests/contract.rs` | `e37aa13e4cd2ddd0c7897a31a938205875fa81baec7438fe501002910988d30d` |
| `tests/live.rs` | `5c103c0ee5fadf87ce80a35ccb625f8d143b2206c837655fe72cceafea710281` |

Copies of those bytes are under `source/blobs/tools/enum-store/`. The evidence inventory is `MANIFEST.sha256`, written after this report and `CLEANUP.txt`. It does not include itself. The manifest runner writes `linux/manifest/console.log`, `hostconfig.txt`, `docker-run.exit`, and `container-removed.txt` after the inventory.
