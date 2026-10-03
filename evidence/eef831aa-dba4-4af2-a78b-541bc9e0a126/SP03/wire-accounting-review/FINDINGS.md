# Findings

Subject `ff783ac933b86e0ff068181afe0258764c5d2e5d`. Line numbers are that commit. Measurements are from `linux/review-2/console.log` unless a row is marked review-1. The peer is a scripted TCP backend. No Postgres server. No RSS sampler.

## Meter, as the source defines it

`BytesMut::reserve(n)` in bytes 1.12.1 (`bytes_mut.rs` 610–617) returns immediately when `n <= capacity - len`. Otherwise `reserve_inner` grows the view, sometimes by more than `n` (the original-capacity floor at `bytes_mut.rs` 784–787, and `Vec` doubling).

`ReadBuffer::prepare_window` (`buffered.rs` 611–635) computes `spare = available.capacity() - available.len()`. When `deficit <= spare` it returns `(0, 0)` and does not call `reserve` (616–618). When it does call, the argument is `deficit`, the additional bytes beyond current `len`, not `deficit - spare`.

`reserved_additional` adds the change in `read.capacity() + available.capacity()` (`buffered.rs` 534–536, 663–676). `reserve_argument_bytes` adds the argument. `noop_reserve_calls` increments only when an argument was passed and that sum did not change (670–673). `rounding_peak` is the largest `available.capacity() - available.len() - argument` among calls that grew (679–689). `assembly_bytes` adds `old` when `reserve` changes `as_ptr` and `old > 0` (637–648, 651–660), and adds the appended length, plus `old` again if that `extend_from_slice` moves `as_ptr`, when `try_unsplit` fails (`buffered.rs` 736–746). A successful `try_unsplit` adds nothing (`bytes_mut.rs` 1081–1095).

The event row is pushed before `socket.read` is awaited (`buffered.rs` 544–560). EOF with a zero-length read finishes that row with `bytes_returned=0` and `read_completed=1` (564–573). The log stores at most 64 rows (`buffered.rs` 147, 693–696) and then increments `events_dropped`. Counters are updated whether or not the row is stored.

`PATCH.md` 128–153 and `connection/mod.rs` 149–152 say the same thing: the two view capacities are not a physical resident total, not `malloc_usable_size`, and not the sum of caller-retained frames. That wording matches the fields. It does not match a reading of those fields as RSS.

## F1 is repaired on the measured traces

Header first, 12000-byte `CommandComplete`, bounds `100000/0/0/0`. Parked after the 5-byte header:

```
logical_peak=12000 reserved_growth=8192 reserve_arguments=11995
reserve_calls=1 noop_calls=0 view_capacity_peak=16369
reads=2 bytes_read=20 assembly=0 rounding=4369
events=3 dropped=0
```

`bytes_read=20` is the 15-byte handshake plus the header. The body has not been read. An event with `read_completed=0`, `reserved_growth=8192`, `reserve_argument=11995`, and `bytes_returned=0` is already stored. `11995 > 8172` (spare on `available` before the call) and `11995 != 8192`. `view_after - view_before = 8192`. Rounding 4369 is spare left above the 11995 argument after the tail is split off (`16369 - 5 = 16364 = 11995 + 4369`). The 15-byte handshake offset is outside both windows (`16384 - 15 = 16369` if the underlying vec doubled to 16384). The meter does not show that vec, and this review does not treat 16369 or 16384 as `malloc_usable_size`.

After the body, `reserved_growth` is still 8192, `reserve_calls` is still 1, `bytes_read=12015`, and the parked row is still `read_completed=0`. The retry row has `reserved_growth=0`.

The same 8192 is unchanged after a 100-byte partial body (`bytes_read=120`) and after the rest of the frame (`bytes_read=12015`).

EOF, header then write-side shutdown, same sizes:

```
reserved_growth=8192 reserve_arguments=11995 bytes_read=20
rounding=4369 events=3
```

The error is `UnexpectedEof` with the text `expected to read 12000 bytes, got 5 bytes at EOF`. The reserve row is `read_completed=1` and `bytes_returned=0`. Growth is not rolled back.

