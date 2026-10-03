# Findings

Subject `a90c86b405febebfdc70f8169be42dc4de4c3dfc`. Evidence run `linux/review-1/`. The review probe was not changed after that run. Line numbers below are that commit.

## F1 — major: reserve arguments are counted when capacity does not change

`ReadBuffer::reserve` (`buffered.rs` 513–524) computes `additional = amt - available.capacity()` and, when that is positive, calls `BytesMut::reserve(additional)` and adds `additional` to `reserved_additional`.

`BytesMut::reserve` (`bytes` 1.12.1 `bytes_mut.rs` 610–617) returns immediately when `additional <= capacity - len`. Its argument is spare beyond `len`, not growth beyond `capacity`. After the handshake and a 5-byte header, `available` capacity is 8172 and `len` is 0. The next deficit is 11995, so `additional` is 3823. Spare 8172 is already greater than 3823, so `reserve` does not change capacity. The meter still records 3823.

Reproduced with a 12000-byte `CommandComplete`, header first:

- Parked after 5 bytes: `reserved_additional` delta 3823, `reserve_calls` delta 1, `view_capacity_peak` 8192, event count still 2. The event is pushed only after the read returns (`buffered.rs` 492–502), so this count has no event.
- Resumed read, event 0: `reserve_argument=3823`, `available_capacity_before=8172`, `available_capacity_after=8172`, `bytes_returned=8172`.
- Event 1: `reserve_argument=3823`, `available_capacity_before=0`, `available_capacity_after=8192`, `bytes_returned=3823`.

Post-park `reserved_additional` delta is 7646. Fresh-event capacity growth is 8192. Lifetime counted arguments for this frame are `3823 + 7646 = 11469` against 8192 bytes of new available capacity. The cancelled future and the retry each count the same no-op.

The probe assertion `reserved > growth` (`pg_wire_bounds_review.rs` 733) is a bad oracle. 7646 is not greater than 8192 because the second call really did allocate 8192 bytes for a 3823 argument. The failure of that comparison does not erase event 0.

Rejected headers in the passing tests stayed at `reserved_additional` unchanged and `view_capacity_peak` 8192. The no-op shows up when the deficit is larger than the spare and at most twice the spare, which is this 12000-byte split, not the short rejects.

Minimal change: pass `BytesMut::reserve` the shortfall against `capacity - len`, and add to `reserved_additional` only `capacity_after - capacity_before`. If the future is dropped before the event is pushed, do not leave the counter ahead of the log.

## F2 — major: `assembly_bytes` stays 0 on the copy path

`assembly_bytes` is initialized to 0 (`buffered.rs` 76) and never increased. The field comment (`buffered.rs` 53–54) says the path does not make a copy. `PATCH.md` 128–131 says the receive path does not allocate a second copy of the frame.

Event 1 of the same 12000-byte frame has `available_capacity_before=0` and `read_len_before=8177`. The new buffer comes from `reserve_inner` abandoning the shared allocation (`bytes_mut.rs` 787–804). `advance` is `read.unsplit(available.split_to(amt))` (`buffered.rs` 526–528). `unsplit` copies when the views are not contiguous (`bytes_mut.rs` 964–972). Measured `assembly_bytes` is 0 and `view_capacity_after` is 20738.

That 20738 matches bytes' growth on this trace: double the 8192 shared vec to 16384, keep the 15-byte handshake offset out of the window (`16369`), and keep the unused tail of the new 8192 buffer after a 3823 split (`4369`). `16369 + 4369 = 20738`. The copy size is not reported anywhere on `PgIncomingAllocation`.

`rounding_peak` 4369 is `8192 - 3823`, from `buffered.rs` 466–468. It matches `max(requested, original_capacity)` in bytes, so it is real extra view capacity for this call. It is not `malloc_usable_size`. A later exact `with_capacity` can still report 0. This image did not report 0 for the 3823-from-zero reserve.

The 20011-byte one-shot later in the same test did not run. F2 does not depend on it.

Minimal change: keep the tail in the same allocation so `unsplit` stays O(1), or set `assembly_bytes` to the copied length. Correct `PATCH.md` 130–131 to match the path that remains.

## F3 — not a defect: disable during a legal partial header accepts a later negative length

`disable_during_partial_does_not_accept_corrupt_field` panicked at `pg_wire_bounds_review.rs:528` after printing:

```
REVIEW corrupt-after-disable accepted format=68 len=16 sync=true
```

Format 68 is `'D'`. Length 16 is the DataRow body. `incoming_synchronized()` stayed true.

Sequence: bounds were set with message cap 1000; a 5-byte header with `message_len` 20 was absorbed; `set_incoming_bounds(None)` ran; then natts 1, field length `-2`, and 10 payload bytes arrived. `inspect` (`stream.rs` 425–429) takes the historical branch when `bounds` is `None`, including for DataRow. `recv_backend` returns the raw frame and does not decode field lengths. `set_incoming_bounds` (`connection/mod.rs` 140–143) only replaces the option. It does not re-check the retained buffer by itself. The next `inspect` does, under the option now stored.

`PATCH.md` 104–108 says unset keeps the previous whole-frame path, and that field checks run when bounds are set. The same run's `bounds_change_during_partial_receive` passed: a valid row completes after disable, and an already buffered over-cap header is rejected if the cap is tightened or bounds are turned on before the decision. Poison is not cleared by the setter. The probe required a reject after the caller had turned the checks off. That requirement is not the contract.

No product change for F3.

## F4 — observation: historical unknown tag stays a Protocol error and stays synchronized

With bounds unset, tag `0x7f` and body `NOLEAK` produced `Error::Protocol` and `incoming_synchronized() == true`. The text was:

```
encountered unexpected or invalid data: unknown message type: '\u{7f}' (sqlx_postgres::message:173)
```

The body `NOLEAK` is absent. The type byte is in the historical string at `message/mod.rs` 173. With bounds set, the same tag is `IncomingWireError::BadType` at byte 5 and the display is `incoming wire type` (`error.rs` 77), with the fault stored (`stream.rs` 273–277). `PATCH.md` 116–117 describes both. `map_driver` still folds `IncomingWire` into fact `driver` (`oo-connectors/src/error.rs` 212). That mapping is outside this verdict.

## Passed checks that bound the defects

Short rejects, exact and plus-one caps, 1600 and 1601 columns, fragmented cancel and resume, poison, `ping`, setter not clearing a fault, and `close_hard` flushing queued `Sync` bytes `S\x00\x00\x00\x04` all passed in this run. See `REPORT.md` for the measured limits. Those passes do not clear F1 or F2.
