# REPORT — a194 local streaming download review

Verdict: **APPROVE LIMITED**.

The limited approval is one local `LocalFileSystem` deployment directory on product commit `e3363ffecc21af4dc9fcf75509978da34b0860b1` (tree `fb3f57bfcb2853e56a617549be340043fe3cd7fb`, parent `33426a733d8f569a74e8074745a2a77528b1cb53`). Author evidence is `a699f8da3dfdae918eb1129509d074c7d1d3bdce`. a188 (`b6cddea1`) remains the limited approval of the quota critical section and the 8192-byte publish path. This review measured the pull. It does not close SP05-W01 or SP05-W06, does not change the ledger, and is not a production release.

## What was reviewed

`FileHost::open_download` and `ObjectDownload::next_chunk` in `crates/oo-control/src/file_hosting.rs`, the author tests in `crates/oo-control/tests/file_hosting.rs`, and `contracts/control/local-file-hosting-v1.md`. `lib.rs`, `crates/oo-control/Cargo.toml`, and `Cargo.lock` are unchanged from that commit. The six identities are in `source/blobs.txt`. `Cargo.lock` is stored as a hash only.

The independent harness is `oracle/review_download.rs`, sha256 `b4b0d18a317d019026143be4984c2cbd706de77539cbbb5235b6e3caf508827c`. It is Safe Rust. Digests are the `sha2` crate. The parent recomputed the planted files with `sha2` as well. No Python `hashlib`.

## Runs

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One Rust builder at a time. No PostgreSQL. Private network `oo-a194-eef831aa-net` (`internal=false` for the component download). Private volumes `oo-a194-eef831aa-cargo` and `oo-a194-eef831aa-target`. Container flags: user `0:0`, `no-new-privileges`, cap-drop `ALL`, memory and swap 8g, cpus 4, pids 4096, tmpfs `/tmp` 2g, `HOME=/tmp`. No host home, docker socket, host network, or shared cargo cache. `--pull never`.

Pre-overlay smoke of the git archive, before any overlay file existed: `pre-edit/docker-run.exit` 0, `pre-edit/phases/worst.exit` 0. `cargo fmt --check -p oo-control` clean. `cargo test --locked --offline -p oo-control --test file_hosting --lib` : 13 tests passed in 19.24s, including `concurrent_quota_two_hosts_and_child`. Clippy `-D warnings` clean. Lock unchanged. The six product paths still matched the fixed commit (`pre-edit/archive-after.txt`). That quota test was not run again.

Overlay attempt 4 is the green run: `oracle-attempt-4/docker-run.exit` 0, `phases/worst.exit` 0. rustfmt touched only `review_download.rs` (`overlay_fmt_changed=1`). `cargo fmt --check -p oo-control` clean. Five overlay tests passed in 21.11s. Clippy `-D warnings` clean. Lock unchanged. `archive_unchanged_after_overlay=1`. `OOMKilled=false`.

Preserved earlier attempts, consoles left in place:

- `oracle/`: compile failed, exit 101. `Range<u64>` is not `Copy`, and `pin!` kept the `next_chunk` borrow alive past an explicit `drop`.
- `oracle-attempt-2/`: 4 passed, `review_download_exact_pull_and_refusals` failed. The harness compared quota names captured before the second `publish`. The product had added the second session's quota file. Memory, mutations, generic store, and the pending-get test had already passed in that process.
- `oracle-attempt-3/`: 5 tests passed in 20.47s. Clippy denied five harness lints. Exit 101.

## What the green run measured

Pattern byte `0x3C`, planted in 8192-byte writes, then `complete`, then `open_download` / `next_chunk` with the caller holding one chunk. The high-water window starts after `complete`. Parent `sha2` matched the child's download digest, the pattern digest, and the file digest.

| Sample | 8388608 | 33554432 |
|---|---|---|
| `hwm_delta_bytes` | 73728 | 77824 |
| ranges | 1024 of at most 8192 | 4096 of at most 8192 |
| `range_sum` | 8388608 | 33554432 |
| `body_unbounded` | 0 | 0 |
| second EOF extra body gets | 0 | 0 |
| sha256 | `7837430d6dfb147fe6e0455f954f6ecd460c0fa45bebbab7cfc40d383ecb364c` | `963593ae1962f4b89712cf618f2e31f9cebe6b1eed84eb7872fcfda617859e78` |