A one-shot 12000-byte frame with bounds left unset fills the window before the reserve. review-1 recorded this event and then failed a probe assertion that required spare `> 0`:

```
logical_need=12000 reserve_argument=3823 reserved_growth=8192
read_capacity_before=8177 available_capacity_before=0 available_len_before=0
available_capacity_after=8192 view_before=8177 view_after=16369
read_len_before=8177 bytes_returned=3823 assembly_bytes=0 read_completed=1
```

`3823 = 12000 - 8177`. The argument is the deficit past `read.len()`. Spare on `available` is 0, so this call cannot be the old no-op (that no-op was a 3823 argument against spare 8172). Growth is 8192, rounding on the review-2 rerun of this case is 4369, and `noop_calls=0`. review-2 accepts the frame and records `reserve_arguments=3823`, `reserved_growth=8192`. review-1 had `assembly=0` for this case; review-2 had `assembly=8177`. Both are `<= read_len_before`. Which one occurs is not stable. See F2.

a122 counted a 3823 argument that did not change capacity, including a count with no event, because `reserve` was given `deficit - capacity` and the row was pushed after the read returned. Neither of those shows up on these traces.

## F2, within the documented scope

`assembly_bytes` is no longer stuck at 0, and `PATCH.md` 144–147 no longer says the receive path allocates no second copy.

Two 20011-byte one-shots in review-2, each on its own connection:

```
CommandComplete format=67 argument=11834 growth=11834 assembly=8177
  read_len_before=8177 bytes_returned=11834 avail_before=0 avail_after=11834 rounding=0

DataRow         format=68 argument=11834 growth=11834 assembly=0
  read_len_before=8177 bytes_returned=11834 avail_before=0 avail_after=11834 rounding=0
```

`11834 = 20011 - 8177`. Growth equals the argument because `available_capacity_after` is 11834, so rounding is 0. `8177` is the live prefix (`read_len_before`), not the new 11834 bytes. `0` means `as_ptr` did not change. The author's final log had the opposite pair: CommandComplete assembly 0, bounds DataRow assembly 8177. review-1 in this rereview matched review-2 for these two tests (8177 and 0) and flipped the unset 12000 case. The message type does not decide the copy. Heap layout does.

The pointer check counts `old` when `as_ptr` changes (`buffered.rs` 641–645). That matches bytes' explicit `copy_nonoverlapping` paths. It does not establish a physical copy:

- `Vec::reserve` / `realloc` may change the address with `mremap` and not copy pages. The meter still adds `old`.
- When `realloc` does copy, the `Vec` includes the offset in front of the view. `assembly_bytes` does not. `PATCH.md` 146–147 says that offset is outside either window. The 15-byte handshake hole is that offset.
- In-place growth that keeps the pointer adds 0, which is what bytes does on that path. An allocator copy that does not change the `BytesMut` pointer is not visible and was not observed as a separate quantity.

So `assembly_bytes` is a live-view relocation count plus an explicit failed-`unsplit` copy. It is not RSS and not a proven DRAM memcpy. This approval does not treat 8177, 0, or 289 as a physical copy size of the frame.

Disjoint spare, reached on purpose. After the header-first reserve (`growth=8192`, `assembly=0`), `Connection::shrink_buffers` replaces `available` when its capacity is above 8192 (`buffered.rs` 751–764, `connection/mod.rs` 313–315). The public meter does not change on that call. The body then requires a new reserve because the fresh buffer is 8192 and the remaining deficit is 11995:

```
copied=11995 body=11995 grew=8192 calls=2
reserved_growth=16384 reserve_arguments=23990 assembly=11995
view_capacity_peak=16389 bytes_read=12015
```

`23990 = 11995 + 11995`. The second argument is the deficit past the 5 bytes already in `read`. `11995` is one `extend_from_slice` of the new piece (`buffered.rs` 741–742), not that piece plus itself, and not the piece plus the 8192 of new capacity. `try_unsplit` failed because shrink installed a different allocation. `available.len()` at `prepare_window` was 0 on this path. The arm at `buffered.rs` 621–622 (`available.len() != 0`) was not entered.

