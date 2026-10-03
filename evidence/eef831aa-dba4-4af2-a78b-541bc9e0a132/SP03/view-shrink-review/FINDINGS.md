# FINDINGS — a132 receive-view shrink review

Reviewed `79515fcf` (`buffered.rs`, `PATCH.md` hunk 3 wording, `tests/pg_wire_view_shrink.rs`). The buffered change is the 199-line diff, including the embedded test. Line numbers below are that file in the reviewed tree.

## Panic and meter

`view_capacity` at `buffered.rs:522` is `read.capacity() + available.capacity()`. It is the two live windows. It is not RSS.

`read` at `526-606` used to `checked_sub(...).expect("incoming view shrank during reserve")` and `expect("incoming view shrank during advance")`. Both expects are gone. Growth is `saturating_sub` (`540-541`, `592`).

`note_reserve` (`672-694`) still returns immediately when the argument is 0, the view did not grow, and the view did not shrink. A reserve whose view fell increments `reserve_calls` and `reserve_argument_bytes`, adds nothing to `reserved_additional`, and is not a no-op.

After a completed read the step's contribution is `view_done.saturating_sub(view_before)` (`599-603`). `phase` is the provisional growth plus any post-advance extra. `excess` is `phase.saturating_sub(net)`. The subtraction removes only that excess. `net` is at least 0, so the subtraction cannot exceed what this step added. The probe's per-frame samples of `reserved_additional` are non-decreasing on every run, and with `events_dropped == 0` the counter equals the sum of the stored row growths.

EOF (`568-577`) finishes the row with the pre-read growth and returns before `599-603`. The probe asked for 9000 bytes, the reader called `init_mut` and returned 0, and the row stayed `read_completed=1`, `bytes_returned=0`, `reserved_growth=8192`, `reserved_additional=8192`.

A parked cancel drops at the await (`566`). The adjustment is after that await. The probe polled once, observed `Pending`, dropped the future, and the row stayed `read_completed=0`, `bytes_returned=0`, `reserved_growth=8192`, `reserved_additional=8192`.

`push_event` (`710-716`) still stores at most `MAX_INCOMING_ALLOC_EVENTS` (64, line 152). The adjustment still runs when the row was not stored (`597-602` run before `finish_event`, and `finish_event` returns immediately when `pushed` is false). `prepare_window` (`620-644`), `advance` (`746-766`), and the unset-bounds path in `stream.rs` are unchanged by this commit. `pg_wire_accounting` was not edited. Its 12 tests passed; those runs do not shrink the view.

## The embedded test

`init_mut_small_frames_shrink_the_receive_view` (`867-937`) sets `FRAMES` to 128 (`874`). The comment on `871-873` says the 512-frame shape was killed at the 8 GiB cgroup once the expect was gone. The test asserts a copy happened, the trace length is at most 64, and the meter identity. `assert_eq!(events.len() == 64, events_dropped > 0)` (`915`) is false equals false for 14 reads, so it does not exercise the trace cap. There is no assertion on `view_capacity_peak`. a129's plaintext `pg_wire_view_shrink` passed on the unfixed driver because `TcpStream::try_read` uses `try_read_buf` (`rt_tokio/socket.rs:12-14`) and never reaches the shrink expect. That pass is not the regression proof.

## init_mut is the production TLS contract

`BytesMut::init_mut` (`read_buf.rs:17-26`) zero-fills the spare and sets `len` to `capacity`, then returns the whole buffer. `RustlsSocket::try_read` (`tls_rustls.rs:49-50`) is `self.state.reader().read(buf.init_mut())`. `Read::read` writes `n` bytes at the front of that slice and returns `n`. It does not shrink `BytesMut`. The async-io adapters do the same call (`rt_async_io/socket.rs:13-14`). `ReadBuffer::advance(n)` (`746`) then keeps the tail.

