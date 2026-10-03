# Handoff

Verdict: **REQUEST_CHANGES**. Commit `a90c86b405febebfdc70f8169be42dc4de4c3dfc`, tree `6e7eb46c09470a8163a324063beef48c6254bbb1`, parent `6de125ab01df6564024005dcc74fbf9d0a96b392`. Reviewer `eef831aa-dba4-4af2-a78b-541bc9e0a122`. Limited R2-W is not accepted. The 87-item goal is not complete.

`linux/review-1/` is the only execution. `docker-run.exit` is 101 because `pg_wire_bounds_review` failed two tests. Library test 49, author `pg_wire_bounds` 13, both Clippy runs, fetch, and the seven blob checks are exit 0. Do not read the wrapper or the Clippy exit as approval.

## Defects to fix

F1. `buffered.rs` `ReadBuffer::reserve` counts `BytesMut::reserve` arguments that do not change capacity, including a count that never becomes an event if the read future is dropped while parked. Independent case: 12000-byte frame, 5 bytes then the body. See `FINDINGS.md`.

F2. `assembly_bytes` remains 0 and `PATCH.md` says the receive path does not copy, while `advance` copies when the new available buffer is not contiguous with `read`. Same trace: `available_capacity_before=0`, `view_capacity_after=20738`, `assembly_bytes=0`.

Minimal proposal, not applied:

- Reserve against spare (`capacity - len`), and record reserved bytes only when `available.capacity()` grows.
- Drop the counter if the future is dropped before the event is pushed.
- Stop the copy, or add the copied length to `assembly_bytes`, and make `PATCH.md` 130–131 match the code.

`rounding_peak` 4369 on this trace is real extra view capacity (`8192 - 3823` from bytes' original-capacity floor). Do not force it to 0.

## Do not treat as defects

F3. After a legal 5-byte header, `set_incoming_bounds(None)` accepts a later DataRow whose field length is `-2`. Measured `format=68`, `len=16`, `synchronized=true`. Unset is the historical parser (`stream.rs` 425–429, `PATCH.md` 104–108). The probe panic at `pg_wire_bounds_review.rs:528` is the wrong oracle. Tightening or enabling bounds on an already buffered illegal header still rejects; that test passed.

F4. Unset unknown tag `0x7f` remains `Error::Protocol` with the type byte and `message/mod.rs:173` in the string, and it does not set the fault. The body is not in the string. Bounded mode uses `BadType` and does set the fault.

The assertion `reserved > growth` in the accounting test is also a wrong oracle. The event lines above it are the evidence.

## Open, not this verdict

- `map_driver` (`crates/oo-connectors/src/error.rs` 212) maps `IncomingWire` to fact `driver`. Catalog and probe opt-in are later tasks.
- `executor.rs` 254–271 still sends Execute limit 0 and an unnamed Close. Named portals, paging, Arrow, decode, and a full reader are not in this hunk.
- Client bounds are not a backend heap cap. `docs/10-security.md` TB7 is unchanged.
- Unexecuted on this arm64 run: `stream.rs` 398–403 (`usize` overflow and the `expected != frame_u32` check), 448–449, 454, 503, 506, and a `u32` row-sum overflow. The 20011-byte one-shot and the RSS sampler inside the accounting test did not run, because the bad comparison panicked first.
- No dual-arch, supply, release, or whole-SP03 claim.

## Evidence

`REPORT.md`, `FINDINGS.md`, this file, `product-blobs.sha256`, `probe/pg_wire_bounds_review.rs`, and `linux/review-1/` (console, invocation, hostconfig, phase exits, launch and inside scripts). `hashes.sha256` is a `sha256sum` manifest of those files and does not list itself.

Cleanup recorded in `linux/review-1/cleanup.txt`: `docker rm` 0, `docker network rm` 0, `docker volume rm` 0, `/tmp/oo-a122-eef831aa` removed. Afterward those names were absent. No prune, no foreign containers, no push.