Retained frames. Twenty-four `CommandComplete` bodies of 1000 bytes, all held by the caller:

```
retained_wire=24120 bodies=24000
view_peak=8192 accepted_backing=8192
reserved_growth=16095 reserve_arguments=1721 reserve_calls=2
assembly=289 rounding=7187 reads=27 bytes_read=24135
accepted_logical=24005 events=27 dropped=0
```

`24005 = 24000 + 5` handshake contents. `8192` is one live window. `16095` is the sum of live-window increases and is larger than one window because a shared allocation was abandoned and a new one charged. `289` is the prefixes copied while retained frames kept the old allocation. `7187` is slack above a small argument, from the original-capacity floor. Dropping the 24 frames leaves `view_capacity_peak` at 8192. The meter has no sample of their storage. `PATCH.md` 136–138 is accurate on that point. There is still no counter for the sum of pinned shared buffers.

## Event log

Small `CommandComplete` frames until a row is dropped, then one 12000-byte frame:

```
event-cap:     reads=65 bytes_read=399 messages=66 events=64 dropped=1 trace_bytes=6656
after-cap:     reads=67 bytes_read=12399 messages=67 events=64 dropped=3
               reserved_growth=8192 reserve_arguments=4207 assembly=7793
               view_capacity_peak=15985 rounding=3985 trace_bytes=6656
stored_growth=0 stored_arguments=0
```

`6656 = 64 * 104`, and 104 is the size of the 13-field event. `7793 = 12000 - 4207`, the live prefix of the post-cap reserve. That prefix is on `assembly_bytes` and the 8192 is on `reserved_additional`, while the stored rows still sum to 0 growth and 0 arguments. `events_dropped` went from 1 to 3. `trace_bytes` did not move. The first stored event is still the connect-time row. The diagnostic is a fixed-capacity integer log, not a per-frame string.

`reconcile` on traces with `events_dropped=0` matched growth, assembly, arguments, calls, no-op calls, bytes read, and rounding to the rows. After the cap, the stored rows are a prefix of the history and the counters are the full history. Both are what the code does (`buffered.rs` 693–723).

## Regressions that still hold

Huge declared length 2000000000, message cap 1000, header only:

```
bytes_read=20 reserved_growth=0 reserve_arguments=0 reserve_calls=0
logical_peak=9 last_logical=5 view_capacity_peak=8192
```

A following `NOLEAK` frame does not increase `bytes_read`. The error display has no digits and no `NOLEAK`. `incoming_synchronized` stays false.

Short length 3 with bounds unset is `ShortLength` at 5 bytes, no reserve. A following frame is not read. `set_incoming_bounds(None)` and a wider bound do not clear the fault. `ping` returns that fault inside 2 seconds. `close_hard` makes the peer read `S\x00\x00\x00\x04`. A second connection reads `ok`.

A 4-byte prefix of `ORDER-BODY`, dropped, then the remainder, then `NEXT`, delivers both payloads. `reserved_growth=0` because the frame fits in the initial spare. `noop_calls=0`.

Unset 12000-byte `CommandComplete` is accepted (`logical_peak=12000`, `bytes_read=12015`, synchronized). Default-off is the historical whole-frame path plus the length fixes. It is not a reject, and it still accounts the reserve. F3 and F4 from a122 are still not defects. Unset after a legal partial header, and unset unknown tag `0x7f`, were not recast as failures.

`stream.rs` has no `unsafe` in `inspect`. The bounded error display stays static.

## No-op reserve is unreachable, and was not faked

Every trace recorded `noop_reserve_calls=0`, including the old 12000 split that used to count 3823 without growing. The counter was not shown incrementing.

`prepare_window` calls `reserve` only when `deficit > spare`. `grow_available` reserves that same `available`, whose spare was just measured. `grow_read` runs only after `try_unsplit` of an empty tail. `split_off` leaves `read.capacity() == read.len()`, so the joined spare equals the old `available` spare, which is `< deficit`. `BytesMut::reserve` then cannot take the early return at `bytes_mut.rs` 614–617. A positive no-op would require a call with `additional <= capacity - len`. This function does not make one.

