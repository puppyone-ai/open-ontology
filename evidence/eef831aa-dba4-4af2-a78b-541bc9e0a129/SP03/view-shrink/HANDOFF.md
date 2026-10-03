# HANDOFF — a129 receive-view shrink repair

Source commit `79515fcf7e7032c8c212540eaae4b48a5957e3a2` on parent `3f66773adf1afe378fc73a8ca56abf25bcb556cf`. Tree `b78f84497484f6232aecf0b51ffcc6d27be9fd7c`. The commit touches only `buffered.rs`, `PATCH.md` hunk 3 wording, and `tests/pg_wire_view_shrink.rs`.

The false `checked_sub` expects are gone. A completed read records the net increase of `read.capacity() + available.capacity()`, or zero when that sum fell. Protocol errors, the copy in `advance`, cancel, and EOF behavior are unchanged. This is not an RSS figure.

## Read first

1. `FINDINGS.md` — panic site, bytes 1.12.1 window rules, and the meter change.
2. `REPORT.md` — image, toolchain, source hashes, and exits, including the failures.
3. `red-init/console.log` — the panic: `incoming view shrank during advance` at `buffered.rs:590:18`, `unit_exit=101`.
4. `green/console.log` — unit, plaintext 1024-frame peer, lib 55, bounds 13, accounting 12, portal 6, clippy 0.
5. `green/oom-512/console.log` — the same fix with 512 frames, signal 9, `OOMKilled=true`. The other checks in that run passed.
6. `red/console.log` — plaintext peer passed on the unfixed driver. `cargo test -p sqlx-core` cannot run inside this workspace.

a128 verify attempt 3 was not rewritten. `pg_wire_accounting.rs` was not changed. No checklist, ledger, or spec edit.

## Still open

Full stream behavior, backend heap, and caller storage. After the panic is gone, small `init_mut` reads double the live view; 512 frames exhausted 8 GiB. The 60000-row catalog path was not re-run. Independent review of `79515fcf` is next.
