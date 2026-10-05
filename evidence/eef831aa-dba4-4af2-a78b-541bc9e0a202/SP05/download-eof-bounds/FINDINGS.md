# FINDINGS — a202 local download EOF and await repair

Repair commit `5e3ba29f8d460a1aa6919733100c34a68a2fdb75` on top of the download in `e3363ffecc21af4dc9fcf75509978da34b0860b1`. Review `ef6c4fc7da03342f900146e90ff692fe742d1ba4` recorded two limits. This run reproduced both on the unmodified blobs, then changed the download path. It is not an approval of the result.

Blob `8c32a4feaf69d9a445a5839ba2a01982c994936b` is `crates/oo-control/src/file_hosting.rs`, sha256 `05c9d4d7e2888c40d9de848021f3452968671ae0a3ea04ca58a13f147c49b38b`. The test blob is `303ae518ed2e133017e71cacada6c9af66419720`, sha256 `ed79247f13c216188047034cecc7b3c20ed3ed940d21f385c97e65daf4e2ec52`. The contract blob is `c6c73c1a48f805abed36a5d0965e64287223a5a7`, sha256 `51f8899bcdb29f22919746e50bdd6f93cb5b22f2599e9659b0b32cb116334419`.

## What the pre-edit run showed

`finish_eof` on e336 hashed the chunks already returned to the caller, reloaded the manifest bytes, and compared `store_len` to the snapshot size. It did not read the object again. Publishing 20 bytes (`abcdefghij0123456789`, sha256 `7ddccfc3e23baf04a142c2c74182139117ecfa82697d7a22404ceb4ac1a8a48b`), pulling the first 8, flipping stored byte 0, and pulling the rest completed. `pre-edit/repro-notes/consumed-prefix.txt` records `consumed_prefix_flip_complete=1`, running hash equal to the snapshot, stored file different.

`read_bounded_range` awaited `get_opts` and then `bytes` with no `ensure_running` between them. The overlay held `get_opts` on a `CountingStore`, slept past a 400 ms deadline, and observed `bytes()` start. `pre-edit/repro-notes/held-get.txt` records `bytes_started_after_deadline_during_get_opts=1` and `deadline_dropped_get_opts=0`. The chunk was then `Stopped`, and `is_complete` stayed false. The held future was not dropped by the deadline.

## End of object

`ObjectDownload::finish_eof` (`file_hosting.rs` 1254) runs when `offset == manifest.size`. It reloads the manifest bytes and requires them to equal the open snapshot. It heads the object and requires that length to equal the snapshot size. On `open_local` it safe-stats the object file. It then rehashes the stored object (`rehash_stored`, 1291) in `GetRange::Bounded` ranges of `STREAM_CHUNK_BYTES` (8192), updates a second `Sha256`, and drops each range before the next. It reloads the manifest, heads the object, and stats the file again.

Completion requires all of the following: manifest bytes still equal the snapshot, the head (`len`, `e_tag`, `version`) is unchanged across the rehash, the local stat is unchanged across the rehash when this reader has a local file, the running digest of bytes returned to the caller equals the snapshot digest, and the rehash digest equals the snapshot digest. Then `complete` is set.

`local_object_file` (1709) rejects an empty segment, `.`, `..`, or NUL, canonicalizes the deployment root and the parent, and requires the parent to stay under that root. `stat_local` (1734) uses `symlink_metadata`. A symlink or a non-file is `Corrupt`. On Unix the identity is device, inode, modified time, and length. The local backend's object `version` is empty. That empty field is stored and compared as `None` to `None`. It is not treated as an immutability token.

`FileHost::new` leaves `local_root` empty, so the download has no local stat. Its proof is the two digests, the manifest bytes, the length, and equality of `e_tag` and `version` across the rehash when the store reports them. A missing etag stays missing. The proof does not write a quota row. `open_download` still does not take the deployment lock.

A change during the rehash window is `Corrupt` even when a hash would otherwise match, because the head and the local stat must be equal on both sides. A same-length flip that lands before that window is caught by the rehash digest. The running hash of the returned chunks can still match the snapshot. Both have to match. The open-time stat is not required to equal the stat at the start of the rehash.

