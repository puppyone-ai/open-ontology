# SP08-W01C calendar store

## Scope and verdict

This child adds a standalone Rust workspace at `tools/calendar-store`. It is the first single-project Agent DB deployment for the reviewed business calendar. One immutable project binding is stored at install. The library re-executes `business_calendar::resolve` on the original calendar YAML, the explicit `as_of`, and the declared closure, and it refuses a caller hash that disagrees. Normalized snapshots and closure provenance stay in the private schema `oo_calendar_control`. The readonly view `oo_dict.calendar` exposes typed anchor dates, `business_tz`, the stored `as_of`, the calendar version, the project binding, and blocked midnight status.

`finance_approval`, `authenticated`, ontology publication, and `clock_used` stay false. Activation is an explicit compare-and-swap of the active pointer. There is no silent financial close.

This report does not claim parent acceptance, a public gateway, authentication, a calendar owner UI, a metric certificate, full SP08, or a release. Multi-project shared-database authorization and a production release remain parent requirements.

Fixed commit `216e6df82a61d2bc22548ce1a5075d5f4a045648`, tree `b7d5faef17d3e8f0170047cc7f82a996f46538ff`. Helper tree `ea5778dfb87a7cfdb04af2f236db78fe97f7bc4b`. The product proof archives that commit and overlays only `tools/calendar-store`. The root `Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. a207 remains the connector manifest and root lock writer. No root manifest or root lock was edited. `cargo update` was not invoked. The registry subset reported `registry=208 path=3 missing=0 mismatch=0`. There was no supply conflict.

Contract: `tools/calendar-store/CONTRACT.md`, name `calendar-store-v1`, written before the Rust sources.

## Recipes

Launch, TLS, lock subset, and the manifest tool follow reviewed recipes. The UUID, fixed commit, owned paths, 6 GiB limit, and 1 CPU limit are this child's values. Origins, paths, and hashes are in `recipe-origin.txt`.

`linux/tools/sha256_manifest.rs` matches the a204 blob `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. `linux/tools/lock_subset.rs` checks the three path packages `calendar-store`, `business-calendar`, and `oo-core`.

## Pre-edit smoke

`linux/precode` on the pinned Linux arm64 Rust 1.94 image, before the calendar-store overlay. It ran the existing business-calendar tests from the fixed archive. `test.log` reports 8 library, 0 binary, 5 CLI, 8 hand-computed, and 0 doctests: 21 passed, 0 failed. `smoke.exit` 0. The business-calendar lock stayed `7a9470817fc50ce83b44faacd4cd6330b3ee6d76d22a0e0bf9cbb75532f83cf5`. `cargo-update-invoked.txt` is `no`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Container name `oo-a289-precode`, removed by the launch script.

## Decisive verification

