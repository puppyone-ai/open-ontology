# FINDINGS — a186 local upload streaming and quota

Line numbers below are the working tree that the green smoke compiled. `source-commit.txt` names the commit that contains those bytes. a183 findings F2 and F3 are the measured gaps this run closes on the local store. a181 `SHA256SUMS` stays the historical six-file inventory.

## F2 — Same-directory quota is one critical section

a183 ran one concurrent pair with `HostLimits::try_new(16, 16, 4, 10)`. Both `begin` calls returned `Ok`. `reserved=16` and `limit=10`. `reserve_quota` summed the quota keys and returned before the reservation write.

`begin`, `put_part`, `complete`, and `abort` now take `lock_deployment` for the whole call (`file_hosting.rs:415`, `file_hosting.rs:490`, `file_hosting.rs:568`, `file_hosting.rs:632`). `reserve_quota` still only computes the sum. The reservation `put_create` stays later in `begin`, and the same guard is held across both. Cleanup helpers do not take the lock again.

`open_local` creates `{dirname}.oo-file-hosting.lock` beside the deployment directory (`file_hosting.rs:390`). The caller does not choose that path. The file is outside the store prefix, so admission tests that expect an empty object list still see an empty list. `acquire_lock` (`file_hosting.rs:1178`) takes an in-process gate and then `std::fs::File::try_lock`. The gate orders tasks in one process. The file lock orders other processes. The helper thread retries for `LOCK_WAIT` of 30 seconds (`file_hosting.rs:48`) and the async caller waits with `poll_fn`. Timeout, a lock-file error, or a non-`WouldBlock` `TryLockError` returns `Unavailable` and writes nothing. Drop unlocks the file and then clears the gate.

`FileHost::new` stores no lock path (`file_hosting.rs:377`). `lock_deployment` returns `Unsupported` before any store call (`file_hosting.rs:950`). Display text is `object store cannot enforce a linearizable project quota`. There is no unlocked sum. A later S3 backend needs its own conditional quota. This file lock is local to one directory.

`concurrent_quota_two_hosts_and_child` passed on the green run. It uses `HostLimits::try_new(64, 64, 4, 10)` and 8-byte objects. Four in-process rounds use a ready/go file barrier and require exactly one `Ok`, the other `Quota`, `reserved=8`, then abort back to 0. A later round uses two hosts plus a re-executed child process on distinct names and requires the same single success and `reserved=8`. The lock file exists beside the store and no object key contains `oo-file-hosting`. After abort, a new `begin` succeeds. A same-config round with both hosts and the child requires one shared session and `reserved=8`. Abort, a retry with a new session, and a second abort of `NotFound` leave `reserved=0`.

`concurrent_complete_and_part_keep_one_object` passed. Two `complete` calls and a child `put_part` of different bytes share one barrier. The two manifests are equal. The child result is `Conflict` or `Closed`. The object bytes stay the original part. `staging/<session>/part/00001` is gone. `staging/<session>/part/notes`, `meta/quota/<project>/notes`, and a file outside the store remain.

The six a183 tests passed again, including checksum and size failure with no visible object, abort that leaves foreign bytes, and idempotent reopen.

The lock is advisory. A raw store client can still write keys. Two machines that do not share the directory do not share the lock. A crash can still leave keys between the quota record and the pointer; that window stays with the reaper.

## F3 — Part bytes are read and published in 8192-byte ranges

a183 `complete` called `get_bytes`, which calls `GetResult::bytes` and then `to_vec` on the whole part. On an 8388608-byte part the resident high-water delta was 16801792 bytes.

`STREAM_CHUNK_BYTES` is 8192 and is fixed before each range is built (`file_hosting.rs:46`). `read_bounded_range` (`file_hosting.rs:1242`) sets `GetRange::Bounded` to at most that many bytes, then rejects an empty buffer, a buffer longer than 8192, or a buffer whose length is not the requested range, before `to_vec`. `for_each_chunk` (`file_hosting.rs:891`) and `write_chunks` (`file_hosting.rs:915`) walk the object with that helper. `hash_parts` (`file_hosting.rs:826`) uses it. `publish_parts` (`file_hosting.rs:851`) calls `put_multipart_opts` only after that hash already matched, and writes the same ranges into the multipart handle. The local store's multipart handle appends each chunk to its `dest#N` file. `complete` re-hashes the published key with the same ranged reader before it writes the manifest. A checksum or size miss returns before `publish_parts`, so the object key is not created. A republish mismatch deletes that object key and does not write the manifest.

`put_part` still receives the caller's slice, and `read_object` (`file_hosting.rs:657`) still returns one `Vec` of the stored size. Those two buffers are the caller's. The 8192-byte bound is the host's internal read and multipart write.

`streaming_complete_memory_is_bounded` passed for 8388608 and 33554432 bytes. Resident deltas were 114688 and 122880 bytes. Virtual-peak delta was 2203648 bytes on both, with thread count 3 before and after `complete`. The part file was removed and the published object hash matched. `post-edit/attempt2/metrics/memory-8388608.txt` is the earlier 8 MiB sample whose virtual peak rose by 136380416 bytes while the resident delta was 143360 bytes. That sample is kept. The green resident numbers are the whole-part signal, and they do not scale from 8 MiB to 32 MiB.

`get_bytes` remains for session JSON, the quota decimal, and the idempotency pointer.

## What this run leaves in place

`generic_store_refuses_before_any_write` passed. An `InMemory` host returns `Unsupported` from `begin`, `put_part`, `complete`, and `abort`, and `list` stays empty. The display string has no `/` and no `memory`.

Grant evidence is still stored and still unchecked. `FileHostError` display strings are still fixed sentences. Debug output still omits the store. The a181 `SHA256SUMS` gap recorded by a183 is unchanged.