`read_object` is the same function as on e336. It still returns one buffer of the object size. `read_bounded_range` remains the publish and `read_object` helper. The download path uses `read_download_range` (1315), which checks the deadline and cancel around each await.

The contract Download section states the same rule, including the sentence that a write after `is_complete` is already true is a race on the shared file. It does not clear `is_complete`, and a later `next_chunk` returns `Ok(None)` without reading again. That is the clean-EOF check. It is not a later revocation.

## What the green tests measured

`download_eof_rejects_flipped_bytes_and_keeps_a_clean_object` on a real `LocalFileSystem`:

- A clean pull with budget 8 returns the source bytes, the independent `sha2` of those bytes matches the snapshot, the file matches the snapshot, and `is_complete` is true.
- Flipping stored byte 0 after that leaves `is_complete` true. The next `next_chunk` is `Ok(None)`. `manifest()` is still the open snapshot. The file hash differs.
- Flipping stored byte 0 after the first 8-byte chunk, then pulling the rest, returns `Corrupt`. The hash of the bytes already returned still matches the snapshot. The file does not. `is_complete` stays false. `manifest()` is unchanged.
- Flipping the last byte after the first chunk returns `Corrupt` before completion. The file hash differs. `manifest()` is unchanged.

`download_deadline_does_not_start_bytes_after_held_get` publishes with `open_local` and reads through `FileHost::new(CountingStore)`. The store pauses a body `get_opts` of a key ending in `original` before the inner read, and it returns a stream that pauses only when `bytes()` is polled. Head and manifest gets are not paused.

- Deadline `now + 400 ms`. The test polls until `entered_get`, sleeps until 50 ms past the deadline, and sees `dropped_get` false and `entered_bytes` false. After `release_get`, the chunk is `Stopped` and `entered_bytes` is still false. `is_complete` is false. The display is `download stopped before the object was verified`. The next `next_chunk` is `Stopped`.
- The same hold on the end-of-object `get_opts`, after the data chunk has been returned, also ends `Stopped` with `entered_bytes` false and `is_complete` false.
- Dropping `next_chunk` while `get_opts` is held sets `dropped_get` and does not set `done_get`. After release, `done_get` becomes true. The reader stays incomplete. The next `next_chunk` is `Stopped`.

The deadline does not drop the in-flight `get_opts`. A local filesystem read that has already entered the kernel is not preempted by this host, and the code does not claim that it is. `Drop` sets the cancel flag checked on the next `ensure_running`. `await_get` (1392) checks before `get_opts` and again after it returns, and it returns `Stopped` without calling `await_bytes` (1408) when the second check fails.

`download_regressions_deadline_cancel_manifest_truncate_quota`:

- A deadline already in the past: `open_download` succeeds, the first `next_chunk` is `Stopped`, the display is the sentence above, `is_complete` is false.
- `cancel` after one chunk: `Stopped`, incomplete, snapshot unchanged.
- A manifest byte flip at index 20 after open: `Corrupt`, incomplete, `manifest()` still the open snapshot.
- Truncate the object to length 0 after the first chunk: `Store` or `Corrupt`, display contains no `/`, incomplete.
- `FileHost::new(LocalFileSystem)` : `begin`, `put_part`, `complete`, and `abort` are `Unsupported`. A clean pull completes, and the independent hash of the returned bytes matches the file. A consumed-prefix flip on that same host is `Corrupt`. The original session quota file bytes are unchanged, and the quota directory names are unchanged.

## Resident set

See `REPORT.md`. The 8 MiB and 32 MiB pulls completed with high-water deltas 77824 and 73728 bytes, chunk max 8192, `VmPeak` unchanged across the pull, and `fixture_object_vec=0`. The existing assertions `delta < 4 MiB` and `delta * 2 < size` still pass. The test does not build a vector of the object.

## What this run does not claim

No S3 or MinIO behavior. No HTTP download, auth proxy, grant authentication, revocation, TTL, presigned URL, formula-safe output, retention delete, or reaper. No change to the publish-path memory ceiling. No claim that a stable etag across a window proves the bytes were immutable before the window. No claim that `is_complete` means the file is immutable afterward. The fixed review of this commit is the next task. This author run does not mark a parent checkbox.
