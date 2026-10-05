# FINDINGS — a194 local streaming download review

Verdict: **APPROVE LIMITED** for one local deployment directory on `e3363ffecc21af4dc9fcf75509978da34b0860b1`.

Blob `1af90356ae515da382d4dc23c783fa3f42d1c93b` is `crates/oo-control/src/file_hosting.rs`, sha256 `a8ac952d3d4842bb7b357b7201b76a19c7c940b770042bcaaab86f2b32ab33aa`. The contract blob is `3418685b2a009d75eea7760788d48b1655d30d3e`, sha256 `e5e4315399ee8367580aa567dca6b34bf3f809a42110730fca56a4e40e7d29bd`. Author evidence `a699f8da`. a188 (`b6cddea1`, product `3a251dea`) approved the quota section and the 8192 publish path. This run is the download measurement.

## Pull behavior that held

`open_download` (`file_hosting.rs:706`) rejects a chunk budget of 0 or above 8192 at line 713 with `Limit` before a store read. The overlay saw zero body `get_opts` for both. It loads the manifest through `load_capped_bytes` (bounded range, cap 4096), checks the key and session, then `authorize_manifest` (1692), which compares project and principal. A decoded size of 0 is `Corrupt` at 732. A stored length that is not the manifest size is `Corrupt` at 738. The method does not hash the object and does not take the deployment lock.

`read_object` (667) still `try_reserve`s the object size and returns one buffer. The overlay called it on a 20-byte object and got that buffer (`read_object_returns_full_buffer=1`). The 8 MiB and 32 MiB pulls did not call it. The compatibility buffer stays open.

`next_chunk` (1194) arms `CancelArm`. `pull_chunk` (1204) returns `Ok(None)` immediately when `complete` is already set, so cancel after a verified end does not clear it. `ensure_running` (1274) runs before the read (1216) and after `read_bounded_range` returns (1225). `read_bounded_range` (1527) builds `GetRange::Bounded(offset..end)` at 1543. `end` is the minimum of `offset + budget` and the snapshot size. An empty buffer, a buffer longer than the budget, or a short buffer is `Store`. The host does not keep the returned `Vec`. The hasher is updated from that chunk only (1239). A returned chunk leaves `complete` false. The overlay's widest body range was 8192 bytes. `body_unbounded` stayed 0 on every test, including both large samples (`range_count` 1024 and 4096).

`finish_eof` (1244) runs only when `offset == manifest.size`. It reloads the manifest bytes, requires those bytes to equal the snapshot, requires `store_len` to equal the snapshot size, and requires `finalize` of the running hasher to equal the snapshot digest. Only then does it set `complete`. The second `next_chunk` after that returned `Ok(None)` with no extra body get.

`FileHost::new` (384) has no deployment lock. `begin`, `put_part`, `complete`, and `abort` on that host returned `Unsupported` and left the quota file bytes and the quota names unchanged. `open_download` on that same host then pulled the object that `open_local` had published.

`Stopped` displays `download stopped before the object was verified` (line 107). The overlay matched that string for cancel, a deadline already past, a deadline between chunks, and the held-`get_opts` case.

## Deadline and cancel, measured

`read_bounded_range` awaits `get_opts` at 1546 and `bytes` at 1550. The host does not call `ensure_running` between those two awaits. The overlay held `get_opts` on a `CountingStore`, slept past a 400 ms deadline, and observed:

- the held future was not dropped
- `bytes()` started after the deadline had passed
- the chunk was then `FileHostError::Stopped`
- `is_complete` stayed false
- the open snapshot was unchanged
- the following `next_chunk` was `Stopped` and did not issue another body get

Dropping the `next_chunk` future set the cancel flag (`CancelArm` drop, 1301) and did not make the held wait return early. After the wait was released, the next `next_chunk` was `Stopped`.

Contract line 121 says the deadline is checked before each read and after each await, and that `Drop` and cancel do not cancel a local filesystem read that has already started. The code checks around `read_bounded_range` as a whole. The non-preemption sentence matches the run. A check between `get_opts` and `bytes` was not present, and this approval does not claim one. No chunk was accepted after the deadline, and completion was not set. That is the limit, not a failed predicate.

## Identity at clean EOF

Contract lines 119–123 say `Ok(None)` and `is_complete` require every byte pulled, the stored manifest bytes unchanged, the stored length unchanged, and the running sha256 equal to the snapshot. The overlay's clean pulls met that and kept the snapshot fields stable, including after a different grant string with the same project and principal.

Counterexample for the broader sentence "a changed object hash is Corrupt": publish the same 20 bytes (`exact.txt` digest `7ddccfc3e23baf04a142c2c74182139117ecfa82697d7a22404ceb4ac1a8a48b`), pull the first 8, flip stored byte 0, then pull the rest. `is_complete` became true. `mutations.txt` records `consumed_prefix_flip_complete=1`, `consumed_prefix_running_hash_matches_snapshot=1`, and `consumed_prefix_stored_file_matches_snapshot=0`. EOF hashed the chunks the caller already held. It did not read the object again. An unread last-byte flip returned `Corrupt` and stayed incomplete. An append of one byte after the last data chunk and before EOF returned `Corrupt` because `store_len` was no longer the snapshot size. A manifest byte flip returned `Corrupt` and `manifest()` still returned the open snapshot.

Truncate to zero after the first chunk returned `Store` (not `Corrupt`). The pull stayed incomplete. The display string had no `/`.

Forged `object_key` with a re-encoded lineage: `NotFound`, object bytes unchanged. Size 0 re-encoded: `Corrupt`. Empty manifest: `Corrupt`. 4097-byte manifest: `Corrupt`. Deleted manifest: `NotFound`.

## Resident bound

Fresh child process per size, pattern `0x3C`, parts written in 8192-byte slices, `complete` first, then one chunk held by the caller. Parent recomputation is `oracle-attempt-4/notes/pattern-recompute.txt`.

| | 8388608 | 33554432 |
|---|---|---|
| `hwm_delta_bytes` | 73728 | 77824 |
| `range_max` / `chunk_max` | 8192 | 8192 |
| `range_count` | 1024 | 4096 |
| `range_sum` / pulled | 8388608 | 33554432 |
| sha256 | `7837430d6dfb147fe6e0455f954f6ecd460c0fa45bebbab7cfc40d383ecb364c` | `963593ae1962f4b89712cf618f2e31f9cebe6b1eed84eb7872fcfda617859e78` |

`body_unbounded=0`, `second_eof_extra_gets=0`, `complete=1`, quota unchanged. These are not the author's `0xA5` digests. The author's figures stay in a193. This run did not reuse them as proof.

## What stays open

Contract lines 137–152. S3 and MinIO, conditional quota on a generic store, quota across directories, `dest#N`, HTTP, an auth proxy, revocation, TTL, presigned URLs, formula-safe output, retention delete, a reaper, and checking the grant. `read_object`'s object-sized buffer. SP05-W01 and SP05-W06 stay open. No parent checkbox and no release claim.
