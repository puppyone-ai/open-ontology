# REPORT — a202 local download EOF and await repair

This is the repair of the two limits in review `ef6c4fc7da03342f900146e90ff692fe742d1ba4` (a194) against product commit `e3363ffecc21af4dc9fcf75509978da34b0860b1`. The repair commit is `5e3ba29f8d460a1aa6919733100c34a68a2fdb75`, parent `b3f8f6161c40c87686c1159421b800a53f874d6f`, tree `846870547f225b2bdddc86385539d4c0c79f4427`. Author evidence for the original pull remains `a699f8da3dfdae918eb1129509d074c7d1d3bdce`. a188 (`b6cddea1`) remains the quota and 8192 publish measurement.

This directory does not approve the repair, does not close SP05-W01 or SP05-W06, does not change the ledger, and is not a production release. The next reader is an independent fixed review.

## Files

`crates/oo-control/src/file_hosting.rs`, `crates/oo-control/tests/file_hosting.rs`, and `contracts/control/local-file-hosting-v1.md`. `lib.rs`, `crates/oo-control/Cargo.toml`, and `Cargo.lock` were not edited. Identities are in `source/blobs.txt`. `Cargo.lock` is stored as a hash only. The tested lock blob is `2cc7617c9e06a849de890b4ef28e5f1331988e15` (sha256 `9608f88b7dbe46674e064f005280768cf9c383cce18827a57a8241056fa03f08`), which matches the archive parent and differs from the e336 lock blob `2147cc3bd9fed782c209d2125cdba5b266291c45` because commits after e336 changed the lock.

The pre-edit reproduction is `pre-edit/repro_download_eof.rs`. It is evidence only. It is not in the product commit.

## Runs

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One Rust builder at a time. No PostgreSQL. Private network `oo-a202-eef831aa-net` (`internal=false`, used for the toolchain component download and `cargo fetch`). Private volumes `oo-a202-eef831aa-cargo` and `oo-a202-eef831aa-target`. Container flags: user `0:0`, `no-new-privileges`, cap-drop `ALL`, memory and swap 8g, cpus 4, pids 4096, tmpfs `/tmp` 2g, `HOME=/tmp`. No host home, docker socket, host network, or shared cargo cache. `--pull never`. Hash oracle is `shasum` and `git hash-object`. No Python `hashlib`.

`inside.sh` prints `airgap_claim=no`. Tests and clippy use `--locked --offline` after fetch.

### Pre-edit, before this repair

Archive of HEAD `0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1`. The three product blobs still matched e336. `pre-edit/docker-run.exit` is 0. `pre-edit/phases/worst.exit` is 0. `cargo fmt --check -p oo-control` clean. `cargo test --locked --offline -p oo-control --test file_hosting --lib`: 13 passed, 0 failed, 19.80s. Clippy `-D warnings` clean. Lock unchanged during that run.

The reproduction overlay was copied onto the archive only after that smoke. Two tests passed in 0.48s and asserted the defects still present on unmodified code:

- `pre-edit/repro-notes/consumed-prefix.txt`: `consumed_prefix_flip_complete=1`, running hash matches the snapshot, stored file does not. Source is the 20 bytes `abcdefghij0123456789`, sha256 `7ddccfc3e23baf04a142c2c74182139117ecfa82697d7a22404ceb4ac1a8a48b` (`shasum` of those bytes, no newline).
- `pre-edit/repro-notes/held-get.txt`: `bytes_started_after_deadline_during_get_opts=1`, `deadline_dropped_get_opts=0`, result `Stopped`, display `download stopped before the object was verified`, `complete=0`.

Unmodified download high-water, same 8 MiB and 32 MiB tests, pattern `0xA5`: delta 8192 and 4096 bytes. `download_chunk_max=8192`. `fixture_object_vec=0`. `VmPeak` did not move during the pull.

### Preserved verify failures

