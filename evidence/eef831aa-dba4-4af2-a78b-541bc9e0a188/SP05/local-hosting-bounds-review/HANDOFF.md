# HANDOFF — a188 local hosting bounds review

Verdict: **APPROVE LIMITED**. One local deployment directory passed the quota critical section and the 8192-byte hash and publish path. Product commit `3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6`. This review is not acceptance of SP05-W01, not a ledger update, and not a production release.

Do not mark AT-FILE-01, AT-FILE-05, SP05-W01, or SP05-W06 from this run. Slot B owns the parser worker. Slot C owns the executor slice. This directory does not authorize edits there.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `SHA256SUMS` (every file here except itself)
- `source-commit.txt` and `source/blobs.txt`
- `pre-edit/console.log`, `pre-edit/docker-run.exit` (0), `pre-edit/phases/worst.exit` (0), `pre-edit/archive-match.txt`
- `oracle/console.log`, `oracle/docker-run.exit` (0), `oracle/phases/worst.exit` (0), `oracle/product-files-match.txt`
- `oracle/notes/quota.txt`, `complete.txt`, `lock.txt`, `scope.txt`, `generic.txt`, `bounded.txt`
- `oracle/notes/memory-8388608.txt` and `oracle/notes/memory-33554432.txt`
- `oracle/pattern-recompute.txt`
- `oracle/attempt1-fmt/console.log` and `oracle/attempt1-fmt/docker-run.exit` (1)
- `contracts/control/local-file-hosting-v1.md` at the product commit
- a183 `FINDINGS.md` F2 and F3, evidence commit `950406b8`
- a186 `REPORT.md`, evidence commit `523951fd`, as the author run only

## What this run measured

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One Rust builder at a time. No PostgreSQL. Private network `oo-a188-eef831aa-net`. Private volumes `oo-a188-eef831aa-cargo` and `oo-a188-eef831aa-target`.

Pre-overlay archive of `3a251dea`: 10 `file_hosting` tests passed, fmt and clippy `-D warnings` clean, lock unchanged. The overlay was absent.

Oracle, after a preserved fmt failure and an in-image rustfmt of `oracle/review_bounds.rs` only: 6 overlay tests passed in 43.73s. Clippy `-D warnings` clean. Lock unchanged. The six product files still matched the fixed commit.

Quota ceiling 10, eight concurrent 8-byte reservations: exactly one success, reserved 8, abort 0. The same configuration stayed one session and stayed at 8. Different part bytes returned `Conflict` and left the stored part. Two completes published `original-bytes` (`8e338be649868e7c9e053212037e4bb0965ba7653e81b974bab76578bd59c80a`); the racing writer returned `Closed`; foreign files stayed. Cancel of a waiting begin returned in 0 ms. The 30-second wait returned `Unavailable` at 30001 ms and wrote nothing. Process exit released the flock. The lock file is beside the canonical directory. `FileHost::new` returned `Unsupported` on an empty store.

Resident high-water delta during `complete`: 143360 bytes on an 8388608-byte part, 139264 bytes on a 33554432-byte part. Both published hashes match a host recomputation of the planted pattern. a183's comparable delta was 16801792 bytes.

## Still open

S3 and MinIO, conditional quota on a generic store, quota across hosts that do not share one deployment directory, `dest#N` leftovers, a crash between owned keys, download and TTL, HTTP and an auth proxy, grant authentication, and a reaper. `read_object` still returns one buffer of the object size. The contract sentence about a poisoned wait is stricter than `lock_mutex`, and this run did not produce a poisoned mutex.

## Cleanup

Done. `cleanup-done.txt` is the command log. The launch traps had already removed containers `oo-a188-pre-edit` and `oo-a188-review`. Network `oo-a188-eef831aa-net`, volumes `oo-a188-eef831aa-cargo` and `oo-a188-eef831aa-target`, and `/tmp/oo-a188-eef831aa-dba4-4af2-a78b-541bc9e0a188` are gone. Other owners' containers, networks, and volumes were left in place. Nothing was pruned.

## Next reader

The limited approval is the local directory only. A later S3 or cross-machine quota change needs its own review. Do not treat `523951fd` as this review's measurement.
