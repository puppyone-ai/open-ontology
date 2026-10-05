# HANDOFF — a194 local streaming download review

Verdict: **APPROVE LIMITED**. One local deployment directory passed `open_download` / `next_chunk` on commit `e3363ffecc21af4dc9fcf75509978da34b0860b1`, tree `fb3f57bfcb2853e56a617549be340043fe3cd7fb`, parent `33426a733d8f569a74e8074745a2a77528b1cb53`. Author evidence `a699f8da3dfdae918eb1129509d074c7d1d3bdce`. This review is not acceptance of SP05-W01, not a ledger update, and not a production release.

Do not mark AT-FILE-01, AT-FILE-05, SP05-W01, or SP05-W06 from this run. Other slots own the parser worker, the policy catalog, CSV, and the executor review. This directory does not authorize edits there.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `SHA256SUMS` (every file here except itself)
- `source-commit.txt` and `source/blobs.txt`
- `source/blobs/` (the three product files plus unchanged `lib.rs` and `crates/oo-control/Cargo.toml`; `Cargo.lock` is hash-only)
- `pre-edit/console.log`, `pre-edit/docker-run.exit` (0), `pre-edit/phases/worst.exit` (0), `pre-edit/archive-after.txt`
- `oracle-attempt-4/console.log`, `oracle-attempt-4/docker-run.exit` (0), `oracle-attempt-4/phases/worst.exit` (0), `oracle-attempt-4/archive-after.txt`
- `oracle-attempt-4/notes/exact.txt`, `generic.txt`, `mutations.txt`, `pending.txt`, `memory.txt`, `pattern-recompute.txt`
- `oracle/console.log` and `oracle/docker-run.exit` (101), `oracle-attempt-2/console.log` (101), `oracle-attempt-3/console.log` (101)
- `contracts/control/local-file-hosting-v1.md` at the product commit
- a193 `HANDOFF.md` as the author run
- a188 `HANDOFF.md`, evidence `b6cddea1`, as the quota and 8192 publish approval only

## What this run measured

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One Rust builder at a time. No PostgreSQL. Private network `oo-a194-eef831aa-net`. Private volumes `oo-a194-eef831aa-cargo` and `oo-a194-eef831aa-target`.

Pre-overlay archive of `e3363ffe`: 13 `file_hosting` tests passed in 19.24s, including `concurrent_quota_two_hosts_and_child`. fmt and clippy `-D warnings` clean. Lock unchanged. The overlay was absent. That quota test was not repeated.

Attempt 4, after preserved compile, assertion, and clippy failures: rustfmt of `oracle/review_download.rs` only, sha256 `b4b0d18a317d019026143be4984c2cbd706de77539cbbb5235b6e3caf508827c`. Five overlay tests passed in 21.11s. Clippy `-D warnings` clean. Lock unchanged. The six product paths still matched the fixed commit.

Resident high-water delta during the pull, after `complete`, caller holding one chunk, pattern `0x3C`: 73728 bytes on 8388608, 77824 bytes on 33554432. Range counts 1024 and 4096, each range at most 8192, sums equal to the object sizes, `body_unbounded=0`. Parent `sha2` matched the child digests. A deadline during a held `get_opts` did not abort it; `bytes()` still started; the chunk was then `Stopped` and `is_complete` stayed false. A same-length flip of an already pulled prefix still completed, because EOF hashes pulled chunks and checks length and manifest bytes. An unread flip was `Corrupt`.

## Still open

S3 and MinIO, conditional quota on a generic store, quota across hosts that do not share one deployment directory, `dest#N` leftovers, HTTP, an auth proxy, grant authentication, revocation, TTL, presigned URLs, formula-safe output, retention delete, and a reaper. `read_object` still returns one buffer of the object size. The deadline check is around `read_bounded_range` as a whole (`file_hosting.rs` 1216 and 1225), not between `get_opts` (1546) and `bytes` (1550).

## Cleanup

Done. `cleanup-done.txt` is the command log. `docker rm -f` of `oo-a194-pre-edit`, `oo-a194-review`, `oo-a194-review2`, `oo-a194-review3`, and `oo-a194-review4` printed `No such container`; the launch traps had already removed them. The same log removed network `oo-a194-eef831aa-net`, volumes `oo-a194-eef831aa-cargo` and `oo-a194-eef831aa-target`, and `/tmp/oo-a194-eef831aa-dba4-4af2-a78b-541bc9e0a194`. `oo-a196-red` and `oo-a196-pg` were still present afterward. Nothing was pruned.

## Next reader

The local pull on one deployment directory is approved with the limits in `FINDINGS.md`. Do not treat a188's measurement as this run's download proof. A later HTTP, TTL, revocation, or S3 change needs its own review.
