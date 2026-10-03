# Findings

Author measurements from `linux/final/console.log` on commit `ff783ac933b86e0ff068181afe0258764c5d2e5d`. The peer is a scripted TCP backend. No Postgres server. No RSS sampler.

## Contract that the code now implements

`BytesMut::reserve(n)` guarantees `capacity - len >= n`. `n` is additional bytes beyond `len`. When `deficit <= spare`, the buffer does not call `reserve`. The argument passed when it does call is `deficit`, not `deficit` minus spare and not `deficit` minus `capacity`.

`reserved_additional` adds the change in `read.capacity() + available.capacity()`. That sum is the two live windows. It can share one allocation, it omits bytes already advanced past, and it omits frames the caller still holds. `reserve_argument_bytes` is the sum of arguments, including a no-op if one occurs. `noop_reserve_arguments` is only the arguments whose capacity did not change. This suite recorded `noop_reserve_calls=0` on every trace: the calls that would have been no-ops under the old shortfall formula were not issued.

`rounding_peak` is the largest `spare_after - argument` among calls that grew, with spare measured as `available.capacity() - available.len()`. Zero means that spare equalled the argument. It is not `malloc_usable_size`.

`assembly_bytes` adds the live prefix length when `reserve` changes `as_ptr`, and the appended length when `try_unsplit` fails. A contiguous `unsplit` adds nothing. An abandoned offset in front of both windows is not in either view and is not added. In-place growth that keeps the pointer adds nothing. The field is allowed to be zero when that is what happened.

The diagnostic log stores at most 64 events and then increments `events_dropped` without deleting earlier rows. The row is pushed before `socket.read` is awaited. Dropping that future leaves the growth and any copy already performed on the counters and on the row, with `read_completed=0` and `bytes_returned=0`. A later read appends its own row.

`stream.rs` `inspect` is unchanged. `connection/mod.rs` changes the `incoming_allocation` doc sentence. `lib.rs` is unchanged.

## F1, from the 12000-byte header-first traces

`header_first_12000_trace`, `reserve_argument_is_distinct_from_growth`, `parked_read_drop_keeps_growth_and_reconciles`, and `fragmented_growth_survives_cancel` each use one `CommandComplete` of 12000 bytes (body 11995). Bounds are `100_000/0/0/0`.

Parked after the 5-byte header (`parked-drop`):

```
logical_peak=12000 reserved_growth=8192 reserve_arguments=11995
reserve_calls=1 noop_calls=0 view_capacity_peak=16369
reads=2 bytes_read=20 assembly=0 accepted_logical=5
accepted_backing=8192 overread=10 rounding=4369
events=3 dropped=0
```

`bytes_read=20` is the 15-byte handshake plus the header. The body has not been read. Growth 8192 is already on the counter, and the suite requires an event with `read_completed=0`, `reserved_growth>0`, and `bytes_returned=0`.

The growth event's own fields (`argument-vs-growth`):

```
argument=11995 growth=8192 spare_before=8172 deficit=11995
view_before=8177 view_after=16369
```

`11995 > 8172`, so the argument is the deficit and is larger than the spare. `8192 != 11995`. `view_after - view_before = 8192`. Rounding 4369 is the spare left above that 11995 argument after the bytes 1.12.1 doubling (view sum 16369, which is 16384 minus the 15-byte handshake offset that sits outside both windows).

After the body (`parked-retry` and `header-first-12000`):

```
reserved_growth=8192 reserve_arguments=11995 reserve_calls=1
view_capacity_peak=16369 reads=3 bytes_read=12015
assembly=0 accepted_logical=12000 accepted_backing=16369
overread=10 rounding=4369 events=4 dropped=0
```

`bytes_read` 12015 is 15 plus 12000. `reserved_additional` does not increase on the retry. `accepted_backing_peak=16369` is the live-window sum at accept, spare included. It is not a resident-set size.

The fragmented run wrote the header, dropped the future, wrote 100 body bytes, dropped again, then wrote the rest. After the 100-byte piece, `bytes_read=120` and `reserved_growth` was still 8192. After the frame completed, `bytes_read=12015` and `reserved_growth` was still 8192. The cancelled row stayed `read_completed=0`.

`reconcile` on these traces, while `events_dropped=0`, matched `reserved_additional`, `assembly_bytes`, `reserve_argument_bytes`, `noop_reserve_arguments`, `reserve_calls`, `noop_reserve_calls`, `bytes_read`, and `rounding_peak` to the event rows.

## F2, copy counts from two 20011-byte deliveries

`one_shot_20011_trace` and `unsplit_copy_does_not_duplicate_the_body` send one `CommandComplete` of 20011 bytes before `recv`.

```
argument=11834 growth=11834 assembly=0
read_len_before=8177 bytes_returned=11834
avail_before=0 avail_after=11834 rounding=0
logical_peak=20011 view_capacity_peak=20011
bytes_read=20026 accepted_logical=20011 accepted_backing=20011
overread=8182 reserve_calls=1
```

`assembly_bytes=0` with `read_len_before=8177` means the pointer did not move. The new 11834 bytes were not added to the copy count. The test accepts either 0 or exactly `read_len_before`, and rejects a count that includes `bytes_returned` on top of the prefix.

`pg_wire_bounds` `allocator_rounding_and_retained_storage` is a 20011-byte DataRow on a different connection:

```
logical_peak=20011 reserved_growth=11834 view_capacity_peak=20011
reads=3 bytes_read=20026 assembly=8177 accepted_logical=20011
overread=8182 rounding=0 reserve_calls=1 accepted_backing=20011
```

