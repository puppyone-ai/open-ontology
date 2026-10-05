# REPORT — a188 local hosting bounds review

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a188`. Slot A. Verdict: **APPROVE LIMITED**.

The limited approval covers one local deployment directory on `LocalFileSystem`: the project-quota critical section, and `complete`'s hash and publish of stored parts in ranges of 8192 bytes. It is this review's measurement of product commit `3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6`. Author evidence `523951fd` is context. Its resident-set numbers are not this review's proof.

SP05-W01 stays open. The ledger is unchanged. This review does not mark AT-FILE-01, AT-FILE-05, SP05-W01, or SP05-W06. It is not a customer deployment and not a production release.

## Source

| Item | Value |
|---|---|
| Fixed commit | `3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6` |
| Parent | `4e495b0df4e8ce6ebfa43052299dd490e10b9669` |
| Tree | `a13cf1b8a4e7390ebbd58384871675a5f271a4b9` |
| Subject | `control: lock local project quota and stream part publish` |
| Author evidence | `523951fd` |
| a183 evidence | `950406b8` |
| Reviewed tree | git archive of the fixed commit, then the overlay |

`source/blobs.txt` records the git blob and sha256 of the six product paths this run compared. Those bytes match `pre-edit/archive-match.txt` (`archive_hashes_match=1`) and `oracle/product-files-match.txt` (`product_files_match=1`). The oracle did not change `Cargo.lock` or those six files. Slot B owns `parser_worker.rs` and its bin, tests, and lib export. Slot C owns `executor.rs`, its live test, and its contract. This review did not edit them.

## Pre-overlay smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, Debian 13. Container `oo-a188-pre-edit`.

`pre-edit/docker-run.exit` is 0. `pre-edit/phases/worst.exit` is 0. `pre-edit/inspect.txt` says `ExitCode=0`, `OOMKilled=false`. `pre-edit/capacity.txt` recorded `running_rust_builders=0`, `own_rust_builder_running=0`, `host_cargo_rustc_rustdoc_rustup=0`. The snap did not contain `review_bounds.rs`.

`cargo test --locked --offline -p oo-control --test file_hosting --lib` reported 0 library tests and 10 integration tests passed in 10.40s. `cargo fmt --check -p oo-control` and `cargo clippy --all-targets -- -D warnings` exited 0. The lock file was unchanged. `pre-edit/console.log` ends with `pre_overlay_smoke=pass`.

## Overlay

`oracle/review_bounds.rs` is an independent safe-Rust integration test. It uses `oo_control`, `object_store`, `sha2`, and the pinned tokio dev-dependency (`macros`, `rt-multi-thread`). It does not use `unsafe`. The first oracle container failed `cargo fmt --check` before any test. `oracle/attempt1-fmt/docker-run.exit` is 1. `oracle/attempt1-fmt/phases/fmt-check.exit` is 1. The console diff is preserved there.

`rustfmt --edition 2024` then rewrote only that overlay inside the same pinned image. `oracle/rustfmt-apply.exit` is 0. A login-shell attempt that could not see `rustup` is preserved as `oracle/attempt1-fmt/rustfmt-login-shell.exit` (127). Product files in the snap still matched the fixed commit after that rewrite. The formatted overlay was copied back to `oracle/review_bounds.rs` and the snap copy was removed so the second launch could install it.

## Oracle

Same image and toolchain. Container `oo-a188-review`. Network `oo-a188-eef831aa-net` (`driver=bridge`, `internal=false`). Volumes `oo-a188-eef831aa-cargo` and `oo-a188-eef831aa-target`. `oracle/capacity.txt` recorded zero running Rust builders and zero host cargo, rustc, rustdoc, rustup, or clippy-driver processes. No PostgreSQL.

`oracle/hostconfig.txt`: privileged false, CapDrop ALL, no-new-privileges, no published ports, bridge network, memory and swap 8589934592, `/tmp` tmpfs `rw,nosuid,nodev,size=2g`. `HOME=/tmp`. The launch script does not mount the host home, the docker socket, or a shared cargo cache.

`oracle/docker-run.exit` is 0. `oracle/inspect.txt` says `ExitCode=0`, `OOMKilled=false`. Phase exits are 0 for rustfmt, clippy, fmt check, the overlay tests, clippy `-D warnings`, lock-after, product files, and worst. `oracle/console.log` ends with `review_oracle=pass`.

`cargo test --locked --offline -p oo-control --test review_bounds -- --nocapture --test-threads=1` finished in 43.73s: 6 passed, 0 failed.

| Test | Result |
|---|---|
| `quota_ten_eight_byte_reservations` | 8 begins, 1 ok, 7 `Quota`, reserved 8, abort 0 |
| `complete_part_writer_across_processes` | both completes equal, writer `Closed`, original bytes kept |
| `lock_lifetime_cancel_timeout_exit` | cancel 0 ms, timeout 30001 ms `Unavailable`, kill-to-begin 6 ms |
| `trusted_directory_lock_scope` | one reservation on the canonical directory |
| `generic_store_refuses_before_write` | `Unsupported`, store stayed empty |
| `bounded_publish_and_resident_complete` | 8192 and 8193 published; resident delta stayed under 2 MiB |

Raw lines are `oracle/notes/`. Clippy `--all-targets -D warnings` finished clean. `Cargo.lock` was unchanged.

## Quota, idempotency, and conflict

Limits were `HostLimits::try_new(64, 64, 8, 10)`. Eight begins of 8 bytes ran together: four `FileHost` tasks and four OS processes, one barrier. `oracle/notes/quota.txt`:

- `distinct_ok=1`, `distinct_quota=7`, `distinct_other=0`, `distinct_reserved=8`, `distinct_session_ids=1`
- `after_distinct_abort=0`
- same display, principal, and bytes: `same_ok=8`, `same_unique_sessions=1`, `same_reserved=8`
- a later begin with grant `grant-two` returned that session; the stored session JSON still contained `grant_evidence` `grant-one`
- abort, a new begin, abort, and a second abort: reserved went 8 then 0, and the second abort was `NotFound`

A wider directory, `try_new(64, 64, 8, 40)`, held two sessions. `put_part` of different bytes on the first part returned `Conflict`. The on-disk part stayed `aaaaaaaa`. Abort of that session left the other part and `staging/<session>/part/notes` in place, and the reservation went from 16 to 8 and then to 0.

## Complete and the part writer

Two processes called `complete` and one called `put_part` with a same-length replacement, after the parent had stored `original-bytes`. `oracle/notes/complete.txt`:

- both complete lines were equal and started with `ok`
- the writer line was `err Closed`
- object sha256 `8e338be649868e7c9e053212037e4bb0965ba7653e81b974bab76578bd59c80a`
- a second `complete` left the manifest file bytes unchanged
- the lineage digest matched an independent recomputation
- `staging/<session>/part/notes`, `meta/quota/projectc/notes`, and a file outside the store were unchanged

Host `hashlib.sha256(b"original-bytes")` is that same digest (`oracle/pattern-recompute.txt`). The cross-process writer observed `Closed` because a complete had already published the manifest. The different-bytes `Conflict` path is the quota note above: the stored part was not replaced.

## Lock lifetime and scope

`oracle/notes/lock.txt`:

- aborting a `begin` that was waiting on a held flock returned in 0 ms; the follow-up begin took 9 ms and reserved 8, then abort left 0
- a begin blocked by a live holder returned `Unavailable` at 30001 ms and left the store directory empty
- `SIGKILL` of the holder let the waiting begin finish in 6 ms, reserved 8, and a later begin of a different display also reserved 8

`oracle/notes/scope.txt`: four begins through the real directory, a symlink alias, a `../` path, and an alias path from another process produced `canonical_ok=1`, `canonical_quota=3`, `canonical_reserved=8`. The decoy `deploy-real.oo-file-hosting.lock` inside the store was unchanged. No `deploy-alias.oo-file-hosting.lock` appeared. A sibling directory with the same basename took its own reservation of 8 while the first store stayed at 8.

## Generic store and bounded publish

`FileHost::new` on `InMemory` returned `Unsupported` from `begin`, `put_part`, `complete`, and `abort`. The display string was `object store cannot enforce a linearizable project quota` and contained no `/`. `Debug` of the host contained neither `InMemory` nor `/`. `list_with_delimiter` on that same store returned no objects and no prefixes. `oracle/notes/generic.txt` says `unsupported_before_write=1` and `store_keys=0`.

Empty `put_part` returned `Limit` and created no part file. `complete` of 8192 bytes and of 8193 bytes published those exact lengths. The 1-byte tail was accepted.

Resident samples are from `/proc/self/status` in a fresh child, around `complete` only. The child planted the part with 8192-byte `std::fs` writes. The parent rehashed the published file. `oracle/notes/bounded.txt`:

| Sample | 8388608 bytes | 33554432 bytes |
|---|---|---|
| `memory_hwm_delta_bytes` | 143360 | 139264 |
| `memory_rss` before / after KiB | 5048 / 5188 | 5040 / 5176 |
| `memory_vmpeak_delta_bytes` | 2203648 | 2203648 |
| threads before / after | 3 / 4 | 3 / 4 |
| `object_sha_match` | 1 | 1 |
| `part_gone` | 1 | 1 |
| `object_size` | 8388608 | 33554432 |

Both resident deltas are under the 2 MiB gate in the overlay. The 32 MiB delta is smaller than the 8 MiB delta. Virtual peak is recorded and was not a pass gate. Host recomputation of the planted pattern matches `expected_sha` in both memory notes (`oracle/pattern-recompute.txt`). a183's comparable resident delta on an 8388608-byte part was 16801792. This review did not use the a186 resident figures as its result.

## Still open

S3, MinIO, HTTP, grant authentication, the reaper, `dest#N` leftovers, a crash between owned keys, and quota across machines that do not share one deployment directory stay open. `read_object` still fills one caller-sized buffer. `put_part` still receives the caller slice. Contract lines 109–123 list the rest of the open SP05-W01 work. The poisoned-wait sentence is discussed in `FINDINGS.md`. It has no measured counterexample in this run.

## Cleanup

`cleanup-done.txt` records the removal. Containers `oo-a188-pre-edit`, `oo-a188-review`, and `oo-a188-rustfmt` were already gone. Network `oo-a188-eef831aa-net` and volumes `oo-a188-eef831aa-cargo` and `oo-a188-eef831aa-target` were removed. `/tmp/oo-a188-eef831aa-dba4-4af2-a78b-541bc9e0a188` was removed. Other owners' networks, volumes, and containers were left in place. Nothing was pruned.
