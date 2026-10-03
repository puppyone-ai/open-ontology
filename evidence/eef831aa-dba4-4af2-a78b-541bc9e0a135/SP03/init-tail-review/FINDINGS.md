# Findings

Driver tail and cap path: **APPROVE_LIMITED**.

Full reader, R3, and R4 stay unsigned. One finite `pg_stream_live` and one finite `pg_stream_bounds` on `bd77b252` exited 0. That pair shows the corrected driver and the Arrow stream in this commit can complete those two tests. It is not a full-reader signature.

## Growth

`BytesMut::init_mut` (`read_buf.rs:16-26`) sets `len` to `capacity` before `Read` writes `n` bytes at index 0. rustls (`tls_rustls.rs:50`) and the async-io adapter (`rt_async_io/socket.rs:14`) use that path. Tokio (`rt_tokio/socket.rs:14`) uses `try_read_buf` and appends.

`committed_init_len` (`buffered.rs:535-540`), applied at `buffered.rs:589-592`:

- Append (`len_now - pre_len <= n`) keeps `len_now`.
- A short overwrite of an empty prefix keeps `n` and drops the zero tail.
- A short overwrite of a nonempty prefix keeps `max(pre_len, n)`, so the unread prefix remains and the filled spare past it does not.

Without that truncate, bytes 1.12.1 reserves a new window for a few bytes of deficit and doubles. a132 measured that shape to view 32643 by the fourth 512-frame read, and a message cap of 32 did not stop it. The later 8 GiB cgroup kill is the same shape with the expect removed.

This review's cap-set of 512 frames, cap 32, on bytes 1.12.1 (`fc652a48c352aef3ea3aed32080501cf3ef6ed5da78602a020c991775b0aff04`) and on the product blob `b56fe942…`:

- dropped: view 8192 from frame 0 through frame 511, reserve 0
- retained: view 8192 across the same samples, reserve 0, caller held 1024 payload bytes
- fuse 24 KiB, `OOMKilled=false`, no 8 GiB run

The cap is still not what stops the doubling. Truncation is. The cap test shows an oversize header (declared length 100000) is refused at 5 bytes with view 8192.

## Prefix, EOF, cancel, reset

Three-byte reads of a split frame kept order and stayed at view 8192 on both the `init_mut` peer and the append peer.

EOF (`read == 0`) still truncates before the `UnexpectedEof` return. On a 5-byte ask the reserve stays 0, which is `after >= before`. The following 512 frames match the oracle and stay at 8192. That case does not exhibit a nonzero reserve; the 9000-byte parked read does: reserve 8192, view 16384, event `read_completed == 0`.

A cancel dropped before `socket.read` returns does not truncate (`buffered.rs:582-584`). For the 5-byte ask, view at the drop is 8192 and reserve is 0. The next 512 frames reserve 8192 once and peak at 16384, with no growth after the frame-64 warmup. A `ConnectionReset` from `Socket::read` (`socket/mod.rs:75-80` returns non-`WouldBlock` immediately) is the same class: view 8192 at the error, 16384 after the following frames, then flat.

`truncate` does not shrink capacity. The view meter remains `read.capacity() + available.capacity()`. The charge is not refunded on EOF, cancel, or reset.

## TLS read

Verify-full, 12000 rows, independent host oracle. View peak 8192 bytes, RSS 8924 KiB, caller-retained 48000 bytes. Pairwise distinct. Reader log nonempty (2 statement lines), 0 write verbs, 0 reader `SET`. Admin `ALTER ROLE ... SET log_statement` ran on a separate connection that had `log_statement=none` and was closed before the reader connected. Row order and the sha256 of big-endian ids match `probe/oracle.txt` (`ec867603e6dd3cbddf278bc24c3b97d2ef1ddfe82509661190acec0804352d89`).

`PgConnection::incoming_allocation().view_capacity_peak` is the two `BytesMut` windows. It is not RSS and not the caller-held rows. `fetch_all` here is the driver functional test. The bounded Arrow observations are the live-test figures: wire view peak 8192, decoded payload 59 matching the SQL oracle, reader audit with 0 write verbs.

## Integration, limited

`pg_stream_live` and `pg_stream_bounds` exit 0 on this tree. Live released finish, abort, and drop. Bounds refused `max_value` and `max_row`, and the idle consumer logged no new statement across a 400 ms sleep. See `REPORT.md` for the outcome lines.

Author exits reused, not re-run: bounds, accounting, portal, tail, view, pg-test, pg-worst, lib, and `clippy-worst.exit`, all 0 under a133 `proof/`. This tree's own lib smoke is 57, not the author's 55, because `stream.rs` is in the archive.

No workspace `sqlx-core` unit run. The embedded stripped copy from a133 was not repeated. Feature set used by `oo-connectors` is `sqlx` `runtime-tokio` + `postgres`, and `sqlx-core` `_rt-tokio` + `_tls-rustls-ring-no-roots`.

## Harness failures, not product failures

1. First unit assert demanded `reserved_additional > 0` immediately after a 5-byte parked `init_mut`. The spare already covered the ask. Excerpt: `probe/attempt1/recovered-panic-excerpt.txt`.
2. TLS compare double-hashed the digest. Ids had matched. `probe/attempt2/tls.stdout`.
3. Live and bounds passed their functional checks and then failed to open `/pglog/postgresql.log` until the log volume was mounted read-only. `probe/attempt2/`.

No product line is cited for a change request.