Both deltas are under 4 MiB, and twice either delta is under the object size. `VmPeak` did not move during the pull. Threads stayed at 4. `caller_retained_chunks=1`. Quota bytes for that session were unchanged.

On a 20-byte object the body ranges were `(0,8)`, `(8,16)`, `(16,20)` at budget 8; twenty 1-byte ranges at budget 1; ranges ending in a 2-byte chunk at budget 3; and one `(0,20)` range at budget 8192. A 9000-byte object at budget 8192 was `(0,8192)` then `(8192,9000)`. Budgets 0 and 8193 returned `Limit` with no body `get_opts`. A second `Ok(None)` did not issue another body get. `cancel` after a verified end left `is_complete` true. `is_complete` stayed false on every returned chunk.

`FileHost::new` returned `Unsupported` for `begin`, `put_part`, `complete`, and `abort`, left the quota bytes and names unchanged, then pulled the already stored object. `read_object` on the 20-byte object returned one buffer of that size. The 8 MiB and 32 MiB samples did not call `read_object`.

A different grant string with the same project and principal opened the pull. A different project or principal was `Denied`. The `Denied` and `Stopped` display strings contain no path and no grant text. `Debug` of the reader omitted the grant and the deployment path.

Unread last-byte flip: `Corrupt`, incomplete, snapshot unchanged. Append one byte after every chunk and before EOF: `Corrupt`, incomplete. Manifest byte flip: `Corrupt`, incomplete, snapshot unchanged. Forged object key: `NotFound`, object bytes unchanged. Forged size 0, empty manifest, and a 4097-byte manifest: `Corrupt`. Deleted manifest: `NotFound`. Truncate to length 0 after the first chunk: `Store`, display has no `/`, incomplete. `body_unbounded` stayed 0.

A deadline that expired while `get_opts` was held did not drop that call and did not skip `bytes()`. The chunk was then `Stopped` (`download stopped before the object was verified`), `is_complete` stayed false, and the next `next_chunk` was `Stopped` with no further body get. Dropping an in-flight `next_chunk` set the cancel flag and did not finish the held wait early. A deadline already in the past returned `Stopped` before a body get.

## Limits recorded with this approval

The deadline and cancel checks sit around `read_bounded_range` as one await (`file_hosting.rs` 1216 and 1225). Inside that helper, `get_opts` (1546) and `bytes` (1550) are separate awaits, and nothing in the host runs between them. Contract line 121 also says `Drop` and cancel do not cancel a local filesystem read that has already started. The measured behavior matches that sentence. It does not match a reading that requires a check between those two awaits. That is a limit of this approval. It is not a failed completion predicate: the late chunk was discarded and `is_complete` stayed false.

`finish_eof` (1244) compares the running hash of chunks already pulled, the stored length, and the manifest bytes. After the first 8 bytes were pulled, flipping stored byte 0 still reached `Ok(None)` and `is_complete == true`. The running hash matched the snapshot. The file on disk did not. An unread byte did return `Corrupt` and stayed incomplete. Contract line 119 describes the running hash. The later sentence that a changed object hash is `Corrupt` is broader than this EOF check. That is a limit of this approval.

Grant evidence is stored and is not checked. `read_object` still allocates one buffer of the object size. HTTP, an auth proxy, revocation, TTL, presigned URLs, a reaper, S3, MinIO, conditional quota on a generic store, quota across directories, `dest#N` leftovers, formula-safe output, and retention delete stay open.

## Cleanup

`cleanup-done.txt` is the command log. The launch traps had already removed the a194 containers. The cleanup removed network `oo-a194-eef831aa-net`, volumes `oo-a194-eef831aa-cargo` and `oo-a194-eef831aa-target`, and `/tmp/oo-a194-eef831aa-dba4-4af2-a78b-541bc9e0a194`. `oo-a196-red` and `oo-a196-pg` were still running afterward and were left in place. Nothing was pruned.
