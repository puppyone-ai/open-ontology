# Handoff

Author delivery for rereview. Commit `ff783ac933b86e0ff068181afe0258764c5d2e5d`, tree `3a5554c7e8ff09555924675b60cb287155398caf`, parent `1150151bf82a1c72997b1c3aaacb306d47e16cd0`. This verdict is not independent acceptance. Limited R2-W is not claimed.

The repair commit touches six paths:

- `crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs`
- `crates/oo-connectors/vendor/sqlx-postgres/src/connection/stream.rs`
- `crates/oo-connectors/vendor/sqlx-postgres/src/connection/mod.rs` (doc sentence on `incoming_allocation`)
- `crates/oo-connectors/vendor/PATCH.md`
- `crates/oo-connectors/tests/pg_wire_bounds.rs` (accounting assertions only)
- `crates/oo-connectors/tests/pg_wire_accounting.rs` (new)

`sqlx-postgres` `src/lib.rs` is not in the diff. Blob `a6ce2f3a54f6712249030dacb38a7fceaa9722eb`.

## What the reviewer should be able to check

F1. On a 12000-byte frame, header first, the parked meter in `FINDINGS.md` has `reserve_argument=11995` and `reserved_growth=8192` while `bytes_read` is still 20. The incomplete event remains after the body is read, and the retry does not add growth.

F2. `assembly_bytes` is 8177 on the bounds 20011 DataRow and 0 on the accounting 20011 CommandComplete, with `read_len_before=8177` and `bytes_returned=11834` on that second run. The retained-frame run copies 289 bytes while holding 24000 content bytes. `PATCH.md` no longer says the receive path allocates no second copy.

`rounding_peak` 4369 on the 12000 trace is spare above the 11995 argument. The 20011 runs recorded 0 because `available_capacity_after` equalled 11834. Neither value was forced.

`view_capacity_peak` and `accepted_backing_peak` are `read.capacity() + available.capacity()`. On 24 retained frames those peaks stay 8192 while `reserved_additional` is 16095 and the retained wire length is 24120. 16095 is cumulative live-window growth. 8192 is one window. Neither is RSS, and neither is the sum of retained frames.

## Runs to trust, and the overlap

`linux/final/` is the run of the committed bytes. Docker exit 0. Library 49, `pg_wire_bounds` 13, `pg_wire_accounting` 12, Clippy 0. Container seconds 23. Toolchain rustc 1.94.0 host `aarch64-unknown-linux-gnu`. Image sha256 `0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, arch arm64. `snapshot-files.sha256` matched inside the container before the build and matched the workspace after it. That workspace is the commit.

`linux/baseline/` is the sealed archive of `57f60032`, which has no `pg_wire_accounting.rs`. Fetch, library 49, and Clippy exited 0 in 41s. Edits to the working tree started while that container was still fetching. Do not read the baseline as a completed smoke that happened before the repair, and do not read it as coverage of `pg_wire_bounds` or the new accounting file.

No cargo phase in either run exited non-zero. No new RED log was produced. a122 `linux/review-1` remains the historical 12-pass/2-fail probe, wrong comparison included. It was not rewritten.

## Still open

- Independent review of this commit. Until that review is accepted, R2-W stays closed.
- `map_driver` in `crates/oo-connectors/src/error.rs` still maps `IncomingWire` to fact `driver`.
- Catalog and probe production opt-in.
- `executor.rs` still sends Execute limit 0 and an unnamed Close. Named portals and a full reader are open.
- R1, the schema fix (slot A `a125` was live and was not touched), and a backend heap cap. `docs/10-security.md` TB7 is unchanged.
- F3 and F4 from a122 are still not defects. Unset after a legal partial header, and unset unknown tag `0x7f`, were not recast as failures.
- Unexecuted branches listed in `FINDINGS.md`: positive no-op reserve, EOF after reserve, a growth row past the 64-event cap, `grow_available` / failed `try_unsplit`, a large frame with bounds unset, the 64-bit overflow arms in `inspect`, vendored Clippy as its own package, dual-arch, supply, and RSS.
- Client bounds are not a backend heap cap.

## Cleanup

Removed `oo-a124-msrv-final`, `oo-a124-msrv-baseline`, network `oo-a124-eef831aa-net`, volumes `oo-a124-eef831aa-cargo` and `oo-a124-eef831aa-target`, and `/tmp/oo-a124-eef831aa`. Afterwards those names were absent. `eef831aa-dba4-4af2-a78b-541bc9e0a125-pg` was still up. No `docker prune`. Commands are in `cleanup.txt`.

## Evidence

`REPORT.md`, `FINDINGS.md`, this file, `cleanup.txt`, and `linux/baseline/` plus `linux/final/` (console, invocation, hostconfig, image, phase exits, launch and inside scripts, `snapshot-files.sha256`). `hashes.sha256` lists those files and does not list itself.
