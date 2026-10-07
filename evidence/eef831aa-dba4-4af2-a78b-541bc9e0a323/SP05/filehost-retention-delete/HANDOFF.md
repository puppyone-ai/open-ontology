# HANDOFF — a323 completed-object delete

Verdict: `parent_complete` is false. Product commit `c27525a2a7c31dfbe9cb64b183312c7dab354511`. Contract commit `e6ebf93b5ac261c097a24d145611e0300e4f88b5`. Baseline `895864a5bf64e50cef84075d20fd7b0c9e41c264`. Proof-3 `docker-run.exit` is 0 and `phases/worst.exit` is 0. That exit does not accept SP05-W06, AT-FILE-05, AT-SEC-03, the ledger, or a release. Do not mark a parent checkbox from this directory.

Owner UUID `eef831aa-dba4-4af2-a78b-541bc9e0a323`. Owned product paths are `crates/oo-control/src/file_hosting.rs` and `tools/artifact-retention/**`. This evidence path is the measurement. Proof-3 launched at worktree `3517cd131719cfdd572ba2b85bb7013e6c90588e`. The baseline is an ancestor. Root `Cargo.lock`, `crates/oo-control/src/lib.rs`, and `crates/oo-control/Cargo.toml` were not part of the product commit.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `SHA256SUMS` (every file here except itself; the sums-file hash is in the evidence commit message)
- `source/rust-inventory.txt` and `source/blobs.txt`
- `source/blobs/oo-control/src/file_hosting.rs`
- `source/blobs/artifact-retention/` including `FILEHOST-DELETE-CONTRACT.md` and `Cargo.lock`
- `source/blobs/lock_from_baseline.rs` (Safe Rust lock oracle, `forbid(unsafe_code)`, sha256 `9441d4911c379715d4ccdac39e60dfe62d0a20fcc9f313a0bce7372fafd83245`)
- `linux/pre-edit/console.log`, `docker-run.exit` 0, ends `smoke=pass`
- `linux/proof-1/console.log`, `docker-run.exit` 101, `oo-control` clippy 8/7 and `manual_is_multiple_of`
- `linux/proof-2/console.log`, `docker-run.exit` 101, retention tests passed, clippy `collapsible_if`, and `cargo fmt -p oo-control` rewrote foreign files in that snap only
- `linux/proof-3/console.log`, `docker-run.exit` 0, `phases/worst.exit` 0, `hostconfig.txt`, `recipe-origin.txt`
- `tools/artifact-retention/FILEHOST-DELETE-CONTRACT.md` (frozen before the product commit)
- `tools/artifact-retention/CONTRACT.md`

## What this run measured

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One Rust builder, 6 GiB, 1 CPU. No PostgreSQL. Private network `oo-a323-eef831aa-net`. Private volumes `oo-a323-eef831aa-cargo` and `oo-a323-eef831aa-target`.

Smoke of `oo-control` file hosting, before the overlay: `smoke=pass`.

proof-3: 16 existing `file_hosting` tests passed in 37.79s. `new_returns_unsupported_before_store_io` passed. 13 retention live tests passed in 0.15s. The lock-subset test passed (`registry_pins=185`, `packages=189`). Clippy `-D warnings` exited 0 on both packages. Child lock stayed `--locked`. Root lock, `lib.rs`, the control manifest, the delete contract, and a sampled foreign `oo-builder` lib stayed byte-identical inside the container.

A published object past expiry is deleted through `FileHost::delete_completed`. Observation then reports the object, manifest, and five-digit parts absent. The quota release count is 1 on a repeated reap. A second project inside its uploaded-source window stays readable. A wrong digest or a wrong tuple does not delete. A kill at `quota_released` does not print success and does not release quota twice. `parent_complete` stays false. Open abort stays `aborted_parts_unverified`. `dest#1` is left in place. `bearer_url` is `none`. Gateway URL revocation is `not_implemented`.

## Limits to carry

These seven names remain in `required_integration()`:

- `filehost_part_absence_observation`
- `filehost_dest_staging_reap`
- `filehost_object_version`
- `filehost_admission_role_and_policy`
- `gateway_authenticated_url_revocation`
- `formula_safe_csv_excel`
- `sessions_outputs_prefix`

Workspace 24h retention is explicit and default off. Uploaded-source retention is a separate 180 day class. The operator UUID is the deployment directory name. `FileHost::abort` of a completed session still deletes nothing.

## Cleanup

`cleanup-done.txt` is the command log. Launch scripts already removed the named proof containers. The log removes network `oo-a323-eef831aa-net`, volumes `oo-a323-eef831aa-cargo` and `oo-a323-eef831aa-target`, and `/tmp/oo-a323-eef831aa-dba4-4af2-a78b-541bc9e0a323`. Nothing is pruned. Foreign containers are left in place. pre-edit and proof-1 through proof-3 stay in this directory.

## Next reader

Treat proof-3 as the focused result and proof-1 and proof-2 as preserved failures. Do not read exit 0 as parent acceptance. Do not close the ledger from this directory. Independent review is a separate slot and was not run here.