`verify-attempt-1/`: `docker-run.exit` 101, `phases/test.exit` 101, `OOMKilled=false`. fmt, fetch, and the lock check were 0. The test crate did not compile. `ObjectStoreExt` was missing, so existing `LocalFileSystem::get` and `put` calls failed. Clippy did not run. Console left in place.

`verify-attempt-2/`: `docker-run.exit` 101, `phases/test.exit` 0, `phases/clippy.exit` 101, `OOMKilled=false`. 16 tests passed. Clippy denied `while_let_loop` on the generic-store pull loop in `tests/file_hosting.rs`. `ATTEMPT.txt` records that. No `worst.exit`, because the script stopped at clippy.

### Green verify

`verify/docker-run.exit` is 0. `verify/phases/worst.exit` is 0. `inspect.txt`: `ExitCode=0`, `OOMKilled=false`. Archive of parent `b3f8f6161c40c87686c1159421b800a53f874d6f` plus the three working-tree files. `verify/overlay-before.txt` and `verify/archive-after.txt` record the same sha256 for those three files, and those sha256 values are the committed blobs. `lock_matches_head=1`.

Commands, from `verify/inside.sh`:

- `cargo fetch --manifest-path Cargo.toml --locked`
- `cargo fmt -- crates/oo-control/src/file_hosting.rs crates/oo-control/tests/file_hosting.rs` then `cargo fmt -- --check` on the same two files
- `cargo test --manifest-path Cargo.toml --locked --offline -p oo-control --test file_hosting --lib -- --nocapture --test-threads=1`
- `cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-control --all-targets -- -D warnings`

Library tests: 0 passed, 0 failed, 0.00s. Integration tests: 16 passed, 0 failed, 36.45s. The three new tests passed: `download_eof_rejects_flipped_bytes_and_keeps_a_clean_object`, `download_deadline_does_not_start_bytes_after_held_get`, `download_regressions_deadline_cancel_manifest_truncate_quota`. The existing download, quota, and memory tests passed in the same process, including `concurrent_quota_two_hosts_and_child` and `download_pull_memory_is_bounded`. Clippy `-D warnings` clean. fmt phases 0. Lock phases 0.

Capacity at launch, `verify/capacity.txt`: `running_rust_builders=0`, `own_rust_builder_running=0`, `host_cargo_rustc_rustdoc_rustup=0`. Container name `oo-a202-verify`.

## Download resident set on the repair

Fresh child, pattern `0xA5`, parts written in 8192-byte slices, `complete` first, then one pull with the caller holding one chunk. The high-water window includes the end-of-object rehash, because that rehash runs inside the `next_chunk` that finishes the object. Parent metrics are `verify/metrics/download-memory-8388608.txt` and `verify/metrics/download-memory-33554432.txt`.

| Sample | 8388608 | 33554432 |
|---|---|---|
| `download_hwm_delta_bytes` | 77824 | 73728 |
| `download_chunk_max` | 8192 | 8192 |
| `download_pulled` | 8388608 | 33554432 |
| `download_complete` | 1 | 1 |
| `VmPeak` before and after | 210884 KiB | 210884 KiB |
| sha256 | `e474a45740e5efac02e9f7e0626b3f86111b293f8363e477786dfad64e98df3b` | `edeadec8f638055689d5be63b4bcf2654fb64bf91fb6651e9a924f052a9c7db0` |

The child digest, the pattern digest, and the file digest match on each size. `fixture_object_vec=0`. `caller_retained_chunks=1`. Both deltas are under 4 MiB, and twice either delta is under the object size. The ceiling in the test was not raised. The pre-edit deltas on the same test were 8192 and 4096; the rehash raised the observed high-water and did not make it grow with the object. The 32 MiB delta is smaller than the 8 MiB delta.

`streaming_complete_memory_is_bounded` also passed in this process. Publish-path deltas were 110592 bytes on 8 MiB and 139264 bytes on 32 MiB (`verify/metrics/memory-8388608.txt`, `verify/metrics/memory-33554432.txt`). That test is the existing publish measurement. This repair did not change its ceiling.
