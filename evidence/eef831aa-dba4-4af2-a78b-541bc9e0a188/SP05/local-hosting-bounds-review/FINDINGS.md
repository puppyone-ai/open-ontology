# FINDINGS — a188 local hosting bounds review

Verdict: **APPROVE LIMITED** for one local deployment directory.

Line numbers are product commit `3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6`, blob `d5fb6ac06c022edceb820e7b153d7095722a9f59` for `crates/oo-control/src/file_hosting.rs`. The contract is `contracts/control/local-file-hosting-v1.md` at that commit, sha256 `ba9db89fbb8515e4650560bf3e133e4c724fc40334b41593890a402a9510b496`. a183 findings F2 and F3 are the gaps this commit was written to close on `LocalFileSystem`. This review measured that claim. It did not reuse the author's resident-set samples.

## F2 — Same-directory quota held at 8 bytes

a183 F2, on the earlier source, ran two concurrent 8-byte begins against a 10-byte project ceiling and observed `reserved=16`. `reserve_quota` summed the quota keys and returned before `put_create` wrote the reservation.

At this commit, `begin` (`file_hosting.rs:415-420`), `put_part` (`490-497`), `complete` (`568-573`), and `abort` (`632-637`) each take `lock_deployment` before they read or write. `reserve_quota` (`687`) runs only after that guard is held (`433`). `FileHost::new` (`377-381`) stores `deployment_lock: None`. `lock_deployment` (`950-953`) returns `Unsupported` before any store call. `open_local` (`390-407`) creates the lock file beside the canonical deployment directory and drops the descriptor. `deployment_lock_path` (`1150-1158`) is `parent.join(file_name + ".oo-file-hosting.lock")`.

Contract lines 85–87 require that check and the reservation write to be one critical section for every `FileHost` on that root, including other processes, with a 30-second wait and `Unavailable` when the lock is not acquired.

Fresh run, `HostLimits::try_new(64, 64, 8, 10)`, eight concurrent begins of `12345678` behind one barrier (four handles, four processes). `oracle/notes/quota.txt`:

- `distinct_ok=1`
- `distinct_quota=7`
- `distinct_other=0`
- `distinct_reserved=8`
- one quota body, `8`
- `after_distinct_abort=0`

The same configuration from the same eight participants returned one session and left the reservation at 8. A begin with a different grant string returned that session. The stored JSON kept `grant-one`. Abort, begin, abort, and a second abort cycled the reservation 8 then 0. The second abort was `NotFound`.

On a 40-byte ceiling, two different sessions reserved 16. `put_part` of different bytes returned `Conflict` and the stored part stayed `aaaaaaaa`. Abort of one session left the other part, the non-hex notes file, and a reservation of 8.

That is the successful path for the a183 counterexample. Sixteen reserved bytes against a ten-byte ceiling did not recur.

The same run also showed the lock's lifetime and its path:

- Cancelling a waiting `begin` returned in 0 ms (`oracle/notes/lock.txt`, `cancel_abort_ms=0`). The next begin took 9 ms. `acquire_lock` checks `wait.cancel` in both loops (`file_hosting.rs:1186` and `1215`). `CancelOnDrop` (`1096-1110`) sets that flag.
- A holder that kept the flock produced `Unavailable` at 30001 ms and an empty store directory. `LOCK_WAIT` is 30 seconds (`file_hosting.rs:48`).
- `SIGKILL` of that holder process let the waiting begin finish in 6 ms. The reservation was 8, abort returned it to 0, and another display reserved 8 again. The flock did not remain as a permanent gate.
- A symlink and a `../` path shared the canonical lock. Four begins reserved 8 once. A decoy lock file inside the store was untouched. A sibling directory with the same basename reserved its own 8 while the first store stayed at 8 (`oracle/notes/scope.txt`).

`FileHost::new` on an empty `InMemory` returned `Unsupported` for begin, put, complete, and abort. The display text matched contract line 89. The store's delimiter listing stayed empty (`oracle/notes/generic.txt`).

## F3 — Complete's resident set did not grow with the part

