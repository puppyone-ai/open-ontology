# R2-W accounting rereview

Verdict: **APPROVE LIMITED R2-W**. The opt-in driver slice on `ff783ac933b86e0ff068181afe0258764c5d2e5d` does what a122 required of the meter, within the scope the code and `PATCH.md` now state. This is not a checklist tick, not a full reader, and not a resident-set or backend-heap cap. The 87-item goal is unchanged and not complete.

Reviewer `eef831aa-dba4-4af2-a78b-541bc9e0a126`. No product edit. Workspace `HEAD` `fdcb7f00305b66235fc331b8f2583e5e0679958b` was not the tree under test.

## Linkage

Fixed commit `ff783ac933b86e0ff068181afe0258764c5d2e5d`, tree `3a5554c7e8ff09555924675b60cb287155398caf`, parent `1150151bf82a1c72997b1c3aaacb306d47e16cd0`. Six paths against that parent. `sqlx-postgres` `src/lib.rs` blob `a6ce2f3a54f6712249030dacb38a7fceaa9722eb` is the same blob as at `57f60032`.

Author evidence commit `294e3a12aab4b0ccbfb5ff6f669a6123585ca6ec`, parent `ff783ac9`, 56 files, all under the a124 evidence directory. Its `hashes.sha256` has 55 entries. `shasum -a 256 -c` of that manifest against the tree of `294e3a12` exited 0.

`linux/final/snapshot-files.sha256` from that commit matches a `git archive` of the six `ff783ac9` files (`shasum -a 256 -c` exit 0). Replacing those six paths in a `git archive` of `57f6003239ceb375722c5b67aa36647a0a2a1e7f` with the `ff783ac9` bytes produces the same `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates/`, `.cargo/`, and `contracts/source/pgsql/empty-catalog.json` as `ff783ac9`. Those are the only paths inside that set that differ between `57f60032` and `ff783ac9`. The only commit in that range that touches `crates/` is `ff783ac9`.

| Path | Git blob | sha256 |
| --- | --- | --- |
| `crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs` | `f41e0e4dd6baca062fb332ab66da3f5e9c0600fd` | `24b22aa55ecfd1f739fa87a0e95b86bd3f4ab7d4f26305fd51eda85f73476687` |
| `crates/oo-connectors/vendor/sqlx-postgres/src/connection/stream.rs` | `2089b5d675e2d8811334d187f8675407ce7a8d89` | `583c0aa7ffba55726f9c74a4d2275966c5e8d5444ad076dfcb48259a11a96981` |
| `crates/oo-connectors/vendor/sqlx-postgres/src/connection/mod.rs` | `4c2bb3cb45ce330fdac4c0a37294c96644fe41df` | `a2057da12adbf73d83e20282bdcea21ce373d1b0c00f63595ace2124710afdf5` |
| `crates/oo-connectors/vendor/PATCH.md` | `fb933d000c88289075a0a81d634780159f4e6743` | `24ea8ccbc3e3596b76891401bffecbd2679ca7ebaff949a2218dc5454c4c3720` |
| `crates/oo-connectors/tests/pg_wire_bounds.rs` | `c27e7df9ff875ea7022c1bfa9cebceea3caed0fd` | `d234aa8b51caf746a5c1f4de85fbd560e9652af7a6b7e5badfdefbd6bf0697d8` |
| `crates/oo-connectors/tests/pg_wire_accounting.rs` | `93b2f85ff041dfec638a16656e94b200c76d2251` | `404c6a7b9cb4579aeb6e1ce27dc2696fcfef144109201bc15111e6103a5595cb` |

Both author containers recorded source head `57f60032`. The author report says working-tree edits started while the baseline container was still fetching. That qualification stands. The baseline archive did not contain `pg_wire_accounting.rs` (`snapshot.exit` 0 on a script that exits 2 if the file is present). It is not a smoke that finished before the repair, and it did not run `pg_wire_bounds` or `pg_wire_accounting`. Fetch, library 49, and Clippy on that sealed archive exited 0. No cargo phase in the author logs exited non-zero. a122 `linux/review-1` was not rewritten. Its 12-pass/2-fail probe and the `7646 > 8192` comparison remain the historical wrong oracle.

## Runs

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, local inspect `os=linux arch=arm64`, `--pull never`. Bridge `oo-a126-eef831aa-net`, volumes `oo-a126-eef831aa-cargo` and `oo-a126-eef831aa-target`. HostConfig on every container: privileged false, CapDrop ALL, no-new-privileges, memory and swap 8589934592, NanoCpus 4000000000, PidsLimit 4096, OOMKilled false, no host network, no socket mount, `/src` read-only, HOME not mounted. Capacity before each start: running rust builders 0, own builders 0, host cargo/rustc/rustdoc/rustup 0. `eef831aa-dba4-4af2-a78b-541bc9e0a125-pg` was up and was not used.

