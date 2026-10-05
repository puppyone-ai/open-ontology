# REPORT — a186 local upload streaming and quota

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a186`. Slot C. One Rust builder, no PostgreSQL.

Local project quota on one deployment directory is now one critical section across `FileHost` handles and processes. `complete` hashes and publishes stored parts in ranges of 8192 bytes. `FileHost::new` returns `Unsupported` before it writes when that lock is absent.

Product commit `3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6` (`source-commit.txt`). Independent review of that commit is next. SP05-W01 stays open. The ledger is unchanged. This run is not a customer deployment and not a production release.

## Source

| Item | Value |
|---|---|
| Fixed source compared | `755fb84acc7f80e5ba2fba1c2b80412dab7fab26` |
| a183 evidence | `950406b8` |
| Workspace HEAD at the green smoke | `4e495b0df4e8ce6ebfa43052299dd490e10b9669` |
| Product commit | `3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6` |
| Committed hosting blobs at that HEAD | same as `755fb84` for `file_hosting.rs`, `tests/file_hosting.rs`, `local-file-hosting-v1.md`, `Cargo.toml`, and `lib.rs` |
| Working tree diff | 3 files, 1039 insertions, 55 deletions |

`crates/oo-control/Cargo.toml`, `Cargo.lock`, and `crates/oo-control/src/lib.rs` are unchanged. No new crate version and no root manifest change.

## Pre-edit smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`.

`pre-edit/docker-run.exit` is 0. `pre-edit/phases/worst.exit` is 0. `pre-edit/console.log` ends with `pre_edit_smoke=pass`. `cargo test --locked --offline -p oo-control --test file_hosting --lib` reported 0 library tests and 6 integration tests passed. Clippy `--all-targets -D warnings` was clean. The lock file was unchanged. `pre-edit/archive-match.txt` says `archive_hashes_match=1`. `pre-edit/archive-tree.txt` is `5bdc63b3c86c537e4be765c35085783ad1c83212`.

Container `oo-a186-pre-edit`. Network `oo-a186-eef831aa-net` (`internal=false`). Volumes `oo-a186-eef831aa-cargo` and `oo-a186-eef831aa-target`. Privileged false, CapDrop ALL, no-new-privileges, no published ports, memory and swap 8 GiB, `/tmp` tmpfs 1 GiB. `HOME=/tmp`. No PostgreSQL.

## Post-edit smoke

Same image, toolchain, network, and volumes. Container `oo-a186-post-edit`. `/tmp` tmpfs 2 GiB. `OO_FH_METRICS_DIR=/evidence/metrics`. `post-edit/capacity.txt` recorded `running_rust_builders=0`, `own_rust_builder_running=0`, `host_cargo_rustc_rustdoc_rustup=0`. `post-edit/docker-run.exit` is 0. `post-edit/inspect.txt` says `ExitCode=0`, `OOMKilled=false`. `post-edit/hostconfig.txt` matches the containment above with `size=2g`.

Phase exits in `post-edit/phases/` are 0 for rustfmt component, clippy component, `cargo fmt`, `cargo fmt --check`, fetch, lock unchanged, test, clippy, lock-after, and worst. `post-edit/fmt-sync.txt` says both Rust files were unchanged after rustfmt, so the workspace bytes are the bytes that compiled.

`cargo test --locked --offline -p oo-control --test file_hosting --lib -- --nocapture` finished in 10.00s: 0 library tests, 10 integration tests passed.

- `multipart_reopen_reads_manifest_and_lineage`
- `conflicting_part_and_config_do_not_overwrite`
- `two_contexts_deny_a_known_key`
- `checksum_and_size_failure_leave_no_visible_object`
- `abort_releases_quota_and_leaves_foreign_bytes`
- `limits_and_names_are_refused_before_write`
- `generic_store_refuses_before_any_write`
- `concurrent_quota_two_hosts_and_child`
- `concurrent_complete_and_part_keep_one_object`
- `streaming_complete_memory_is_bounded`

Clippy `--all-targets -D warnings` finished clean. `Cargo.lock` was unchanged after fetch and after the smoke.

## Resident memory

Each size ran in its own child. The child wrote the part with `std::fs` in 8192-byte chunks, then sampled `/proc/self/status` around `complete` only.

| Sample | 8388608 bytes | 33554432 bytes |
|---|---|---|
| `memory_hwm_delta_bytes` | 114688 | 122880 |
| `memory_vmpeak_delta_bytes` | 2203648 | 2203648 |
| `memory_threads_before` / `after` | 3 / 3 | 3 / 3 |
| `object_sha_match` | 1 | 1 |
| `part_gone` | true | true |
| `memory_ok` | 1 | 1 |

a183 measured `memory_hwm_delta_bytes=16801792` on an 8388608-byte part. These two deltas stay near 120 KiB and do not grow with the part. Raw lines are `post-edit/metrics/memory-8388608.txt` and `post-edit/metrics/memory-33554432.txt`.

## Failed attempts kept

`post-edit/attempt1/` is the first compile. `tests/file_hosting.rs` dropped `task_a` after `await`. `test.exit` is 101. Rustfmt had already rewritten the overlay; `fmt-sync.txt` records the copy back.

`post-edit/attempt2/` is the 8 MiB memory sample that also required virtual-peak delta to be below the part size. Resident delta was 143360 bytes. Virtual-peak delta was 136380416 bytes. The object hash matched and the part was gone. `memory_ok=0`. `test.exit` is 101. The other nine tests had already passed in that process. The gate on the green run is resident high-water below half the part. Virtual peak is still recorded.

## Still open

S3 and MinIO, conditional quota on a generic store, quota across hosts that do not share one deployment directory, `dest#N` leftovers, a crash between owned keys, download and TTL, HTTP and an auth proxy, MIME and retention delete, presigned URLs, REQ-SRC-05 after `stored`, grant authentication, and a reaper. Customer deployment and a production release are outside this slice. SP05-W01 and SP05-W06 are unchanged. AT-FILE-01 and AT-FILE-05 are unchanged.
