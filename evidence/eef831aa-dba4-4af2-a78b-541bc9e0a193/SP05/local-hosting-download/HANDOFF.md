# HANDOFF — a193 local object download pull

Product commit `e3363ffecc21af4dc9fcf75509978da34b0860b1`. Parent `33426a733d8f569a74e8074745a2a77528b1cb53`. Baseline `15d23fe9f2cbc74e3f3278d70d0f56390568126e`. This directory is the author run. Independent review is next. This is not acceptance of SP05-W01 or SP05-W06, not a ledger update, and not a production release.

Do not mark AT-FILE-01, AT-FILE-05, SP05-W01, or SP05-W06 from this run. Other development slots own the parser worker, the policy catalog, and CSV. This directory does not authorize edits there.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `SHA256SUMS` (every file here except itself)
- `source-commit.txt`, `source/blobs.txt`, and `source/blobs/` (the three committed files plus unchanged `lib.rs` and `crates/oo-control/Cargo.toml`; `Cargo.lock` is hash-only)
- `pre-edit/console.log`, `pre-edit/docker-run.exit` (0), `pre-edit/phases/worst.exit` (0), `pre-edit/archive-match.txt`
- `post-edit/console.log`, `post-edit/docker-run.exit` (0), `post-edit/phases/worst.exit` (0), `post-edit/overlay-after.sha256`, `post-edit/unrelated-match.txt`, `post-edit/fmt-sync.txt`
- `post-edit/metrics/download-memory-8388608.txt` and `download-memory-33554432.txt`
- `post-edit/metrics/memory-8388608.txt` and `memory-33554432.txt`
- `contracts/control/local-file-hosting-v1.md` at the product commit
- a188 `HANDOFF.md`, evidence commit `b6cddea1`, as the limited approval of the quota and 8192 publish path only

## What this run measured

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One Rust builder at a time. No PostgreSQL. Private network `oo-a193-eef831aa-net`. Private volumes `oo-a193-eef831aa-cargo` and `oo-a193-eef831aa-target`.

Pre-edit archive of `15d23fe9`: 10 `file_hosting` tests passed, fmt and clippy `-D warnings` clean, lock unchanged. The overlay was absent.

Post-edit: rustfmt copied `tests/file_hosting.rs` back. 13 tests passed in 19.84s. Clippy `-D warnings` clean. Lock, `lib.rs`, and `oo-control/Cargo.toml` still match the baseline blobs. The three committed file hashes match `post-edit/overlay-after.sha256`.

`open_download` / `next_chunk` on `LocalFileSystem`: 8388608-byte pull resident delta 8192 bytes, sha256 `e474a45740e5efac02e9f7e0626b3f86111b293f8363e477786dfad64e98df3b`. 33554432-byte pull resident delta 4096 bytes, sha256 `edeadec8f638055689d5be63b4bcf2654fb64bf91fb6651e9a924f052a9c7db0`. Pattern, pull, and file digests match. Both are Safe Rust `sha2`. The a186 quota and `complete` tests passed again in the same binary.

The contract paragraph on a poisoned wait now matches `lock_mutex`: `into_inner`, and the wait continues. `Unavailable` is still the timeout and the lock-file error. No poison fault was injected.

## Still open

S3 and MinIO, conditional quota on a generic store, quota across hosts that do not share one deployment directory, `dest#N` leftovers, HTTP, an auth proxy, grant authentication, revocation, TTL, presigned URLs, formula-safe output, retention delete, and a reaper. `read_object` still returns one buffer of the object size. The pull checks the trusted project and principal on the published manifest. It is not a public download URL.

## Cleanup

`cleanup-done.txt` is the command log. `docker rm -f` of `oo-a193-pre-edit` and `oo-a193-post-edit` printed `No such container` and exited 0; the launch traps had already removed them. The same log removed network `oo-a193-eef831aa-net`, volumes `oo-a193-eef831aa-cargo` and `oo-a193-eef831aa-target`, `/tmp/oo-a193-eef831aa-dba4-4af2-a78b-541bc9e0a193`, and `/tmp/oo-a193-commit.txt`. `oo-a190-pg`, `oo-a190-rust194`, and `oo-a191-pg` were still present afterward. Nothing was pruned.

## Next reader

Review the product commit above. Do not treat a188's measurement as this run's download proof. A later HTTP, TTL, or revocation change needs its own review.
