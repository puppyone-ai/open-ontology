# Handoff

Independent rereview of `ff783ac933b86e0ff068181afe0258764c5d2e5d`, tree `3a5554c7e8ff09555924675b60cb287155398caf`, parent `1150151bf82a1c72997b1c3aaacb306d47e16cd0`. Reviewer `eef831aa-dba4-4af2-a78b-541bc9e0a126`.

Verdict: **APPROVE LIMITED R2-W**. The opt-in driver slice on that commit meets the a122 meter requirements inside the live-view scope `PATCH.md` and `connection/mod.rs` already state. This file is the review verdict. It is not an acceptance checkbox. The 87-item goal is unchanged and not complete. No product file was edited.

The tree under test is a `git archive` of `ff783ac9`. Workspace `HEAD` `fdcb7f00305b66235fc331b8f2583e5e0679958b` was not tested.

## Linkage

Author evidence `294e3a12aab4b0ccbfb5ff6f669a6123585ca6ec`, parent `ff783ac9`, 56 evidence files. Its `hashes.sha256` has 55 entries. `shasum -a 256 -c` against that tree exited 0 on this rereview.

The six repaired paths are the only product-path delta from `57f6003239ceb375722c5b67aa36647a0a2a1e7f` to `ff783ac9` inside `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates/`, `.cargo/`, and `contracts/source/pgsql/empty-catalog.json`. One commit in that range touches `crates/`: `ff783ac9`. `sqlx-postgres` `src/lib.rs` blob `a6ce2f3a54f6712249030dacb38a7fceaa9722eb` is the same at both commits. `linux/smoke/snapshot-files.sha256` matches a fresh `git archive` of those six files (`shasum -a 256 -c` exit 0). Details are in `linkage/summary.txt`.

Both author containers recorded source head `57f60032`. The author started the baseline and edited the working tree while that container was still fetching. Keep that timing. The sealed baseline archive has no `pg_wire_accounting.rs`. It is not a smoke that finished before the repair, and it did not run `pg_wire_bounds` or `pg_wire_accounting`. Fetch, library 49, and Clippy on that archive exited 0. No cargo phase in the author logs exited non-zero. a122 `linux/review-1` was not rewritten. Its 12-pass/2-fail probe, including the `7646 > 8192` comparison, stays the historical wrong oracle.

## Runs

Smoke (`linux/smoke/`) is the archive with no probe. It finished before the independent tests were written. Library 49 passed. `cargo clippy -p oo-connectors --all-targets -- -D warnings` exited 0. Docker exit 0. Container seconds 41.

review-1 then ran library 49, `pg_wire_bounds` 13 (6.07s), and `pg_wire_accounting` 12 (4.59s), all exit 0. `pg_wire_accounting_review` exited 101: 11 passed, 1 failed, 3.84s. The failure is `unset_large_frame_is_accepted` panicking in `assert_reserve_exceeds_spare` at `./tests/pg_wire_accounting_review.rs:505` because the probe required spare `> 0`. The frame was already accepted. The event is `reserve_argument=3823`, `available_capacity_before=0`, `read_len_before=8177`, `reserved_growth=8192`, `assembly_bytes=0`. `3823 = 12000 - 8177`. That is a real deficit past `read.len()`, not the old no-op (a 3823 argument against spare 8172). Clippy of the review test exited 0. Docker exit 101. This attempt is preserved.

review-2 is the same phases after that spare requirement was removed. Library 49, bounds 13 (6.06s), accounting 12 (4.59s), review 12 (3.90s), review-test Clippy, and the product-byte check all exited 0. Docker exit 0. Container seconds 17. Scoped all-targets Clippy was not repeated after smoke.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, local inspect `os=linux arch=arm64`, `--pull never`. rustc 1.94.0 (`4a4ef493e` 2026-03-02), host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, cargo 1.94.0. `rust-toolchain.toml` asks for 1.98.1; containers set `RUSTUP_TOOLCHAIN=1.94.0` and refuse any other rustc. `airgap_claim=no`. Smoke fetch filled the owned cargo volume. Later fetches were `--locked --offline`.

HostConfig on every container: privileged false, CapDrop ALL, no-new-privileges, memory and swap 8589934592, NanoCpus 4000000000, PidsLimit 4096, OOMKilled false, bridge `oo-a126-eef831aa-net`, `/src` read-only. Capacity before each start: running rust builders 0, own builders 0, host cargo/rustc/rustdoc/rustup 0. `eef831aa-dba4-4af2-a78b-541bc9e0a125-pg` was up and was not used. No Postgres server was part of these runs. The peer is scripted TCP.

## What review-2 measured

Header-first 12000 `CommandComplete`: parked `bytes_read=20`, `reserve_arguments=11995`, `reserved_growth=8192`, `rounding=4369`, `view_capacity_peak=16369`, `assembly=0`. The reserve row is stored before the body. The retry reaches `bytes_read=12015` and does not add growth. A 100-byte partial body keeps the same 8192. EOF after the header is `UnexpectedEof` (`expected to read 12000 bytes, got 5 bytes at EOF`), reserve row `read_completed=1`, `bytes_returned=0`, growth kept.