The author's socket (`809-819`) does that same trio: `init_mut`, copy `n` at the front, return `n`. The probe's `InitMut` arm is that contract, hard-capped at 4 chunks of 64 bytes (36 frames, 252 bytes). The `Append` arm uses `put_slice` and never calls `init_mut`, which is the effect of Tokio `try_read_buf`.

`InitMut`, frames dropped, cap unset. `reserved_additional=24640`, `view_capacity_peak=32643`, `reads=4`, `bytes_read=252`.

| read | reserve arg | growth | view before | view after | available len before | available cap after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 0 | 0 | 8192 | 8192 | 0 | 8192 |
| 1 | 4 | 64 | 8129 | 8193 | 8128 | 8192 |
| 2 | 3 | 8192 | 8130 | 16322 | 8128 | 16320 |
| 3 | 2 | 16384 | 16259 | 32643 | 16256 | 32640 |

`available.len()` before reads 1-3 is the tail `init_mut` left behind. `prepare_window` then takes the `available.len() != 0` branch (`628-631`) and `reserve`s a deficit of 4, 3, or 2 bytes. bytes 1.12.1 turned those arguments into available capacities 8192, then 16320, then 32640. The live view doubled on the last two reads while every frame was dropped.

The same run with every frame retained reached `view_capacity_peak=40706` and `read_capacity_peak=8192` (`read_capacity_peak` was 66 when the frames were dropped). Retention adds the held window to the sum. The doubling of `available` is already present when the caller drops each frame.

The append reader on the same 252 bytes: `reserved_additional=0`, `view_capacity_peak=8192`, `assembly_bytes=0`, and `available len before` is 0 on every read. The view stepped 8192, 8129, 8066, 8003. That is the plaintext contract. The growth shows up on the TLS and async-io contract.

## The opt-in cap

`PgStream::inspect` (`stream.rs:437-443`) returns `IncomingWireError::Limit` when `message_len > max_message_len`, before `Need(expected)` (`457-460`). The probe's parser does that length check and refuses `expected > 64` as a fuse. It does not construct `PgStream`. The allocation still happens inside `ReadBuffer::read`, and the cap only changes the `usize` passed as `Continue`.

With the cap set to 32, every 7-byte frame has `message_len` 6, so the cap accepts it. Result: `view_capacity_peak=32643`, `reserved_additional=24640`, and the same per-read view numbers as the uncapped drop run. `CLASS cap_vs_uncapped=SAME`. One step's `assembly_bytes` differed (8192 versus 64 on read 2). The view peak did not.

A header whose `message_len` is 100000, with the same cap of 32, returned `Protocol("cap reject 100000")`. `logical_requested_peak=5`, `reads=1`, `bytes_read=5`, `view_capacity_peak=8192`, `reserved_additional=0`. The cap stops a declared body above the limit. It leaves the short-read doubling alone, because those frames are under the limit and the reserve argument is the tiny deficit, which bytes 1.12.1 then floors to a doubled buffer.

a129 `green/oom-512/` is the same curve continued until the 8 GiB cgroup: 512 frames, signal 9, `OOMKilled=true`, after lib, bounds, accounting, portal, plaintext, and clippy had passed in that process. This review did not repeat it. The embedded test's reduction from 512 frames to 128 frames (14 reads of 64 bytes) is why that test fits in 8 GiB. It does not show a bound.

## Minimal fix, not applied

No product file was edited. The change that matches these measurements is local to `ReadBuffer::read`: remember `available.len()` before `socket.read`. When the call returns `n`, including `n == 0`, if `available.len()` is greater than that saved length plus `n`, `truncate` to the saved length plus `n` before `advance` and before the EOF return.

`try_read_buf` already leaves `len` equal to the saved length plus `n`, so the truncate does not fire and the append path stays as measured. `init_mut` leaves `len == capacity`. The truncate drops the zero tail, `advance(n)` leaves `available` empty, and the next `prepare_window` no longer `reserve`s a full buffer in order to read 2 or 3 more bytes. That is the proposal. It was not compiled or run.

The message cap is the wrong knob for this path. A cap of 32 accepted the frames and the view still doubled.
