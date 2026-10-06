# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a297`. Reviewed commit and fixed archive are `cc98bab7eba2c58c127435db4d2ae777797259de`, tree `a9369aab2c53afdf94616abee66b5099adc46d3e`. Helper tree `ea5778dfb87a7cfdb04af2f236db78fe97f7bc4b`. Repo HEAD recorded at the smoke launch is `5ce8b1c4186eb80f9b876a306ae94309f10933ae`.

This directory is the independent review of `tools/calendar-store`. The author session `a289` is not this review. `a283` approved the helper only. The parent ledger was not edited. This file does not accept SEM-D03 for the parent, does not accept a `docs/05` clock-driven calendar view, does not accept authenticated finance close, shared multi-project tenancy, or ontology publication, does not close SP08, and does not certify a release.

The approval covers one project bound at install, explicit `as_of` snapshots, replay and drift, compare-and-swap activation including one winner of two concurrent activations, SQLSTATE `42501` for a revoked view-owner membership and for runtime access to control and business tables, a `NOLOGIN` view owner, the handwritten April, leap-day, DST, gap, and overlap literals, and retention of that row across a recorded postmaster restart. `finance_approval`, `authenticated`, `ontology_published`, and `clock_used` stay false.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/` and `source/commit.txt`
- `overlay/a297_calendar.rs`
- `tools/lock_subset.rs` and `linux/smoke-2/lock-subset.stdout`
- `linux/proof/` for the relative mount failure
- `linux/proof-1/` for the missing root manifest fetch failure
- `linux/proof-2/` for the empty postmaster timestamps
- `linux/smoke-2/` for the 6-test author smoke
- `linux/overlay-1/` for the author restart read, `independent_store`, and Clippy
- `linux/restart/` for the second postmaster change and `restart_keeps_the_explicit_row`
- `CLEANUP.txt`, `MANIFEST.sha256`, `recipe-origin.txt`

## Decisive exits

`linux/smoke-2` container `fa3052dc6e9b4d5be849d134ad9a0ffc5c9d1e45005ed384a91269917d1c2616`. `docker-run.exit` 0. `smoke-test.exit` 0, `smoke-count.txt` 6, `lock-subset.exit` 0, `lock-summary.exit` 0, `fetch.exit` 0, `product-unchanged.exit` 0, `workspace-probe.exit` 101, `root-manifest-used.txt` `harness`.

`linux/overlay-1` container `9ab0b75484bceb20cee12ea323b3badd114ef34a950853cc7fcff29917f1b617`. `docker-run.exit` 0. Postmaster `2026-10-06 11:01:15.444815+00` then `2026-10-06 11:02:21.551234+00`. `author-restart.exit` 0, `test.exit` 0, `overlay-count.txt` 1, `clippy.exit` 0, `product-unchanged.exit` 0.

`linux/restart` container `9f692a1253f2b090b0568f4fdef1478d935070c47522c14767e985a09775e12f`. `docker-run.exit` 0. Postmaster `2026-10-06 11:02:21.551234+00` then `2026-10-06 11:03:54.104408+00`. `test.exit` 0, `overlay-count.txt` 1, `product-unchanged.exit` 0.

Each successful builder hostconfig has Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a297-eef831aa-net`.

Child lock `a3c203e65af360f75cac875143a669e9ae91a73a02fa65d656db4d24db1cbe44`. Root lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Registry summary `208/0/0` with three path packages.

## Preserved attempts

`linux/proof` `docker-run.exit` 125. No container.

`linux/proof-1` `docker-run.exit` 101. Lock subset passed. Fetch failed closed on the absent root manifest.

`linux/proof-2` `docker-run.exit` 1. Restart ran. Timestamp query failed before the builder.

`a289` `linux/impl-1` and the overwritten first `pg/up.sh` stay in the author directory. This review did not edit them.

## Re-run

Pinned Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Pinned Postgres image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `RUSTUP_TOOLCHAIN=1.94.0`. One builder, 6 GiB, 1 CPU, and one private Postgres. Archive `Cargo.toml`, `Cargo.lock`, `crates/oo-core`, `tools/business-calendar`, `tools/calendar-store`, and `examples/schema/calendar.yaml` from `cc98bab7eba2c58c127435db4d2ae777797259de`. If the archived root manifest cannot load because other members are absent, use the harness in `linux/smoke-2/harness-Cargo.toml` as the work-tree root only. Keep both committed locks. Do not run `cargo update`.

```text
cargo test --locked --offline -- --test-threads=1 --skip restart_snapshot_persists
cargo test --locked --offline --test live restart_snapshot_persists -- --test-threads=1 --exact
cargo test --locked --offline --test a297_calendar independent_store -- --test-threads=1 --exact
cargo clippy --locked --offline --all-targets -- -D warnings
cargo test --locked --offline --test a297_calendar restart_keeps_the_explicit_row -- --test-threads=1 --exact
```

Run the author restart test after a real postmaster change and before copying the overlay. Run the retention test after a second postmaster change, without dropping the overlay schemas. Query `pg_postmaster_start_time()` as `oo_fixture_admin`. The database name the author test requires is `oo_a289`.

`linux/manifest/launch.sh` hashes this directory. Its console path stays outside the review directory. `CLEANUP.txt` records removal of this task's container, volumes, network, and secret directory.