One-shot 20011 `CommandComplete`: argument = growth = 11834, `assembly=8177`, `read_len_before=8177`, rounding 0. One-shot 20011 `DataRow`: the same numbers with `assembly=0`. The author's final log had the opposite pair. review-1 unset-large had `assembly=0`; review-2 unset-large had `assembly=8177`. Both are at most `read_len_before`. The message type does not decide the copy.

After `shrink_buffers`, the body copy is counted once: `copied=11995`, `grew=8192`, `calls=2`, `reserved_growth=16384`, `reserve_arguments=23990`, `assembly=11995`. The `available.len() != 0` arm in `prepare_window` was not entered.

Twenty-four retained 1000-byte frames: wire 24120, `view_capacity_peak=8192`, `accepted_backing_peak=8192`, `reserved_additional=16095`, `reserve_argument_bytes=1721`, `assembly=289`, `rounding=7187`. The sum of view capacities is one window. It is not the retained storage.

Past the 64-event cap: stored rows stay 64, `trace_bytes` stays 6656, `events_dropped` goes 1 then 3, and the 12000 frame still adds `reserved_growth=8192`, `reserve_arguments=4207`, `assembly=7793`. Stored rows sum to 0 growth. Counters hold the full history.

Huge length 2000000000, message cap 1000, header only: `bytes_read=20`, no reserve, `last_logical=5`. A following `NOLEAK` frame is not read and does not appear in the error text. Short length poisons `recv`, `ping`, and both setters. `close_hard` flushes `S\x00\x00\x00\x04`. A second connection reads `ok`. A split small frame then `NEXT` delivers both payloads with `reserved_growth=0` and `noop_calls=0`. Unset mode accepts the 12000 frame. F3 and F4 remain non-defects.

`noop_reserve_calls` stayed 0 on every trace. A positive no-op `BytesMut::reserve` is unreachable from `prepare_window`: the function calls `reserve` only when `deficit > spare`, and both call sites pass that same deficit. The `(0, 0)` return is a skipped call. It was observed on small frames (`reserve_calls=0`). It was not recorded as a no-op call.

## Limits of the meter

`reserved_additional`, `view_capacity_peak`, and `accepted_backing_peak` are live-view sums or the sum of increases of that sum. They omit the offset in front of both windows, they can share one allocation, and they omit frames the caller still holds. `rounding_peak` is spare above the reserve argument after a call that grew. It is not `malloc_usable_size`.

`assembly_bytes` counts the live prefix when `as_ptr` changes and counts one `extend_from_slice` when `try_unsplit` fails. A pointer change does not prove the allocator copied pages. `realloc` may remap. The meter still adds `old` in that case, and it does not add the abandoned offset. Do not treat 8177, 0, or 289 as a physical copy size. RSS was not sampled.

On this 64-bit target the `usize` overflow arms in `inspect` and `inspect_data_row` do not run. A `u32` row-sum overflow needs a multi-gigabyte body and was not sent.

Client message caps still reject one declared body before that body is reserved. They do not bound caller-retained frames, pinned shared allocations, or backend `palloc`. If a later task needs a resident cap, this meter is not that cap. No code change is proposed. An optional sentence in `PATCH.md` around line 144 could say that a pointer change counts the live prefix as relocated and that `realloc` may move a mapping without copying pages. The offset exclusion is already there. That sentence is not required for this verdict.

## Still open

- `map_driver` still folds `IncomingWire` into fact `driver`.
- Catalog and probe production opt-in.
- Named portals, Execute limit 0, unnamed Close, same-cut Arrow streaming, and a full reader.
- Backend heap. `docs/10-security.md` TB7 is unchanged.
- R1 and the schema guard. Slot A `a125` was live and was not used.
- Whole reader, SP03, dual-arch, supply, and release.
- The 87-item goal.

## Cleanup

Removed `oo-a126-msrv-smoke`, `oo-a126-msrv-review`, `oo-a126-msrv-review-2`, network `oo-a126-eef831aa-net`, volumes `oo-a126-eef831aa-cargo` and `oo-a126-eef831aa-target`, and `/tmp/oo-a126-eef831aa` plus `/tmp/oo-a126-link`. Afterwards those names were absent. `eef831aa-dba4-4af2-a78b-541bc9e0a125-pg` was still up. No `docker prune`. Commands are in `cleanup.txt`.

## Evidence

`REPORT.md`, `FINDINGS.md`, this file, `cleanup.txt`, `linkage/`, `probe/pg_wire_accounting_review.rs`, and `linux/smoke/`, `linux/review-1/`, `linux/review-2/` (console, invocation, hostconfig, image, phase exits including the 101, launch and inside scripts, snapshot hashes). `hashes.sha256` lists those files and does not list itself.