a183 F3 measured `memory_hwm_delta_bytes=16801792` while completing one 8388608-byte part. `get_bytes` called `GetResult::bytes` and `to_vec` on the whole part, and hash plus publish each did that.

At this commit, `STREAM_CHUNK_BYTES` is 8192 (`file_hosting.rs:46`). `hash_parts` (`826-848`) and `write_chunks` (`915-938`) call `for_each_chunk` (`891-912`). Each iteration calls `read_bounded_range` (`1242-1270`), which sets `GetRange::Bounded(offset..end)` with `end` at most 8192 bytes past `offset`, then rejects an empty buffer, a buffer longer than 8192, or a buffer whose length is not `end - offset`, before `to_vec`. `publish_parts` (`851-874`) starts `put_multipart_opts` only after `hash_parts` has already matched the expected digest and size (`complete`, `594-603`).

`get_bytes` (`1032-1041`) is still a full read. Session JSON, the quota decimal, and the idempotency pointer use it. Those bodies are small. `read_object` (`657-684`) still `try_reserve`s the object size and extends one `Vec`. Contract lines 65–67 assign that buffer to the caller. This review does not approve a streaming download. `docs/17-rust-implementation.md` line 77 still requires streaming large-object download, abort of multipart leftovers, and a reaper. Those stay open with contract lines 109–123.

Fresh samples, one child per size, `/proc/self/status` around `complete` only. The part was planted with 8192-byte writes so the child did not hold it. The parent hashed the published file itself.

| Sample | 8388608 | 33554432 |
|---|---|---|
| `memory_hwm_delta_bytes` | 143360 | 139264 |
| `memory_vmpeak_delta_bytes` | 2203648 | 2203648 |
| threads after | 4 | 4 |
| published size | 8388608 | 33554432 |
| sha256 | `5598dbee2a3ccc9b62206ba34ad81a2425283322e91b8af22faed21486e7053e` | `54a2813fff554842b91ce8dc4285d36e1c3ef92778983b73663f093ca48a205d` |

Both resident deltas are under 2 MiB. The larger part did not raise the resident delta. Host `hashlib` recomputed both digests from the same byte pattern and matched the notes (`oracle/pattern-recompute.txt`). Virtual peak moved by the same 2203648 bytes for both sizes and was not a failure gate.

Empty `put_part` returned `Limit` and left no part file. Completing 8192 bytes and 8193 bytes published those exact lengths, so a one-byte final range was stored and not rejected as a short read. A store that returned an oversized or short range is not reachable through `open_local`, which builds `LocalFileSystem`, or through `FileHost::new`, which returns `Unsupported` before `get_opts`. The rejection is the source check at `file_hosting.rs:1263-1268`. This run did not add a product seam to inject a hostile backend.

Two processes completed `original-bytes` to the same manifest. The simultaneous `put_part` of a same-length replacement returned `Closed`. The object file hashed to `8e338be649868e7c9e053212037e4bb0965ba7653e81b974bab76578bd59c80a`, which is `sha256(b"original-bytes")`. A second complete left the manifest bytes unchanged. The notes file under the part prefix, the non-hex quota name, and a file outside the store were unchanged (`oracle/notes/complete.txt`). Contract lines 81 and 91 describe that publication and that cleanup.

## What this approval does not include

The sibling-directory sample shows two directories do not share a quota. Contract line 87 already says two machines that do not share the directory do not share the lock. That case stays open. So do S3, MinIO, conditional quota on a generic store, HTTP, grant authentication, `dest#N` leftovers, a crash between the quota key and the idempotency pointer, and the reaper.

Contract line 87 also says a poisoned wait returns `Unavailable`. `lock_mutex` (`file_hosting.rs:1113-1114`) recovers with `into_inner` and continues. This review did not panic a holder of `wait.slot` or `wait.waker`, so there is no measured write and no measured non-`Unavailable` result from a poisoned wait. That sentence is not a counterexample in this run. It is also not a reason to widen the approval.

SP05-W01 stays open. The ledger is unchanged. No parent checkbox and no release claim follows from these six tests.
