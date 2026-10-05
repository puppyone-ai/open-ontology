# HANDOFF — a186 local upload streaming and quota

Local F2 and F3 are implemented on `LocalFileSystem` for one deployment directory. The next step is a fixed independent review of the product commit in `source-commit.txt`. This author run is not that review and not acceptance.

SP05-W01 stays open. The ledger is unchanged. Do not mark AT-FILE-01, AT-FILE-05, SP05-W01, or SP05-W06 from this run. Do not claim a customer deployment or a production release.

## Read first

- `REPORT.md` and `FINDINGS.md` in this directory
- `SHA256SUMS` (every file here except itself)
- `source-commit.txt` and `source/blobs.txt`
- `pre-edit/console.log` and `pre-edit/phases/worst.exit` (0)
- `post-edit/console.log`, `post-edit/phases/worst.exit` (0), `post-edit/docker-run.exit` (0)
- `post-edit/metrics/memory-8388608.txt` and `post-edit/metrics/memory-33554432.txt`
- `post-edit/attempt1/console.log` and `post-edit/attempt2/metrics/memory-8388608.txt`
- `contracts/control/local-file-hosting-v1.md` at the product commit
- a183 `FINDINGS.md` F2 and F3, evidence commit `950406b8`
- a181 `SHA256SUMS`, left as the six-file historical inventory

## What this run measured

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One builder. No PostgreSQL. Private network `oo-a186-eef831aa-net`. Private volumes `oo-a186-eef831aa-cargo` and `oo-a186-eef831aa-target`.

Pre-edit archive: 6 file-hosting tests passed, clippy `-D warnings` clean, lock unchanged.

Green overlay: the same 6 tests plus `generic_store_refuses_before_any_write`, `concurrent_quota_two_hosts_and_child`, `concurrent_complete_and_part_keep_one_object`, and `streaming_complete_memory_is_bounded`. 10 passed. Clippy `-D warnings` clean. Lock unchanged.

Resident high-water delta during `complete`: 114688 bytes on an 8388608-byte part, 122880 bytes on a 33554432-byte part. a183's comparable delta was 16801792 bytes.

## Cleanup

Containers `oo-a186-pre-edit` and `oo-a186-post-edit` are removed by the launch traps. After the evidence commit, remove only network `oo-a186-eef831aa-net`, volumes `oo-a186-eef831aa-cargo` and `oo-a186-eef831aa-target`, and `/tmp/oo-a186-eef831aa-dba4-4af2-a78b-541bc9e0a186`. Leave `oo-a184-pg` and every other owner's containers, networks, and volumes alone. Do not prune.

## Next reader

Review the product commit as an independent pass. Check the deployment lock, the `Unsupported` refusal, the 8192-byte publish path, and the two memory samples. Quota across machines that do not share the directory, S3 conditional quota, HTTP, authentication, and the reaper are still open.
