# HANDOFF — a338 retention completion

Verdict: `parent_complete` is false. Product commit `def1f7eea53794449d3975b1987d700727cb7d60`. Addendum commit `9cdbc625d02fca2fc3fd31b62ce8cb03692a9b81`. Baseline `e9cba7df4b8d20ea8b3794f9efcc813434d7b1dd`. Prior product `c27525a2a7c31dfbe9cb64b183312c7dab354511`. Historical contract `e6ebf93b5ac261c097a24d145611e0300e4f88b5`. Post-edit-4 `docker-run.exit` is 0 and `phases/worst.exit` is 0. That exit does not accept SP05-W06, AT-FILE-05, AT-SEC-03, the ledger, or a release. Do not mark a parent checkbox from this directory.

Owner UUID `eef831aa-dba4-4af2-a78b-541bc9e0a338`. Owned product paths are `crates/oo-control/src/file_hosting.rs` and `tools/artifact-retention/**`. This evidence path is the measurement. Post-edit-4 launched at worktree `0a4ae4baee11d0c45f7725035439164ed2fcc8d7`. The seven overlaid files in that snap match the product commit. The baseline is an ancestor. Root `Cargo.lock`, `crates/oo-control/src/lib.rs`, `crates/oo-control/Cargo.toml`, `crates/oo-control/tests/file_hosting.rs`, and `tools/artifact-retention/Cargo.toml` were not part of the product commit. The addendum file was committed in `9cdbc625` and left unchanged.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `SHA256SUMS` (every file here except itself; the sums-file hash is in the evidence commit message)
- `source/rust-inventory.txt` and `source/blobs.txt`
- `source/blobs/oo-control/src/file_hosting.rs`
- `source/blobs/artifact-retention/` including `FILEHOST-COMPLETION-ADDENDUM.md`, `FILEHOST-DELETE-CONTRACT.md`, and `Cargo.lock`
- `source/blobs/lock_from_baseline.rs` (Safe Rust lock oracle, sha256 `9441d4911c379715d4ccdac39e60dfe62d0a20fcc9f313a0bce7372fafd83245`)
- `linux/pre-edit/console.log`, `docker-run.exit` 0, ends `preedit=pass`
- `linux/post-edit/console.log`, `docker-run.exit` 101, planted tombstone parent missing
- `linux/post-edit-2/console.log`, `docker-run.exit` 101, clippy `collapsible_if` at `file_hosting.rs:1970`
- `linux/post-edit-3/console.log`, `docker-run.exit` 101, repeated reap after same-config `begin` exited 2
- `linux/post-edit-4/console.log`, `docker-run.exit` 0, `phases/worst.exit` 0, `hostconfig.txt`, `recipe-origin.txt`
- `tools/artifact-retention/FILEHOST-COMPLETION-ADDENDUM.md` (frozen before the product commit)
- `tools/artifact-retention/FILEHOST-DELETE-CONTRACT.md` and `CONTRACT.md` (banner points at the addendum; the historical body remains)

## What this run measured

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One Rust builder, 6 GiB, 1 CPU. No PostgreSQL. Private network `oo-a338-eef831aa-net`. Private volumes `oo-a338-eef831aa-cargo` and `oo-a338-eef831aa-target`. Five builders were launched, one at a time. The development cap is 20. This directory is not a review.

Smoke of the baseline archive, before the overlay: `preedit=pass`. 16 `file_hosting` tests, 1 lib test, 13 live tests.

post-edit-4: 16 existing `file_hosting` tests passed in 36.52s. 6 lib tests passed, including the five retention-completion tests. 14 retention live tests passed in 0.24s. The lock-subset test passed (`registry_pins=185`, `packages=189`). Clippy `-D warnings` exited 0 on both packages. Child lock stayed `--locked`. Root lock, `lib.rs`, the control manifest, and a sampled foreign `oo-builder` lib stayed byte-identical inside the container.

A published object past expiry is deleted through `FileHost::delete_completed`. The quota key remains until the bytes, sidecars, parts, and platform staging are absent. Observation then reports absence and the release count is 1. A repeated reap stays at 1. A begin while the reservation is held is `Quota`. After `verified`, that size can be begun again. SIGKILL at `data_deleting` keeps the bytes and the quota. SIGKILL at `data_absent` has already removed the bytes and still holds the quota; restart releases once. An open abort proves part absence through `observe_open_absence` and the host reaps platform `{basename}#{n}` files. Foreign `dest#1`, `notes`, `original#01`, and a neighbor file stay. `parent_complete` stays false. `deletion_success` can be true when every expired row has one of the two real proofs.

## Limits to carry

These five names remain in `required_integration()`:

- `filehost_object_version`
- `filehost_admission_role_and_policy`
- `gateway_authenticated_url_revocation`
- `formula_safe_csv_excel`
- `sessions_outputs_prefix`

`filehost_part_absence_observation` and `filehost_dest_staging_reap` left that list on the post-edit-4 proof. Two of the seven a336 seams are done in this child. Five remain. Plan SP05-W06, AT-FILE-05, and AT-SEC-03 stay open.

`FileHost::new` is still `Unsupported`. Workspace 24h retention is explicit and default off. Uploaded-source retention is a separate 180 day class. Session keys stay `uploads/<session>/<sha256>/original`. `abort` of a completed session still deletes nothing. `bearer_url` is `none`. Gateway URL revocation is `not_implemented`.

`finish_abort` accepts `NotFound` plus `observe_open_absence` `Ok(true)` as a verified open row. Addendum line 132 names `abort` `Ok` together with that observation. The frozen addendum was not edited. The proved open path is the first `abort` returning `Ok`.

## Cleanup

`cleanup-done.txt` is the command log. Launch scripts already removed the named proof containers. The log removes network `oo-a338-eef831aa-net`, volumes `oo-a338-eef831aa-cargo` and `oo-a338-eef831aa-target`, and `/tmp/oo-a338-eef831aa-dba4-4af2-a78b-541bc9e0a338`. Nothing is pruned. Foreign containers are left in place. pre-edit and post-edit through post-edit-4 stay in this directory.

## Next reader

Treat post-edit-4 as the focused result and post-edit through post-edit-3 as preserved failures. Do not read exit 0 as parent acceptance. Do not close the ledger from this directory. Independent review is a separate slot and was not run here.