`rust-toolchain.toml` requests channel 1.98.1. Containers set `RUSTUP_TOOLCHAIN=1.94.0` and refuse any other rustc. Recorded toolchain: rustc 1.94.0 (`4a4ef493e` 2026-03-02), host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, cargo 1.94.0 (`85eff7c80` 2026-01-15). `airgap_claim=no`. Smoke fetch updated the crates.io index into the empty cargo volume. Later fetches were `--locked --offline`.

Smoke (`linux/smoke/`) is a `git archive` of `ff783ac9` with no probe. It finished before the independent tests were written.

| Phase | Smoke | review-1 | review-2 |
| --- | --- | --- | --- |
| toolchain | 0 | 0 | 0 |
| snapshot | 0 | 0 | 0 |
| clippy component | 0 | 0 | 0 |
| fetch | 0 (`--locked`) | 0 (`--offline`) | 0 (`--offline`) |
| `cargo test -p oo-connectors --lib` | 0, 49 passed, 0.00s | 0, 49 passed, 0.00s | 0, 49 passed, 0.00s |
| scoped `cargo clippy -p oo-connectors --all-targets -- -D warnings` | 0 | not rerun | not rerun |
| `pg_wire_bounds` | not run | 0, 13 passed, 6.07s | 0, 13 passed, 6.06s |
| `pg_wire_accounting` | not run | 0, 12 passed, 4.59s | 0, 12 passed, 4.59s |
| `pg_wire_accounting_review` | not written yet | 101, 11 passed, 1 failed, 3.84s | 0, 12 passed, 3.90s |
| clippy of the review test | n/a | 0 | 0 |
| product bytes after the run | n/a | 0 | 0 |
| docker run | 0 | 101 | 0 |

Smoke container seconds 41. review-1 seconds 20. review-2 seconds 17. The review-1 failure is a bad assertion in the probe (`spare > 0`). The frame had already been accepted. The event was `reserve_argument=3823`, `available_capacity_before=0`, `reserved_growth=8192`. review-2 drops that spare requirement and passes. Details are in `FINDINGS.md`.

## What this approval covers

On the scripted TCP peer, with bounds set or left unset:

- A 12000-byte header-first `CommandComplete` records `reserve_argument=11995` and `reserved_growth=8192` while `bytes_read` is still 20. The incomplete event stays after the body arrives. The retry does not add growth.
- Dropping that parked read, a later EOF on the same reserve, and a 100-byte partial body all keep the 8192 growth.
- One-shot 20011-byte `CommandComplete` and `DataRow` each record argument 11834 and growth 11834. `assembly_bytes` is either 0 or 8177, which is `read_len_before`, and is below `bytes_returned` 11834.
- After `shrink_buffers` drops the spare, the following body is copied once: `assembly` delta 11995 on a 11995-byte body, with a second reserve of 8192. The body is not counted twice.
- Twenty-four retained 1000-byte frames sum to 24120 wire bytes. `view_capacity_peak` and `accepted_backing_peak` stay 8192. `reserved_additional` is 16095. None of those is the retained sum.
- Past the 64-event cap, a 12000-byte frame still moves `reserved_additional` by 8192 and `reserve_argument_bytes` by 4207 while the stored rows stay at 64 and `trace_bytes` stays 6656.
- A declared length of 2000000000 is rejected at 5 bytes with no reserve. Short length poisons `recv`, `ping`, and both setters. `close_hard` flushes `S\x00\x00\x00\x04`. A second connection still reads a small frame. Unset mode accepts a 12000-byte frame.

`noop_reserve_calls` stayed 0 on every trace. That branch was not exercised. Source-level unreachability is in `FINDINGS.md`.

## What the numbers are not

`reserved_additional`, `view_capacity_peak`, and `accepted_backing_peak` are sums of the two live `BytesMut` windows, or the sum of increases of that sum. They omit the offset in front of both windows, they can share one allocation, and they do not include frames the caller still holds. `rounding_peak` is spare above the reserve argument after a call that grew. It is not `malloc_usable_size`. `assembly_bytes` counts a live prefix when `as_ptr` changes and counts an explicit `extend_from_slice` when `try_unsplit` fails. A pointer change does not prove the allocator copied pages rather than remapping them, and it does not include the abandoned offset the `Vec` may copy. RSS was not sampled.

No positive no-op reserve was observed. The `available.len() != 0` arm in `prepare_window` was not observed. The `usize` overflow arms in `inspect` do not run on this 64-bit target. A `u32` row-sum overflow was not sent; reaching it would buffer on the order of gigabytes.

Client bounds are still not a backend heap cap. Catalog and probe production opt-in, `IncomingWire` mapped to fact `driver`, named portals, same-cut Arrow streaming, R1, the whole reader, SP03, dual-arch, supply, and release stay open.
