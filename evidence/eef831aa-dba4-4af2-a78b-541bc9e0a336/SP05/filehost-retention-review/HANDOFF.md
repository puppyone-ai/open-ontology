# HANDOFF — a336 filehost retention review

Verdict: **APPROVE_LIMITED**. `parent_complete` is false. Fixed archive `53931278e69d723d0ce38961ded897fd6e41d5d3`. Product ancestor `c27525a2a7c31dfbe9cb64b183312c7dab354511`. Contract ancestor `e6ebf93b5ac261c097a24d145611e0300e4f88b5`. Author baseline `895864a5bf64e50cef84075d20fd7b0c9e41c264`. Preoverlay and review `docker-run.exit` are 0. `phases/worst.exit` is 0 on both green runs. Those exits do not accept SP05-W06, AT-FILE-05, AT-SEC-03, the ledger, or a release. Do not mark a parent checkbox from this directory. Do not add a countable unit.

Owner UUID `eef831aa-dba4-4af2-a78b-541bc9e0a336`. This evidence path is the review. Product paths were read-only. The author measurement is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a323/SP05/filehost-retention-delete`. The original E41 packet is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a311/SP05/artifact-retention` and `execution/prompts/v01-artifact-retention-active.md`.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `SHA256SUMS` (every file here except itself; the sums-file hash is in the evidence commit message)
- `source/rust-inventory.txt` and `source/blobs.txt`
- `source/blobs/oo-control/src/file_hosting.rs` (git blob `aa16be987070522105b7b4079ed00ef89899b7f3`)
- `source/blobs/artifact-retention/` including `FILEHOST-DELETE-CONTRACT.md` (git blob `cb8de1af2712005b481ac2a0a792a9cff0ab9014`) and `Cargo.lock`
- `source/blobs/review_independent.rs` (the overlay, sha256 `d8ad6b23a4e0d8428beeebf5d7ff5040da76fe4f22e0f2ef4eeb32c7f8f3fdb3`)
- `source/blobs/lock_from_baseline.rs` (Safe Rust lock oracle, sha256 `9441d4911c379715d4ccdac39e60dfe62d0a20fcc9f313a0bce7372fafd83245`)
- `linux/preoverlay/console.log`, `docker-run.exit` 0, ends `preoverlay=pass`
- `linux/preoverlay-attempt-1/console.log`, `docker-run.exit` 101, offline metadata before fetch, missing `tokio`
- `linux/review-attempt-1/console.log`, `docker-run.exit` 1, rustfmt diff of the review test only
- `linux/review/console.log`, `docker-run.exit` 0, `phases/worst.exit` 0, `hostconfig.txt`, `recipe-origin.txt`
- `contracts/control/local-file-hosting-v1.md` quota section at the fixed commit
- `tools/artifact-retention/FILEHOST-DELETE-CONTRACT.md`

## What this run measured

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One Rust builder, 6 GiB, 1 CPU. No PostgreSQL. Private network `oo-a336-eef831aa-net`. Private volumes `oo-a336-eef831aa-cargo` and `oo-a336-eef831aa-target`.

Preoverlay, before any review overlay: 16 `file_hosting` tests passed in 39.20s. `new_returns_unsupported_before_store_io` passed. 13 retention live tests passed in 0.16s. The lock-subset test passed (`registry_pins=185`, `packages=189`). Clippy `-D warnings` exited 0 on both packages.

Review overlay: 7 tests passed in 0.13s. Clippy `-D warnings` on artifact-retention exited 0. The quota console line is `quota_window old_bytes=64 reserved=64 max=64 reservation_within_max=true bytes_plus_reservation_exceeds_max=true`.

A published object is created, completed, downloaded, expired, and reaped. Observation is then `Ok(true)`. The release count is 1. `parent_complete` stays false. A foreign project inside its uploaded-source window stays readable. Wrong project, wrong grant, wrong digest, hash drift, symlink, and a manifest replaced by a directory do not delete and are not `Ok(true)` absence. SIGKILL at `quota_released` is signal 9, prints no success, and restart releases once. An unfinished pull stays incomplete. Open abort stays `aborted_parts_unverified`. `dest#1` stays. `bearer_url` is `none`. Gateway URL revocation is `not_implemented`.

## Limit to carry

During `quota_released`, one new reservation of the freed size is admitted while the old object bytes remain. The reservation sum stays within `max_project_bytes`. Disk bytes plus that new reservation exceed the cap. Same-config begin does not reserve twice. A symlink after the release is `Corrupt`, is not unlinked, does not release again, and does not restore the reservation. A further begin is then `Quota` while the stuck bytes remain.

These seven names remain in `required_integration()`:

- `filehost_part_absence_observation`
- `filehost_dest_staging_reap`
- `filehost_object_version`
- `filehost_admission_role_and_policy`
- `gateway_authenticated_url_revocation`
- `formula_safe_csv_excel`
- `sessions_outputs_prefix`

## Cleanup

`cleanup-done.txt` is the command log. Launch scripts already removed `oo-a336-preoverlay` and `oo-a336-review`. The log removes network `oo-a336-eef831aa-net`, volumes `oo-a336-eef831aa-cargo` and `oo-a336-eef831aa-target`, and `/tmp/oo-a336-eef831aa-dba4-4af2-a78b-541bc9e0a336`. Nothing is pruned. Foreign containers are left in place. preoverlay-attempt-1 and review-attempt-1 stay in this directory.

## Next reader

Treat `linux/preoverlay` and `linux/review` as the green runs. Treat the two attempt directories as preserved failures. The tested tree is the archive of `53931278`, not whatever HEAD is when you read this. Do not read exit 0 as parent acceptance. Do not close E41, SP05-W06, AT-FILE-05, or AT-SEC-03 from this directory. Do not shrink the seven remaining seams. Do not add a countable unit.
