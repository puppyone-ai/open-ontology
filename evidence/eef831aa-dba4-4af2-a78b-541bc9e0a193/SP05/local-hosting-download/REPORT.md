# REPORT — a193 local object download pull

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a193`. One Rust builder, no PostgreSQL.

`FileHost::open_download` pulls a published local object for a trusted admission context. Each `next_chunk` is one `GetRange::Bounded` read of at most 8192 bytes, or a smaller validated budget. `read_object` still returns one buffer of the object size. `is_complete` becomes true only after the stored manifest bytes, the object length, and the running sha256 match the snapshot taken at open.

Product commit `e3363ffecc21af4dc9fcf75509978da34b0860b1` (`source-commit.txt`). Independent review of that commit is next. SP05-W01 and SP05-W06 stay open. The ledger is unchanged. This run is not a customer deployment and not a production release.

## Source

| Item | Value |
|---|---|
| Baseline | `15d23fe9f2cbc74e3f3278d70d0f56390568126e` |
| Baseline tree | `feec821ab54f0aaf0001218f90150befce2803b2` |
| Parent of the product commit | `33426a733d8f569a74e8074745a2a77528b1cb53` |
| Product commit | `e3363ffecc21af4dc9fcf75509978da34b0860b1` |
| `file_hosting.rs` blob | `1af90356ae515da382d4dc23c783fa3f42d1c93b` |
| `file_hosting.rs` sha256 | `a8ac952d3d4842bb7b357b7201b76a19c7c940b770042bcaaab86f2b32ab33aa` |
| `tests/file_hosting.rs` blob | `89f7381297fc6db02e36a73ea8f8ccb20cc9358d` |
| `tests/file_hosting.rs` sha256 | `4bd62a75f4416f697cea5bd24075d0834ea34866d5ca67810ea941b0faf70690` |
| Contract blob | `3418685b2a009d75eea7760788d48b1655d30d3e` |
| Contract sha256 | `e5e4315399ee8367580aa567dca6b34bf3f809a42110730fca56a4e40e7d29bd` |

`crates/oo-control/src/lib.rs`, `crates/oo-control/Cargo.toml`, and `Cargo.lock` keep the baseline blobs. `post-edit/overlay-after.sha256` matches the three committed file hashes. `post-edit/unrelated-match.txt` records the same for the lock, the control manifest, and `lib.rs`. `source/blobs.txt` lists blob and sha256 for those six paths. `source/blobs/` holds the bytes of the three committed files plus unchanged `lib.rs` and `crates/oo-control/Cargo.toml`. `Cargo.lock` is hash-only in `source/blobs.txt`.

The build used `git archive` of the baseline plus those three overlay files. It did not compile the dirty workspace.

## Pre-edit smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

`pre-edit/docker-run.exit` is 0. `pre-edit/phases/worst.exit` is 0. `pre-edit/console.log` ends with `pre_edit_smoke=pass`. `cargo test --locked --offline -p oo-control --test file_hosting --lib` reported 0 library tests and 10 integration tests passed in 10.18s. Clippy `--all-targets -D warnings` was clean. The lock file was unchanged. `pre-edit/archive-match.txt` matches the baseline blobs, including `file_hosting.rs` `d5fb6ac06c022edceb820e7b153d7095722a9f59`.

Container `oo-a193-pre-edit`. Network `oo-a193-eef831aa-net` (`internal=false`). Volumes `oo-a193-eef831aa-cargo` and `oo-a193-eef831aa-target`. Privileged false, CapDrop ALL, no-new-privileges, no published ports, memory and swap 8 GiB, `/tmp` tmpfs 1 GiB. `HOME=/tmp`. No PostgreSQL. `pre-edit/capacity.txt` recorded `running_rust_builders=0`, `own_rust_builder_running=0`, `host_cargo_rustc_rustdoc_rustup=0`.

## Post-edit smoke

Same image, toolchain, network, and volumes. Container `oo-a193-post-edit`. `/tmp` tmpfs 2 GiB. `OO_FH_METRICS_DIR=/evidence/metrics`. `post-edit/capacity.txt` recorded the same zero builder counts. `post-edit/docker-run.exit` is 0. `post-edit/inspect.txt` says `ExitCode=0`, `OOMKilled=false`. `post-edit/hostconfig.txt` matches the containment above with `size=2g`.

Phase exits in `post-edit/phases/` are 0 for rustfmt, clippy, `cargo fmt`, `cargo fmt --check`, fetch, lock unchanged, test, clippy, lock-after, and worst. `post-edit/fmt-sync.txt` says `file_hosting.rs` was unchanged and `tests/file_hosting.rs` was copied back after rustfmt. The copied test bytes are the committed bytes.

`cargo test --locked --offline -p oo-control --test file_hosting --lib -- --nocapture` finished in 19.84s: 0 library tests, 13 integration tests passed.

- `checksum_and_size_failure_leave_no_visible_object`
- `abort_releases_quota_and_leaves_foreign_bytes`
- `conflicting_part_and_config_do_not_overwrite`
- `concurrent_complete_and_part_keep_one_object`
- `download_pull_refuses_other_context_and_altered_bytes`
- `generic_store_refuses_before_any_write`
- `limits_and_names_are_refused_before_write`
- `multipart_reopen_reads_manifest_and_lineage`
- `concurrent_quota_two_hosts_and_child`
- `two_contexts_deny_a_known_key`
- `download_pull_verifies_identity_without_an_object_buffer`
- `streaming_complete_memory_is_bounded`
- `download_pull_memory_is_bounded`

The quota race and the streaming `complete` memory test from a186 ran again in this binary and passed. Their resident deltas during `complete` were 110592 bytes on 8388608 bytes and 139264 bytes on 33554432 bytes (`post-edit/metrics/memory-8388608.txt`, `memory-33554432.txt`).

## Download measurements

The pull samples are one child per size. The part was planted with 8192-byte writes. `complete` ran before the high-water window. The window is `open_download` plus `next_chunk` only. The caller keeps the current chunk. The pattern digest, the pull digest, and a second 8192-byte read of the published file are all `sha2` in the test process. No host Python hashlib.

| Sample | 8388608 | 33554432 |
|---|---|---|
| `download_hwm_delta_bytes` | 8192 | 4096 |
| `download_chunk_max` | 8192 | 8192 |
| `download_pulled` | 8388608 | 33554432 |
| `download_complete` | 1 | 1 |
| threads before and after | 4 / 4 | 4 / 4 |
| sha256 | `e474a45740e5efac02e9f7e0626b3f86111b293f8363e477786dfad64e98df3b` | `edeadec8f638055689d5be63b4bcf2654fb64bf91fb6651e9a924f052a9c7db0` |

Both resident deltas are under 4 MiB, and the larger object did not raise the delta. Raw lines are `post-edit/metrics/download-memory-8388608.txt` and `download-memory-33554432.txt`. The parent recomputed the pattern and the published file with the same Safe Rust sha256 and required all three digests to match.

The other new tests, on small objects, covered a budget of 8 with widths 8, 8, and 4, a second `Ok(None)` after completion, `read_object` still returning the small buffer, early drop, `cancel`, a deadline already passed, a deadline that passes between chunks, a one-poll drop of `next_chunk`, a foreign context (`Denied`), a same-length byte flip (`Corrupt`, pull left incomplete), a zero-length object file (`Corrupt` at open), a truncate after the first chunk (`Store` or `Corrupt`, incomplete), a flipped manifest (`Corrupt` at the end, snapshot unchanged), a forged size-0 manifest (`Corrupt`), and a chunk budget of 0 or 8193 (`Limit` before a missing key is consulted). `ObjectKey::parse` of `../secret` and `/etc/passwd` stayed `NotFound`.

## Contract delta

`lock_mutex` at `file_hosting.rs:1349` still recovers with `into_inner`. The contract sentence that said a poisoned wait returns `Unavailable` now says the wait continues. No poison-injection test was added.

## Still open

S3 and MinIO, conditional quota on a generic store, quota across hosts that do not share one deployment directory, `dest#N` leftovers, HTTP, an auth proxy, grant authentication, revocation, TTL, presigned URLs, formula-safe output, retention delete, and a reaper. `read_object` still returns one buffer of the object size. This pull is a library call on a trusted project and principal tuple. It is not a public download.

## Cleanup

Recorded in `cleanup-done.txt` after the reports were written and before `SHA256SUMS`. Own containers were already gone: `docker rm -f oo-a193-pre-edit oo-a193-post-edit` printed `No such container` for both and exited 0. That log then removed network `oo-a193-eef831aa-net`, volumes `oo-a193-eef831aa-cargo` and `oo-a193-eef831aa-target`, `/tmp/oo-a193-eef831aa-dba4-4af2-a78b-541bc9e0a193`, and the source-commit scratch `/tmp/oo-a193-commit.txt`. Other owners' containers, networks, and volumes were left in place, including `oo-a190-pg`, `oo-a190-rust194`, and `oo-a191-pg`. Nothing was pruned.