The early return `(0, 0)` is a skipped call, not a no-op call. `note_reserve` ignores that pair (`buffered.rs` 664–666). The small-frame tests hit that return (`reserve_calls=0`).

## `available.len() != 0` was not reached

`available` starts empty. `grow_read` assigns `split_off(read.len())`, which has `len == 0`. `grow_available` is only selected when `len == 0` or, on the untested arm, when `len != 0`. `socket.read` appends, and `advance` then removes exactly the appended count. If the view was empty at the start of the read, it is empty afterward. `shrink` installs another empty buffer. Nothing else writes `available`. By induction the arm at `buffered.rs` 621–622 does not run on `ReadBuffer::read`.

If a later change left bytes in `available`, `advance` would `split_to` the new read count from the front of those bytes and could leave the new socket bytes behind. That is latent. It is not a behavior this peer can produce, and no test here claims it did.

The failed `try_unsplit` that did run is `advance` after shrink (`buffered.rs` 736–746), with `available.len() == 0` at prepare time and a nonempty piece after the read. Copy count 11995, once.

## `usize` and `u32` arms on this target

`inspect` (`stream.rs` 425–435) rejects `message_len == u32::MAX` at the `u32` add (427–428). The `usize` add (430–431) is the same quantity after that check. On 64-bit, `u32::MAX` fits in `usize` anyway. On 32-bit the `u32` check has already returned. The inequality at 433–434 compares `message_len + 1` with itself. Those three arms do not run on this machine.

`inspect_data_row`: `natts` is a `u16`, so `natts * 4 + 2` fits in `u32` (480). `5 + prefix` fits in `usize` (486). `cursor + 4` stays inside a frame whose end fits in `u32` (496) and therefore fits in 64-bit `usize`. `u32` to `usize` (535) succeeds for every `u32` on this target. `prefix_end + flen` (538) fits in 64-bit `usize` because both terms fit in `u32`-sized arithmetic. None of these were executed, and none are reachable here.

A `u32` row-sum overflow (526) is checked before the field is tested against the frame. One `i32` field length cannot overflow `u32` by itself. Two can, only after the first payload has been buffered and the running sum is already above `u32::MAX - i32::MAX`. That is a multi-gigabyte body. It was not sent. Finite peers do not reach it.

## Not a resident cap

The original driver card asked to separate logical need, resident capacity, and retained frames. This commit separates them by not adding the last two into the first. The measured gap is the retained run: 24120 wire bytes held, live-view peak 8192, cumulative growth 16095. A caller that keeps every frame pins the abandoned shared allocations. `reserved_additional` does not shrink when `shrink_buffers` drops a spare, and it does not shrink when the caller drops frames. `view_capacity_peak` is a high-water of one window sum, not the process.

The message cap still rejects a declared body before `reserve` of that body. It does not limit how many accepted frames the caller retains, and it does not limit backend `palloc`. `docs/10-security.md` TB7 is unchanged. If a later task needs a resident cap, this meter is not that cap. No code change is proposed for the accounting that was measured.

Optional doc sentence, not required for this verdict: say in `PATCH.md` 144 that a pointer change counts the live prefix as relocated, and that `realloc` may move a mapping without copying pages. The offset exclusion is already there.

## Left open

- `map_driver` still folds `IncomingWire` into fact `driver`.
- Catalog and probe production opt-in.
- `executor.rs` still sends Execute limit 0 and an unnamed Close. Named portals, paging, Arrow, decode, and a full reader are outside this hunk.
- R1 and the schema guard. Slot A `a125` was live and was not used.
- Dual-arch, supply, release, whole SP03, and the 87-item goal.
- `malloc_usable_size` and RSS. Not sampled.
- Vendored Clippy as its own `cargo clippy -p` on sqlx. The connector package Clippy, including its tests, exited 0.
