# FINDINGS — a193 local object download pull

Product commit `e3363ffecc21af4dc9fcf75509978da34b0860b1`. Blob `1af90356ae515da382d4dc23c783fa3f42d1c93b` is `crates/oo-control/src/file_hosting.rs`. The contract is `contracts/control/local-file-hosting-v1.md`, sha256 `e5e4315399ee8367580aa567dca6b34bf3f809a42110730fca56a4e40e7d29bd`.

Baseline `15d23fe9f2cbc74e3f3278d70d0f56390568126e` already contained the a186 repair `3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6`, which a188 approved for one local deployment directory (`b6cddea1`). That approval left `read_object` as one object-sized buffer and left download open. This commit adds the pull. It does not close SP05-W01 or SP05-W06, and it does not change the ledger.

## What the pull does

`open_download` (`file_hosting.rs:706`) takes the existing `AdmissionContext`, an `ObjectKey` this host already issues, a chunk budget, and an optional `Instant`. A budget of 0 or above 8192 returns `Limit` before any store call. The method loads the published manifest with `GetRange::Bounded` capped at 4096 bytes (`load_capped_bytes`, range at line 1501), checks the object key and session, then compares project and principal (`authorize_manifest`). A decoded size of 0, a missing object, or a stored length that is not the manifest size is `Corrupt`. A foreign project or principal is `Denied`.

`read_object` (`667`) is unchanged: it still reserves one `Vec` of the object size. The contract says that buffer remains the caller's.

`next_chunk` (`1194`) arms a drop flag, then `pull_chunk` checks cancel and deadline, issues `read_bounded_range` (`1527`), and checks cancel and deadline again after that await. `read_bounded_range` builds `GetRange::Bounded(offset..end)` at line 1543 with `end` at most one budget past `offset` and at most the manifest size. An empty buffer, a buffer longer than the budget, or a short buffer is `Store`. The returned `Vec` is at most the budget. The reader does not keep it. There is no `read_to_end` and no object-sized `to_vec` on this path. `store_bytes` still materializes small session, quota, and idempotency bodies for the existing calls.

`finish_eof` (`1244`) runs only when the offset equals the snapshot size. It reloads the manifest bytes, re-reads the object length, and compares `sha2` over the chunks with the snapshot digest. `Ok(None)` and `is_complete` happen only after those three checks. A chunk return does not set `complete`. `cancel` (`1185`), a passed deadline, and `Drop` of an in-flight `next_chunk` set an `AtomicBool` that `ensure_running` (`1274`) turns into `Stopped` (`download stopped before the object was verified`). `Drop` of the reader sets the same flag and drops the store handle and hash state. It does not cancel a local filesystem read that has already entered `get_opts`, and there is no `Read` implementation and no download credential.

The manifest value from `manifest()` is the open snapshot. A later edit of the stored manifest fails the byte compare and leaves `is_complete` false. The snapshot fields stay the ones from open.

`open_download` does not take the deployment lock and does not change quota. `FileHost::new` can pull bytes that are already stored. Mutating calls on that host are still `Unsupported`.

## What this run measured

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One builder. No PostgreSQL. Private network `oo-a193-eef831aa-net`. Private volumes `oo-a193-eef831aa-cargo` and `oo-a193-eef831aa-target`.

Pre-edit archive of `15d23fe9`: 10 `file_hosting` tests passed, fmt and clippy `-D warnings` clean, lock unchanged.

Post-edit overlay: 13 `file_hosting` tests passed in 19.84s, including `concurrent_quota_two_hosts_and_child`, `concurrent_complete_and_part_keep_one_object`, and `streaming_complete_memory_is_bounded`. Clippy `-D warnings` clean. Lock unchanged.

Resident high-water delta during the pull, after `complete`, caller holding one chunk:

| Sample | 8388608 | 33554432 |
|---|---|---|
| `download_hwm_delta_bytes` | 8192 | 4096 |
| pulled bytes | 8388608 | 33554432 |
| chunk max | 8192 | 8192 |
| sha256 | `e474a45740e5efac02e9f7e0626b3f86111b293f8363e477786dfad64e98df3b` | `edeadec8f638055689d5be63b4bcf2654fb64bf91fb6651e9a924f052a9c7db0` |

The pattern digest, the pull digest, and the file digest match inside the child. The parent hashed the pattern and the published file again with `sha2` and required the same strings. Both deltas are under 4 MiB. The 32 MiB object did not raise the resident delta above the 8 MiB sample.

`download_pull_verifies_identity_without_an_object_buffer` and `download_pull_refuses_other_context_and_altered_bytes` passed. Those cover the budget, the stable manifest, `read_object` on the small object, early drop, cancel, both deadline cases, a one-poll drop, cross-context `Denied`, a tampered object, a zero-length file, a truncate after the first chunk, an altered manifest, and a forged size-0 manifest. The test accepts `Store` or `Corrupt` for the mid-read truncate. The log does not print which one fired. Both leave the pull incomplete, and both display strings are static.

## Poisoned wait

`lock_mutex` (`1349-1350`) is `mutex.lock().unwrap_or_else(|poison| poison.into_inner())`. A poisoned mutex is recovered and the wait continues. It is not `Unavailable`. The previous contract sentence said a poisoned wait returns `Unavailable`. This revision replaces that sentence and marks the delta in the contract. This run did not panic a holder of `wait.slot` or `wait.waker`. There is no new production fault test for poison.

## What this commit does not include

HTTP, an auth proxy, checking the grant, revocation, TTL, a reaper, presigned URLs, formula-safe CSV or Excel, S3, MinIO, and quota across directories that do not share the local lock. `read_object` still allocates the object. A finished pull is not revoked if the object changes later; a new `open_download` sees the new bytes. SP05-W06 is not closed. No parent checkbox and no release claim follows from these tests.
