# FINDINGS — a209 local download EOF review

Verdict: **APPROVE LIMITED** for one local deployment directory on `5e3ba29f8d460a1aa6919733100c34a68a2fdb75`. No new component.

Blob `8c32a4feaf69d9a445a5839ba2a01982c994936b` is `crates/oo-control/src/file_hosting.rs`, sha256 `05c9d4d7e2888c40d9de848021f3452968671ae0a3ea04ca58a13f147c49b38b`. The contract blob is `c6c73c1a48f805abed36a5d0965e64287223a5a7`, sha256 `51f8899bcdb29f22919746e50bdd6f93cb5b22f2599e9659b0b32cb116334419`. Author evidence `84c03d01ce35728924100e42ca142c0ef650ac71`. Prior review `ef6c4fc7da03342f900146e90ff692fe742d1ba4` on `e3363ffecc21af4dc9fcf75509978da34b0860b1`.

The independent oracle is `oracle/ran/review_download.rs` (sha256 `7e46e11b87d4755948195d734fcf27bc54f3e8e40f987ea3e4bec4c043634334`), copied onto a `git archive` of the fixed commit and formatted by rustfmt 1.94 before the tests. It is not in the product commit.

## Repair that held

`STREAM_CHUNK_BYTES` is 8192 at line 48. `MAX_MANIFEST_BYTES` is 4096 at line 53. `open_download` (716) rejects a chunk budget of 0 or above 8192 at line 723 with `Limit` before a store read.

`read_download_range` (1315) refuses a chunk limit of 0 or above 8192 at line 1321, then builds `GetRange::Bounded(offset..end)` at lines 1332–1334. `end` is the minimum of `offset + limit` and the snapshot size. It awaits `await_get` (1337) and, only if that returns, `await_bytes` (1338). An empty buffer, a buffer longer than the limit, or a buffer whose length is not the requested range is `Store`.

`await_get` (1392) calls `ensure_running` before `get_opts` (1397) and again after it returns (1401). A failed second check returns `Stopped` and the caller does not start `bytes()`. `await_bytes` (1408) calls `ensure_running` before `bytes()` (1409) and again after it returns (1411). `await_head` (1375) and `stat_pinned` (1424) use the same before-and-after shape. `ensure_running` (1437) checks the latched stop, the cancel flag, and `Instant::now() >= deadline`.

`finish_eof` (1254) reloads the manifest bytes, heads the object, safe-stats the local file when `open_local` set `local_root`, rehashes, then reloads, heads, and stats again. Completion at line 1284 requires the manifest bytes, the head identity, the local stat when a local file exists, the running sha256, and the rehash digest to equal the snapshot. `rehash_stored` (1291) reads `STREAM_CHUNK_BYTES` ranges and drops each vector before the next range. `FileHost::new` has no local file, so its proof is the two digests, the manifest bytes, the length, and etag/version when the store reports them. The local backend version is empty and is not an immutability token.

`Stopped` displays `download stopped before the object was verified` (line 107). The oracle matched that string for the held `get_opts`.

## Prefix flip, measured on LocalFileSystem

Known payload `OOA209-known-payload-v1` (23 bytes), runtime sha256 `e0e224046caa35812b45c51b8a26954aae99bd241927ef719b34af2b1fe9f899` in `oracle/notes/local.txt`.

Clean EOF: returned slices equalled the source, and the running hash, file hash, and manifest digest equalled that digest. `is_complete` was true.

Consumed prefix: first chunk returned, stored byte 0 flipped, further `next_chunk` calls returned the rest of the original bytes to the caller, then `FileHostError::Corrupt`. `is_complete` stayed false. The running hash of the returned chunks still matched the source. The file hash did not. An unread last-byte flip was `Corrupt` and incomplete. The same prefix flip through `FileHost::new(LocalFileSystem)` was `Corrupt`.

This is the a194 counterexample (`consumed_prefix_flip_complete=1` on e336). On this commit it is `Corrupt`.

