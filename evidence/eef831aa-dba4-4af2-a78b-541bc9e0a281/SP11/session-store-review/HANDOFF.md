# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a281`. Reviewed commit `e213ce37140090e843ba98c188009f920962937d`, tree `6b63c32d45fe973cf4671ca41c4ac827c4995636`. Reducer commit `b0f7da4771e3c3625a45329c721e174f4380571c`, reducer tree `a25733870f1ffac55c057dd399270905ccca9151`. Repo HEAD recorded at the proof-3 launch is `5dd7b11a7c7bbbb94e6b16afc82228e82f645275`.

This directory is the independent review of the session-store adapter. The author session `a275` is not this review. The reducer review `a272` is not this review. The parent ledger was not edited. This file does not accept RT-D03 for the parent, does not close SP11, and does not certify a physical release.

The approval covers the adapter at that commit as exercised by the raw exits below. One client transaction locks the row, runs the fixed reducer, and writes only an `Applied` result. Two clients racing one expected revision produce one apply and one stale. An identical retry is `idempotent`. A conflicting payload under a journaled event id is `event_id_conflict` and does not update. A lost COMMIT reply is `commit_unknown`, and a fresh connection reconciles that event. A generation column that disagrees with the JSON is `Consistency`. Reconcile reports `Durable` from event-id presence. The caller supplies `now_ms` and the identity tuple. Project separation is the row key. Public authentication is not implemented.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/` and `source/commit.txt`
- `overlay/a281_session.rs`
- `oracle/registry-triples.txt` and `tools/lock_subset.rs`
- `linux/proof-3/` for the decisive smoke, overlay, Clippy, and lock subset
- `linux/proof/` and `linux/proof-2/` for the preserved harness failures and their overlay sources
- `pg/up.exit`, `CLEANUP.txt`, `MANIFEST.sha256`, `recipe-origin.txt`

## Decisive exits

`linux/proof-3` container `3808862ec9d0662170cc10940ed46f20408796736c31c299ad5f2f3059795203`. `docker-run.exit` 0, `smoke-count.txt` 23, `overlay-count.txt` 8, `test.exit` 0, `clippy.exit` 0, `lock-subset.exit` 0, `product-unchanged.exit` 0. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a281-eef831aa-net`.

`capacity.txt` recorded `own=0 global=0 host_cargo=0 own_postgres=1`. PostgreSQL container `oo-a281-pg`, `pg/up.exit` 0, no host port.

## Preserved attempts

`linux/proof` `docker-run.exit` 1. Smoke 23. Overlay compile `E0277` on `i64::from(u64)`. Do not reread that exit as a product failure.

`linux/proof-2` `docker-run.exit` 1. Smoke 23. Overlay 7 passed and the row-lock observer failed because `oo_session_admin` cannot see another role's `wait_event_type`. Clippy exited 0. The race counts in that stdout are a second fresh sample, winner `ev-a281-beat`.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Postgres image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `RUSTUP_TOOLCHAIN=1.94.0`. `HOME=/tmp`. One builder, 6 GiB, 1 CPU. One private database. `git archive e213ce37140090e843ba98c188009f920962937d`. Keep the committed session-store lock. Do not run `cargo generate-lockfile` or `cargo update`.

```text
cargo fetch --locked
cargo test --locked --offline -- --test-threads=1
cargo test --locked --offline --test a281_session -- --test-threads=1 --nocapture
cargo clippy --locked --offline --all-targets -- -D warnings
```

Mount the fresh fixture at `/run/oo-a275/fixture` so the author live tests can start, and at `/run/oo-a281/fixture` for the overlay. Copy the overlay only after the 23-count smoke. The row-lock observer must use `oo_session_runtime`.

## Cleanup

Own containers, volumes, network, and the tmp secret directory are removed after these reports. `CLEANUP.txt` is the after-state. `oo-a276-pg` and the pinned images stay. Nothing is pruned.
