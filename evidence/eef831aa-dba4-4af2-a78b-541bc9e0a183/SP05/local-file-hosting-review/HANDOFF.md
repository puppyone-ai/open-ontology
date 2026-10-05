# HANDOFF — a183 fixed local file hosting review

**Verdict: APPROVE_LIMITED** of the local `FileHost` primitive in `755fb84acc7f80e5ba2fba1c2b80412dab7fab26`. The spec and plan quota guarantee, streaming large objects, S3, HTTP, authentication, the reaper, and REQ-SRC-05 after `stored` stay open. No product change. No ledger checkbox. No release claim.

## Read first

- `REPORT.md` and `FINDINGS.md` in this directory
- `SHA256SUMS` (every file here except itself)
- `author-sha256sums-gap.txt` for the a181 six-file inventory gap
- `source/blobs.txt` for the six source blobs
- `control/console.log` and `control/phases/worst.exit` (0) for the pre-overlay Linux 1.94 smoke
- `oracle/console.log`, `oracle/oracle-notes.txt`, `oracle/memory-notes.txt`, `oracle/phases/worst.exit` (0), `oracle/docker-run.exit` (0)
- `oracle/host-post-exit.txt` for the host parser failure after that green container
- `contracts/control/local-file-hosting-v1.md` at the fixed commit
- a181 `impl/attempt1` through `impl/attempt6`, left in place

## What was proved on this run

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One builder. No PostgreSQL. Private network `oo-a183-eef831aa-net`. Private volumes `oo-a183-eef831aa-cargo` and `oo-a183-eef831aa-target`.

Author tests on the unmodified archive: 6 passed. Independent oracle: `review_memory` 1 passed, `review_oracle` 6 passed, clippy `-D warnings` clean, lock unchanged, six product blobs unchanged.

Quota sample that keeps the guarantee outside the approval: concurrent `begin` pair, `oks=2`, `reserved=16`, `limit=10`. Memory sample: direct store stream chunk 8192, `complete` VmHWM delta 16801792 bytes on an 8388608-byte part.

## Cleanup

Own containers `oo-a183-review-control` and `oo-a183-review-oracle` are removed by the launch traps. After the evidence commits, remove only `oo-a183-eef831aa-net`, `oo-a183-eef831aa-cargo`, `oo-a183-eef831aa-target`, and `/tmp/oo-a183-eef831aa-dba4-4af2-a78b-541bc9e0a183`. Leave every other owner's containers, networks, and volumes alone. Do not prune.

## Next reader

Treat `APPROVE_LIMITED` as the local contract behavior checked above. A later S3, quota, or streaming change needs its own run. Do not cite the a181 `SHA256SUMS` as a full evidence inventory. Do not mark SP05-W01, SP05-W06, AT-FILE-01, or AT-FILE-05 done from this review.
