# HANDOFF — a311 local artifact retention

Verdict: `parent_complete` is false. Fixed commit `cab5e8e219d1924070df7336f5ab5d4ce7cf40dc`. Proof-6 `docker-run.exit` is 0 and `phases/worst.exit` is 0. That exit does not accept SP05-W06, AT-FILE-05, AT-SEC-03, the ledger, or a release. Do not mark a parent checkbox from this directory.

Owner UUID `eef831aa-dba4-4af2-a78b-541bc9e0a311`. Owned product path `tools/artifact-retention/**`. This evidence path is the measurement. Worktree HEAD when the handoff was written: `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96`. The proof archive was taken while `linux/proof-6/repo-head.txt` was `773c461104729d6c289eae8640c7c2a28207c447`. The fixed commit is an ancestor of both. Root `Cargo.lock` and `file_hosting.rs` still match the fixed commit. The only root manifest delta since the fixed commit is the unapproved axum/tower/tower-http workspace pins in `Cargo.toml`. This task did not edit them.

This resume collected the finished proof-6 result. It did not start another builder or a PostgreSQL container.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `SHA256SUMS` (every file here except itself; the sums-file hash is in the evidence commit message)
- `source/rust-inventory.txt` and `source/blobs.txt`
- `source/blobs/artifact-retention/` (the sources proof-6 compiled, including `Cargo.lock`)
- `source/blobs/lock_from_baseline.rs` (Safe Rust lock oracle, `forbid(unsafe_code)`)
- `linux/pre-edit/console.log`, `docker-run.exit` 0, ends `smoke=pass`
- `linux/proof-1/console.log`, `docker-run.exit` 126 (`/tmp` is `noexec`)
- `linux/proof-2/console.log`, `docker-run.exit` 101, `lock-method.txt` `metadata-offline`
- `linux/proof-3/console.log`, `docker-run.exit` 101, `lock-method.txt` `prebuilt-locked`
- `linux/proof-4/console.log`, `docker-run.exit` 101, `phases/test.exit` 0, `phases/clippy.exit` 101
- `linux/proof-5/console.log`, `docker-run.exit` 101, `phases/test.exit` 0, `phases/clippy.exit` 101, and the unclosed quote in `launch.sh` lines 39–43
- `linux/proof-6/console.log`, `docker-run.exit` 0, `phases/worst.exit` 0, `hostconfig.txt`, `recipe-origin.txt`
- `tools/artifact-retention/CONTRACT.md` (`oo.artifact-retention/v1`)

## What this run measured

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One Rust builder, 6 GiB, 1 CPU. No PostgreSQL. Private network `oo-a311-eef831aa-net`. Private volumes `oo-a311-eef831aa-cargo` and `oo-a311-eef831aa-target`.

Smoke of `oo-control` file hosting, before the overlay: `smoke=pass`. It was not rerun on this resume.

proof-6: 8 live tests passed in 0.11s and 1 lock-subset test passed. Clippy `-D warnings` exited 0 in 0.36s. Child lock stayed `--locked`. Root lock and `file_hosting.rs` were unchanged inside the container.

The published-object reap leaves the object readable. Status `unsupported_completed_object_delete`. `deletion_success` and `parent_complete` stay false. Replay returns the same session. The filled project still returns `Quota`. The unregistered object survives. Drift and read errors do not call `abort`. Open `abort` stays `aborted_parts_unverified`. A kill between `metadata_api_done` and the object step stays partial. The CLI `--now` argument exits 1. `bearer_url` is `none`. Gateway URL revocation is `not_implemented`.

## Limits to carry

Completed-object delete, quota release on that delete, public part-absence observation, `dest#N` reap, an object version on the receipt, host admission of role and policy, authenticated gateway URL revocation, formula-safe CSV/Excel, and a `sessions/<id>/outputs/` prefix are the nine names in `required_integration()`. They are remaining integration. `parent_complete` stays false until a host API can delete a completed object and a post-read can prove the object, sidecars, and multipart parts absent.

Workspace 24h retention is explicit and default off. Uploaded-source retention is a separate 180 day class. Guessed paths and symlink roots are refused. The reaper deletes only through `FileHost::abort`, and only for a registration taken from a host receipt.

## Cleanup

`cleanup-done.txt` is the command log. Launch scripts already removed the named proof containers. The log removes network `oo-a311-eef831aa-net`, volumes `oo-a311-eef831aa-cargo` and `oo-a311-eef831aa-target`, and `/tmp/oo-a311-eef831aa-dba4-4af2-a78b-541bc9e0a311`. Nothing is pruned. Foreign containers are left in place. proof-1 through proof-6 stay in this directory.

## Next reader

Treat proof-6 as the focused result and proof-1 through proof-5 as preserved failures. Do not read exit 0 as parent acceptance. Do not close the ledger from this directory. A later host delete API is a new seam and needs its own frozen contract before code that claims absence.
