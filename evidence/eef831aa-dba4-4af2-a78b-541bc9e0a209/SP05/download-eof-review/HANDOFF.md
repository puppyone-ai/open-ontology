# HANDOFF — a209 review of the local download EOF repair

Verdict: **APPROVE LIMITED** on `5e3ba29f8d460a1aa6919733100c34a68a2fdb75`, tree `846870547f225b2bdddc86385539d4c0c79f4427`, parent `b3f8f6161c40c87686c1159421b800a53f874d6f`. No new component.

Author evidence `84c03d01ce35728924100e42ca142c0ef650ac71`. Prior review `ef6c4fc7da03342f900146e90ff692fe742d1ba4`. This directory is the independent measurement. It is not acceptance of SP05-W01 or SP05-W06, not a ledger update, and not a production release. Do not mark a parent checkbox from this run.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `SHA256SUMS` (every file here except itself)
- `source-commit.txt` and `source/blobs.txt`
- `source/blobs/` (product files at the fixed commit; `Cargo.lock` is hash-only)
- `pre-edit-attempt-1/console.log` and `pre-edit-attempt-1/docker-run.exit` (1)
- `pre-edit/console.log`, `pre-edit/docker-run.exit` (0), `pre-edit/phases/worst.exit` (0)
- `oracle-attempt-0/ATTEMPT.txt` (launch script was not executable; no container)
- `oracle-attempt-1/console.log` and `oracle-attempt-1/docker-run.exit` (101)
- `oracle-attempt-2/console.log` and `oracle-attempt-2/docker-run.exit` (101)
- `oracle-attempt-3/console.log`, `oracle-attempt-3/docker-run.exit` (101), `oracle-attempt-3/notes/`
- `oracle/console.log`, `oracle/docker-run.exit` (0), `oracle/phases/worst.exit` (0), `oracle/archive-after.txt`, `oracle/notes/`
- `oracle/ran/review_download.rs` (the overlay rustfmt 1.94 actually compiled)
- `contracts/control/local-file-hosting-v1.md` at the fixed commit, Download section

## What this run measured

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One Rust builder at a time. No PostgreSQL. Private network `oo-a209-eef831aa-net`. Private volumes `oo-a209-eef831aa-cargo` and `oo-a209-eef831aa-target`.

Smoke of the committed suite, before any overlay: 16 `file_hosting` tests passed in 37.99s, fmt and clippy clean, lock unchanged. The first smoke died in workspace rustfmt and is preserved.

Green oracle: 3 review tests passed in 21.83s, clippy `-D warnings` clean, product sha256 values and the lock unchanged. On a real `LocalFileSystem`, a consumed-prefix flip before EOF was `Corrupt` and a clean EOF completed. Deadline checks around `get_opts` and `bytes()` held, including a 1500 ms `get_opts` that was not preempted. Rehash ranges stayed at or under 8192. Source, manifest, and running hashes matched on the clean pulls. An etag-only change across the rehash was `Corrupt` with identical bytes. Fresh-child resident deltas on pattern `0x5A` were 65536 bytes (8 MiB) and 36864 bytes (32 MiB).

## Limits to carry

An in-flight filesystem await is not preempted. `is_complete` is not a promise that the file stays unchanged, and a later write does not clear it. A new open of the changed file is `Corrupt`. Truncate during a range read was `Store`, and the pull stayed incomplete. `read_object` still reserves one object-sized buffer. HTTP, an auth proxy, TTL, revocation, a reaper, and S3 remain open.

## Cleanup

`cleanup-done.txt` is the command log. Launch traps already removed `oo-a209-smoke` and `oo-a209-oracle`. The log removes network `oo-a209-eef831aa-net`, volumes `oo-a209-eef831aa-cargo` and `oo-a209-eef831aa-target`, and `/tmp/oo-a209-eef831aa-dba4-4af2-a78b-541bc9e0a209`. Nothing is pruned. Foreign containers are left in place.

## Next reader

Do not treat the author run as this review. Do not close the ledger from this directory.
