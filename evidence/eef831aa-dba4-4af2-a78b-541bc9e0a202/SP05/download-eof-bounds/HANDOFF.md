# HANDOFF — a202 local download EOF and await repair

Repair commit `5e3ba29f8d460a1aa6919733100c34a68a2fdb75`, tree `846870547f225b2bdddc86385539d4c0c79f4427`, parent `b3f8f6161c40c87686c1159421b800a53f874d6f`. Fixed commit `e3363ffecc21af4dc9fcf75509978da34b0860b1`. Review under repair: `ef6c4fc7da03342f900146e90ff692fe742d1ba4`. Original author evidence `a699f8da3dfdae918eb1129509d074c7d1d3bdce`.

This directory records the repair and the Linux 1.94 measurements. It is not acceptance of the repair, not acceptance of SP05-W01, not a ledger update, and not a production release. Do not mark a parent checkbox from this run.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `SHA256SUMS` (every file here except itself)
- `source-commit.txt` and `source/blobs.txt`
- `source/blobs/` (the three product files plus unchanged `lib.rs` and `crates/oo-control/Cargo.toml`; `Cargo.lock` is hash-only)
- `pre-edit/console.log`, `pre-edit/docker-run.exit` (0), `pre-edit/phases/worst.exit` (0)
- `pre-edit/repro-notes/consumed-prefix.txt` and `pre-edit/repro-notes/held-get.txt`
- `pre-edit/repro_download_eof.rs` (evidence overlay; absent from the product commit)
- `verify/console.log`, `verify/docker-run.exit` (0), `verify/phases/worst.exit` (0), `verify/archive-after.txt`, `verify/metrics/`
- `verify-attempt-1/console.log` and `verify-attempt-1/docker-run.exit` (101)
- `verify-attempt-2/console.log`, `verify-attempt-2/docker-run.exit` (101), `verify-attempt-2/ATTEMPT.txt`
- `contracts/control/local-file-hosting-v1.md` at the repair commit
- a194 `HANDOFF.md` for the two limits this repair answers
- a188 `HANDOFF.md` for the quota and 8192 publish measurement only

## What this run measured

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One Rust builder at a time. No PostgreSQL. Private network `oo-a202-eef831aa-net`. Private volumes `oo-a202-eef831aa-cargo` and `oo-a202-eef831aa-target`.

Pre-edit smoke of the committed download: 13 `file_hosting` tests passed in 19.80s, fmt and clippy clean, lock unchanged. The overlay then reproduced both a194 defects on unmodified code: a consumed-prefix flip completed (`consumed_prefix_flip_complete=1`), and a held `get_opts` started `bytes()` after the deadline (`bytes_started_after_deadline_during_get_opts=1`) without dropping the in-flight get.

Green verify of the repair: 16 `file_hosting` tests passed in 36.45s, including the three new tests and the 8 MiB and 32 MiB download. Clippy `-D warnings` clean. Lock unchanged from the archive parent. Download high-water deltas 77824 and 73728 bytes, both under 4 MiB and under half the object, chunk max 8192, `VmPeak` flat, `fixture_object_vec=0`. The ceiling was not raised. Pre-edit deltas on that same test were 8192 and 4096.

Attempt 1 failed to compile the tests (missing `ObjectStoreExt`). Attempt 2 passed the 16 tests and failed clippy on `while_let_loop`. Both consoles are kept.

## Still open for the fixed review

Whether the two digests, the manifest bytes, the length, and the local stat window match the contract on a hostile store, not only on `LocalFileSystem` and the `CountingStore` in the test. Whether a head that changes only `e_tag` during the rehash is `Corrupt` when both hashes would match. The product test covers a real file flip and a generic-store flip. It does not inject an etag change with identical bytes.

S3 and MinIO, conditional quota on a generic store, quota across hosts that do not share one deployment directory, `dest#N` leftovers, HTTP, an auth proxy, grant authentication, revocation, TTL, presigned URLs, formula-safe output, retention delete, and a reaper remain open. `read_object` still returns one buffer of the object size.

A passed deadline does not cancel an await that has already been polled. The host does not preempt a filesystem read that has started.

## Cleanup

`cleanup-done.txt` is the command log. The launch traps had already removed `oo-a202-pre-edit` and `oo-a202-verify`. The log removes network `oo-a202-eef831aa-net`, volumes `oo-a202-eef831aa-cargo` and `oo-a202-eef831aa-target`, and `/tmp/oo-a202-eef831aa-dba4-4af2-a78b-541bc9e0a202`. Nothing is pruned. Foreign containers are left in place.

## Next reader

An independent fixed review of `5e3ba29f8d460a1aa6919733100c34a68a2fdb75`. Do not treat this author run as that review. Do not treat a188 or a194 as a measurement of this commit.