After complete, flipping byte 0 left `is_complete` true and `next_chunk` returned `Ok(None)`. A new `open_download` of that key was `Corrupt`. Contract lines 123–124. The EOF check is the object at that end.

Truncate to zero after the first chunk returned `Store`. `await_get` line 1404 maps every `get_opts` error to `Store`, so the length check in `finish_eof` does not run. The pull stayed incomplete. The display text had no `/`. a194 recorded the same `Store` result. This approval keeps that limit.

Quota bytes and quota names were unchanged. Mutating calls on `FileHost::new` returned `Unsupported`. `open_download` does not take the deployment lock.

## Ranges and the etag window

`oracle/notes/probe.txt`. Data budget 1000 on 20000 bytes. 23 body ranges, all `GetRange::Bounded`, lengths 1000 twenty times then 8192, 8192, 3616, endpoints `0..8192`, `8192..16384`, `16384..20000`. `body_unbounded=0`. `body_over_8192=0`. Manifest ranges at most 4096. Source sha256 `b711d0b9615686ec1bc4c42d75657874afc0beb005157edd54492882366348e7` matched the running hash, the file hash, and the manifest digest. After complete, a byte flip added zero body gets.

Stable etag `a209-etag-a` completed. Changing only the etag to `a209-etag-b` on the head after the rehash, with the same bytes, was `Corrupt` and incomplete. The running hash and the file hash still matched the source. The author report says that etag-only case was not injected. This review injected it on `FileHost::new`, which has no local stat. Contract lines 121–122.

## Deadline, measured

Held `get_opts` past the deadline: `held_get_dropped_by_deadline=0`, `bytes_started_after_held_get=0`, result `Stopped`, pull incomplete. The next `next_chunk` was `Stopped`. This is the second a194 limit (`bytes_started_after_deadline_during_get_opts=1` on e336). On this commit `bytes()` was not started.

Held `bytes()`: `held_bytes_dropped_by_deadline=0`, result `Stopped`, pull incomplete. The pause sits inside the payload stream, so the host's `bytes()` future was pending past the deadline and was not dropped. After it returned, `ensure_running` stopped the pull.

Blocking `get_opts`: a thread slept 1500 ms. The future was still pending after the 250 ms deadline plus 80 ms, `dropped_block` stayed false, `bytes()` did not start, and the result was `Stopped`. Contract lines 125–126. An already-polled await is not cancelled. An already-expired deadline issued `expired_deadline_body_gets=0` and returned `Stopped`. Cancel after one chunk was `Stopped` and incomplete. `CancelArm` (1459) sets the flag on drop. It does not abort the call already in flight.

## Resident bound

Fresh child, pattern `0x5A`, one chunk retained, parent file hash with an 8192-byte buffer. `oracle/notes/memory-8388608.txt` and `oracle/notes/memory-33554432.txt`.

| | 8388608 | 33554432 |
| --- | --- | --- |
| `download_hwm_delta_bytes` | 65536 | 36864 |
| `download_chunk_max` | 8192 | 8192 |
| `download_pulled` | 8388608 | 33554432 |
| `download_complete` | 1 | 1 |
| sha256 | `7014ae0f2fc0fee42a440b97859207efb72ffee09d4864f7433f1bf756a17aca` | `371036ccdfc733fa30542a26a4f276536147c906d1570f0becc1d6f8b868c311` |

`fixture_object_vec=0`. Both deltas are under 4 MiB and under half the object. The smoke console's 73728-byte deltas are the committed suite's own pattern. They are not this table.

## What stays open

Contract lines 141–157. S3 and MinIO, conditional quota on a generic store, quota across directories, `dest#N`, HTTP, an auth proxy, revocation, TTL, presigned URLs, formula-safe output, retention delete, a reaper, and checking the grant. `read_object` (675) still returns one buffer of the object size. SP05-W01 and SP05-W06 stay open. No parent checkbox and no release claim. This approval does not increment a new-component count.