`linux/impl-2` is the passing product run. It copied the fixed archive, moved `rust-toolchain.toml` aside, overlaid the tool, fetched from a copy of the root lock, and checked the subset. `lock-subset.txt` reports `registry=208 path=3 missing=0 mismatch=0 names=true`. A later fetch used `--locked --offline`. The root lock hash after the run was still `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. The generated child lock is `a3c203e65af360f75cac875143a669e9ae91a73a02fa65d656db4d24db1cbe44`, identical to the impl-1 lock, and is copied to `tools/calendar-store/Cargo.lock`.

Tests, from `linux/impl-2/test.log`:

- library: 3 passed
- `tests/contract.rs`: 2 passed
- `tests/live.rs`: `fresh_pg_contract` passed in 1.45s; `restart_snapshot_persists` filtered out
- doctests: 0 passed

`cargo clippy --locked --offline --all-targets -- -D warnings` exited 0. `smoke.exit` 0, `docker-run.exit` 0, OOMKilled false, privileged false. `cargo-update-invoked.txt` is `no`. Memory 6442450944, NanoCpus 1000000000, CapDrop ALL, network `oo-a289-eef831aa-net`. Container name `oo-a289-impl-2`, removed by the launch script. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, cargo `1.94.0 (85eff7c80 2026-01-15)`.

`fresh_pg_contract` is one live test against private PostgreSQL 17.10. Its pass covers install, replay, project-binding refusal, allowlist freeze, hash drift, column drift, resolved-hash mismatch, invalid `as_of`, compare-and-swap refusal, activation atomicity when the view-owner membership is revoked, runtime and view-owner `42501` on private and business tables, a non-allowlisted reader, and the handwritten April, leap-February, Los Angeles, and São Paulo view rows. The reader assertion compares the stored `as_of` with `statement_timestamp()`.

`linux/restart` restarted `oo-a289-pg` on the host. `pg_postmaster_start_time` changed from `2026-10-06 10:09:12.725641+00` to `2026-10-06 10:12:24.747857+00`. `restart_snapshot_persists` passed in 0.18s on the impl-2 work tree with `--locked --offline`. `smoke.exit` 0. The same 6 GiB and 1 CPU limits. Container name `oo-a289-restart`, removed by the launch script.

PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. No image was pulled. The database after recreate had ACL `{oo_fixture_admin=CTc/oo_fixture_admin,oo_calendar_admin=Cc/oo_fixture_admin,oo_r_calendar=c/oo_fixture_admin,oo_r_other=c/oo_fixture_admin}`.

## Retained failures

`linux/impl-1` is preserved. `smoke.exit` 101, `phases/test.exit` 101, `phases/clippy.exit` 101. The lock subset had already succeeded with the same 208/3 counts. Library and contract tests passed. `fresh_pg_contract` panicked at `tests/live.rs:361`: left `Connect`, right `Query { sqlstate: Some("42501") }`. The test changed the database owner, which removed `oo_calendar_admin` from the database ACL, so the admin could not connect. Clippy rejected unused variable `name` at `src/publish.rs:94`. Container name `oo-a289-impl-1`. OOMKilled false.

The first PostgreSQL start used `CREATE DATABASE oo_a289 OWNER oo_calendar_admin`. `pg/up.exit` is 0 for that start. `pg/up.sh` was later overwritten in place with `OWNER oo_fixture_admin`, so the first script body is not a separate file. `pg/recreate.sh` stopped and removed only `oo-a289-pg` and volume `oo-a289-eef831aa-pgdata`, created them again from the existing secrets, and exited 0. `docker prune` was not used.

## What the store does

Install is one transaction. The SQL file checksum and the catalog shape must match on replay. A foreign `oo_dict` schema or `oo_dict.calendar` is a collision and is left in place. Snapshot rows are inserted once. An equal identity and equal resolved hash replays. A hash or body difference is drift and does not overwrite. Unknown, invalid, and blocked midnights keep their status and a NULL instant.

The view owner `oo_calendar_view` is NOLOGIN. It can select the published metadata and the active-pointer columns the view needs. It has no business-table privilege. Allowlisted runtime roles receive `SELECT` on the view only inside the activation transaction. The view has no project argument.

## Product hashes

| File | SHA-256 |
|---|---|
| `CONTRACT.md` | `83e216c60dc197bb6af1407d793666b1b7f8561d449a5c576b3a9c7041ef4419` |
| `Cargo.lock` | `a3c203e65af360f75cac875143a669e9ae91a73a02fa65d656db4d24db1cbe44` |
| `Cargo.toml` | `d662a1b4571363a3faa5546ebc2c75b68b504ee3078489698db700837dd53e97` |
| `migrations/oo_calendar_control_v1.sql` | `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` |
| `src/error.rs` | `0a558363bc59ee02f3821e62080185231d5dab106c3f0d8294aa003986b81ac1` |
| `src/lib.rs` | `c6c98691b1dcb75b2c0bc72ac8f86b8c8f94b47e38abc8bf8aae8f93831da66f` |
| `src/publish.rs` | `42bee1f09db82399178cfbd1df8852a8713154e441d618a0f107fb76b2660d44` |
| `src/role.rs` | `171688940c4365b67fc209a79846dc2aaa9abfe5ec1470798ce22fba60532ec5` |
| `src/sql.rs` | `aa9f421546c43f4403b50582b8d214e003e3f8bc7bfc50699f907e4861d70b9a` |
| `src/store.rs` | `5e68faf12ed890b4248c9e83a948e62b5ab67f0a3eb8ff853232cae51c570e54` |
| `src/tls.rs` | `7defca6cf75b3650d7247e837085ae1fbfe0f008955757f552e2211b0a6a532b` |
| `tests/contract.rs` | `943f35e7fe27a0ae05ff999732fb323ad48c9d0aeb89863e6c7bf07e1824f52f` |
| `tests/live.rs` | `4481fd4ab1aa95d0b8cb931e0bd3536bb1bce7479bbeacf072540eea34bc3056` |

Copies of those bytes are under `source/blobs/tools/calendar-store/`. The evidence inventory is `MANIFEST.sha256`, written after this report and `CLEANUP.txt`. It does not include itself. The manifest runner writes `linux/manifest/console.log`, `hostconfig.txt`, `docker-run.exit`, and `container-removed.txt` after the inventory.
