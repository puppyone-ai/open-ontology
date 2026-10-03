# FINDINGS — a129 receive-view shrink

## The panic

a128 verify attempt 3, thread `catalog_rate_live`, panicked at `buffered.rs:590:18` with `incoming view shrank during advance`. That console was left as it was. The same text was reproduced here on Linux arm64, Rust 1.94.0, bytes 1.12.1.

`red-init/console.log` is the reproduction. The socket implements `ReadBuf::init_mut`, the contract used by `tls_rustls::try_read`. 512 frames of 7 bytes were delivered in 64-byte reads, so a read ends one byte inside the next frame and the following `advance` copies onto a live prefix. The unfixed `checked_sub` at line 590 panics. `unit.exit` is 101. `docker-run.exit` is 101.

`red/console.log` is a different failure: `cargo test -p sqlx-core` exits 101 because the package is outside the workspace. In that same run, plaintext `pg_wire_view_shrink` passed (`tcp.exit` 0). Plaintext `TcpStream::try_read` appends into spare and does not take this path.

## Why the sum can fall

`view_capacity` is `read.capacity() + available.capacity()`. That is the two live windows. It is not RSS and it is not `malloc_usable_size`. The windows may share one allocation. A caller-retained frame and any offset in front of both windows are outside the sum.

bytes 1.12.1 `split_to` gives the piece `cap = at` and advances the parent. `reserve` on a unique `KIND_ARC` handle can set `cap` to the requested length and hide spare. A non-unique `reserve` allocates a new `Vec`, floored at the original-capacity power of two. `try_unsplit` joins only contiguous siblings. When `advance` cannot join, it `extend_from_slice`s and drops the piece. The piece's capacity leaves the sum. The bytes already copied stay valid.

`init_mut` sets `len == capacity` before the reader writes `n` bytes at the front. `advance(n)` takes only those `n` bytes. A zero tail stays in `available`, so the next prepare grows that tail in place instead of rejoining. The windows diverge, the next advance copies, and the view sum can fall (`view_done < view_prepared`) while the net across the step is still positive. That is the line-590 site. The zero tail and the copy are existing sqlx behavior. This repair does not change them.

## The repair

Both expects are gone. Growth and the post-advance extra use `saturating_sub`. A negative delta is not added to `reserved_additional`.

`note_reserve` still ignores a step that did not reserve and did not move the view. A reserve whose view fell increments `reserve_calls` and `reserve_argument_bytes`, adds nothing to `reserved_additional`, and is not a no-op.

After a completed `advance`, the step's contribution is `view_done.saturating_sub(view_before)`. Any excess already added for that step is subtracted, including when the trace row was dropped. `reserved_growth` on that row is the same net, or zero when the sum did not grow. EOF finishes the row with the pre-read growth and returns the unexpected-EOF error. A parked cancel never reaches the adjustment, so the growth recorded before the read stays. `advance` itself is unchanged. No field, dependency, or second copy was added.

`pg_wire_accounting` reconcile was not edited. It still requires `reserved_growth == view_after - view_before` and `events_dropped == 0`. Those runs do not shrink. They passed: 12 tests.

## Bound on the reproducer

With the expect removed, each `init_mut` read commits the new spare, and the live view doubles. `green/oom-512/` kept 512 frames. The test process was signal 9. `OOMKilled=true` at the 8 GiB cgroup limit. lib 55, bounds 13, accounting 12, portal 6, the plaintext peer, and clippy had already been scheduled in that process and passed.

The passing unit test uses 128 frames, which is 14 reads of 64 bytes. `assembly_bytes > 0`. The trace stays at most 64 rows. It does not reach `events_dropped > 0`, because 64 doubling reads do not fit in 8 GiB. Order of the retained `u16` ids matches. `green/unit.exit` is 0.

## Left open

The 60000-row catalog read was not run again. No PostgreSQL was started. Full stream behavior, backend heap, and caller-retained storage stay open. The doubling above is that open heap behavior, not a resident-set measurement.
