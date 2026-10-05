# REPORT — a209 review of the local download EOF repair

Verdict: **APPROVE LIMITED** for the local pull on `5e3ba29f8d460a1aa6919733100c34a68a2fdb75`. No new component.

This review measured that commit. It did not edit product source, manifests, the lock, or the ledger. It does not close SP05-W01, SP05-W06, AT-FILE-01, or AT-FILE-05, and it is not a release.

Author evidence is `84c03d01ce35728924100e42ca142c0ef650ac71`. The review under repair is `ef6c4fc7da03342f900146e90ff692fe742d1ba4` on `e3363ffecc21af4dc9fcf75509978da34b0860b1`. Parent of the fixed commit is `b3f8f6161c40c87686c1159421b800a53f874d6f`. Tree is `846870547f225b2bdddc86385539d4c0c79f4427`. Subject is `control: rehash a local object before download completion`.

## What was run

Linux/arm64 image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One builder. No PostgreSQL. Private bridge network `oo-a209-eef831aa-net` (`internal=false`, used for the toolchain component download and `cargo fetch`). Private volumes `oo-a209-eef831aa-cargo` and `oo-a209-eef831aa-target`. Container flags: user `0:0`, `no-new-privileges`, cap-drop `ALL`, memory and swap 8g, cpus 4, pids 4096, tmpfs `/tmp` 2g, `HOME=/tmp`. No host home, docker socket, host network, or shared cargo cache. `--pull never`. Builds used `git archive` of `5e3ba29f` only. Hash oracle is `shasum` and `git hash-object`. No Python hasher.

`airgap_claim=no` is printed by both `inside.sh` scripts. Tests and clippy use `--locked --offline` after fetch.

The smoke archive had no overlay (`pre-edit/archive-after.txt` `overlay_present_after=0`). The oracle archive is that same commit plus `crates/oo-control/tests/review_download.rs` copied after extraction. Product sha256 values after the oracle match the fixed commit. `Cargo.lock` sha256 stayed `9608f88b7dbe46674e064f005280768cf9c383cce18827a57a8241056fa03f08`.

## Smoke, before the overlay

`pre-edit-attempt-1/` is the first smoke. `docker-run.exit` is 1. `phases/fmt-check.exit` is 1. `OOMKilled=false`. `cargo fmt -- --check` with two file arguments still walked the workspace and failed on `oo-builder` files that are outside this review. Fetch and the lock check were 0. The file-hosting tests did not run.

`pre-edit/` is the rerun with `cargo fmt -p oo-control -- --check`. `docker-run.exit` is 0. `phases/worst.exit` is 0. fmt, the 16 `file_hosting` tests, and clippy `-D warnings` are 0. The tests finished in 37.99s with 16 passed and 0 failed. The committed download test printed high-water deltas of 73728 bytes on both 8388608 and 33554432, chunk max 8192. Those figures belong to that suite. The independent measurement below uses a different pattern and a fresh child.

## Oracle attempts kept

`oracle-attempt-0/`: the first launch script was not executable. The kernel refused it before docker. No container and no console.

`oracle-attempt-1/`: `docker-run.exit` 101, `phases/test.exit` 101, `OOMKilled=false`. The overlay did not compile. `ObjectDownload` is not re-exported. `Arc::clone` was applied to atomics inside `Arc<ProbeLog>`. A pinned `next_chunk` future still borrowed the reader. Product hashes and the lock were unchanged.

`oracle-attempt-2/`: `docker-run.exit` 101, `phases/test.exit` 101, `OOMKilled=false`. `GetResult.range` is `Range<u64>`, which is `Clone` and not `Copy`, so moving the result into the held `bytes()` stream was a partial move.

`oracle-attempt-3/`: `docker-run.exit` 101, `phases/test.exit` 0, `phases/clippy.exit` 101, `OOMKilled=false`. The three review tests passed in 22.19s. Clippy denied `dead_code` on unused range endpoints, `while_let_loop` on two pull loops, and `needless_borrow` on `Context::from_waker`. Notes from that passing run are kept in that directory.

## Green oracle

`oracle/docker-run.exit` is 0. `oracle/phases/worst.exit` is 0. `inspect.txt`: `ExitCode=0`, `OOMKilled=false`. rustfmt 1.94 rewrote the overlay (`overlay_rustfmt=rewritten`), then `cargo fmt -p oo-control -- --check` passed. Launch input sha256 `2ca6c370f7944ff71b3ec7d12198020ff50158560c5ef6a63a8f6e493af42d77`. Tested file `oracle/ran/review_download.rs` sha256 `7e46e11b87d4755948195d734fcf27bc54f3e8e40f987ea3e4bec4c043634334`, matching `oracle/archive-after.txt`.