`assembly_bytes=8177` is the same prefix length the CommandComplete event reported as `read_len_before`. The suite checks that this equals the sum of the event rows and is `<= bytes_read`. 8177 is below 20011. The two runs disagree because one allocation moved and the other did not. Both numbers were left as measured.

`retained_frames_are_not_summed_into_backing` keeps 24 `CommandComplete` frames of 1000 content bytes:

```
retained_logical=24120 bodies=24000
accepted_logical=24005 accepted_backing=8192 view_peak=8192
reserved_growth=16095 reserve_arguments=1721 reserve_calls=2
assembly=289 rounding=7187 reads=27 bytes_read=24135
events=27 dropped=0
```

`accepted_logical_payload` 24005 is 24000 plus the handshake contents (AuthenticationOk 4 bytes, ReadyForQuery 1 byte). `accepted_backing_peak` 8192 is below that sum and below half of the retained wire length. `assembly_bytes=289` is the prefixes copied when retained frames kept the old allocation shared. It is not 24000 and not 24120. `reserved_additional` 16095 is larger than `reserve_argument_bytes` 1721 because the original-capacity floor grew the new window by more than the argument. `rounding_peak` 7187 is that slack. `view_capacity_peak` stayed 8192, so the cumulative growth is not the high-water of the live windows and is not the sum of retained frames.

## Other accounting tests that passed

`noop_request_is_zero_growth`: a 7-byte `CommandComplete` (`b"ok"`). `reserved_growth=0`, `reserve_arguments=0`, `noop_calls=0`, `reserve_calls=0`, `assembly=0`, `view_capacity_peak=8192`.

`event_log_is_bounded_and_counters_continue`: small `CommandComplete` frames until the log drops a row, then three more.

```
event-cap:       reads=65 bytes_read=399 messages=66 events=64 dropped=1 trace_bytes=6656
event-cap-later: reads=68 bytes_read=417 messages=69 events=64 dropped=4 trace_bytes=6656
```

6656 is 64 times 104, and 104 is the size of the 13-field event. The first event's `logical_need`, `bytes_returned`, and `read_completed` still matched the connect-time row. `bytes_read` rose by 18 and `messages` by 3. These dropped rows were zero-growth reads. A growth row past the cap was not produced.

`huge_prefix_reject_does_not_reserve_body`: header only, declared length 2000000000, message cap 1000.

```
bytes_read=20 reserved_growth=0 reserve_arguments=0 reserve_calls=0
last_logical=5 logical_peak=9 view_capacity_peak=8192
```

The second `recv` returned the same fault and did not change `bytes_read` or `reserved_additional`.

`fragment_cancel_keeps_bytes_and_order`: 4-byte prefix of `CommandComplete` body `ORDER-BODY`, drop, remainder, then a second frame `NEXT`. `bytes_read` rose by 4 on the drop and by the frame length after resume. Contents matched. `reserved_growth=0` because the frame fits in the initial spare.

`poison_stops_reads_without_clearing_on_unset`: length field 3, `ShortLength`. `bytes_read` rose by 5. A following frame and `set_incoming_bounds(None)` both return `ShortLength` again. `bytes_read`, `reserved_additional`, `reserve_calls`, `reads`, and `events.len()` stay put. `incoming_synchronized` stays false.

## pg_wire_bounds, 13 passed

The previous author file still passes, including message, value, and row caps at the limit and one past it, 1600 and 1601 columns, null/zero/negative/truncated/remaining/overflow, control-message huge prefixes at tags 78, 69, 83, 68, and 255, bad type, default-off accept and short length, cancel at offsets 4, 6, 10, and 14 with bounds and at 4 and 14 without, and the poison path that pings, clears bounds, widens bounds, and still does not read further. Small rejects in that file keep `assembly_bytes=0` and `reserved_additional` unchanged. The 20kb assertion no longer requires `assembly_bytes=0`, `rounding_peak=0`, or `available_capacity_after == reserve_argument`. It checks the growth sum, the copy sum, and the slack identity against the event fields.

## Not executed

- A `BytesMut::reserve` call that leaves capacity unchanged. `noop_reserve_calls` stayed 0. The counter was reconciled at 0. It was not shown incrementing.
- Peer EOF after a reserve. Cancellation here is dropping the future. The zero-byte read path was not hit.
- A growth event after `events_dropped > 0`. Dropped rows in this run were small frames inside the 8192 spare.
- `grow_available` after `try_unsplit` fails, and the branch that grows `available` because `available.len() != 0`. The traces that grew went through a rejoin onto `read`. Those other branches are in the source and were not isolated.
- A frame larger than the spare with bounds left unset. Large frames in the accounting file set a 100000 message cap. Default-off coverage in `pg_wire_bounds` is the small row.
- `inspect`'s `usize` overflow and `expected != frame_u32` check on this 64-bit target, plus the DataRow arms a122 already listed as unexecuted (truncation, remaining body, and a `u32` row-sum overflow beyond the cases `pg_wire_bounds` does hit). `inspect` was not edited. Its line numbers moved because the allocation structs above it grew.
- Vendored Clippy deny lints as their own `cargo clippy -p` invocation.
- `malloc_usable_size` and RSS. Not sampled. The view sums above are not those numbers.
- A new failing oracle against the unfixed tree. The historical RED is a122 `linux/review-1`: `pg_wire_bounds_review` exit 101, 12 passed, 2 failed. That probe was not rewritten. Its `7646 > 8192` comparison remains the wrong oracle it was.
- Dual-arch, supply, release, and a full workspace matrix.
- Postgres itself. Slot A `eef831aa-dba4-4af2-a78b-541bc9e0a125-pg` was up during cleanup and was not used by these tests.