`cargo test --locked --offline -p oo-control --test review_download -- --test-threads=1`: 3 passed, 0 failed, 21.83s.

- `review_local_clean_eof_prefix_flip_and_quota`
- `review_ranges_etag_and_deadline`
- `review_memory_is_incremental`

Clippy `--locked --offline -p oo-control --all-targets -- -D warnings` exited 0. The lock check after that exited 0.

### Local file, known bytes

Payload `OOA209-known-payload-v1`, 23 bytes. The test hashed it with `sha2` in the same process. `oracle/notes/local.txt` records sha256 `e0e224046caa35812b45c51b8a26954aae99bd241927ef719b34af2b1fe9f899`. Clean EOF completed. Returned bytes, the running hash, the file hash, and the manifest digest matched. Quota file bytes and quota directory names were unchanged. `begin`, `put_part`, `complete`, and `abort` on `FileHost::new` returned `Unsupported`.

Consumed-prefix flip, byte 0 after the first chunk and before EOF: `Corrupt`, incomplete. The running hash of the returned chunks still matched the source. The file hash did not. An unread last-byte flip was `Corrupt` and incomplete. A generic-store prefix flip was `Corrupt`.

After a clean EOF, flipping stored byte 0 left `is_complete` true and the next `next_chunk` returned `Ok(None)` without another body read. A new `open_download` of that key was `Corrupt` and incomplete. The open manifest digest was still the source digest.

Truncate to zero after the first chunk returned `Store`. The pull stayed incomplete. The display text contained no `/`.

### Ranges, etag, deadline

Object of 20000 bytes on `FileHost::new(Probe)`. Body ranges were 20 of length 1000 at `i*1000..(i+1)*1000`, then rehash `0..8192`, `8192..16384`, `16384..20000`. No unbounded body range. No body range over 8192. Manifest ranges were at most 4096. Source, returned, and file hashes matched (`b711d0b9615686ec1bc4c42d75657874afc0beb005157edd54492882366348e7`). After complete, flipping byte 3 added no body get and `is_complete` stayed true.

A stable etag `a209-etag-a` across the heads completed. An etag-only change to `a209-etag-b` on the post-rehash head, with identical bytes, was `Corrupt` and incomplete. The running hash and the file hash still matched the source. `FileHost::new` has no local stat; this is the etag window the author left untested.

A `get_opts` held past a 300 ms deadline was not dropped and did not start `bytes()`. The result was `Stopped` with display `download stopped before the object was verified`. The following `next_chunk` was `Stopped`. A `bytes()` future held past a 400 ms deadline was not dropped, then `Stopped`, and the pull stayed incomplete. A thread sleep of 1500 ms inside `get_opts` stayed pending past a 250 ms deadline, was not dropped, did not start `bytes()`, and then returned `Stopped`. An already expired deadline issued zero body gets and returned `Stopped`. Cancel after one chunk was `Stopped` and incomplete.

### Resident memory

Fresh child per size. Pattern byte `0x5A` (decimal 90). The parent publishes. The child only downloads and holds one chunk. The parent recomputes the file hash with an 8192-byte buffer.

| bytes | hwm delta | chunk max | pulled | complete | sha256 |
| --- | --- | --- | --- | --- | --- |
| 8388608 | 65536 | 8192 | 8388608 | 1 | `7014ae0f2fc0fee42a440b97859207efb72ffee09d4864f7433f1bf756a17aca` |
| 33554432 | 36864 | 8192 | 33554432 | 1 | `371036ccdfc733fa30542a26a4f276536147c906d1570f0becc1d6f8b868c311` |

Both deltas are under 4 MiB and under half the object. `fixture_object_vec=0`. VmPeak on the 8 MiB child moved from 207436 KiB to 209588 KiB. VmPeak on the 32 MiB child stayed 207436 KiB. These are not the author's `0xA5` figures and not a194's `0x3C` figures.

## Limits of this approval

An await that has already been polled runs until that call returns. Drop and cancel set the flag that the next check reads. They do not abort a filesystem call already in flight. The 1500 ms sleep is that observation.

`is_complete` after a verified end is the check at that end. A later write does not clear it. A later reader of the changed file is `Corrupt`.

`read_object` at `file_hosting.rs:691` still `try_reserve`s the object size. This review did not call it. The 8 MiB and 32 MiB figures are `open_download` only.

A truncate that fails the next range read is `Store` (`await_get` maps every `get_opts` error to `Store` at line 1404) and the pull stays incomplete.

The local pull is not an HTTP download, an auth proxy, a TTL, a revocation check, a reaper, or an S3 backend. SP05-W06 in the spec still requires those. This approval does not add a component and does not close that work.
