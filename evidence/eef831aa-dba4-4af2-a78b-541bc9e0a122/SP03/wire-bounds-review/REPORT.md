# R2-W receive-bound review

Verdict: **REQUEST_CHANGES**. Limited R2-W is not approved. The opt-in reject path that this run executed behaves as specified. The allocation meter does not. Docker exit 101 is the review test binary returning two failures. It is not approval, and it is not a protocol panic.

Subject: commit `a90c86b405febebfdc70f8169be42dc4de4c3dfc`, tree `6e7eb46c09470a8163a324063beef48c6254bbb1`, parent `6de125ab01df6564024005dcc74fbf9d0a96b392`. Seven-path diff against that parent. Reviewer UUID `eef831aa-dba4-4af2-a78b-541bc9e0a122`. One Linux arm64 run, preserved at `linux/review-1/`. No product edit. No second builder.

## Identity

Host `shasum -a 256 -c` of `product-blobs.sha256` against a `git archive` of that commit exited 0. Inside the container, `sha256sum -c` exited 0 before the tests and again after the review probe was copied. The seven content hashes:

| Path | Git blob | sha256 |
| --- | --- | --- |
| `crates/oo-connectors/tests/pg_wire_bounds.rs` | `98ef572b9feefd9d32debf0ab650198310fe1dd8` | `a437de205a904081baeb835986bb4916ff69a184e7712d9d6d7041bd924d2397` |
| `crates/oo-connectors/vendor/PATCH.md` | `1086209f7922f28f09a3c028c559c20dcdb74375` | `debf1235c759358fc787d1fe3f99036e3d92965be5cd751b0c5a46cfb06adca1` |
| `crates/oo-connectors/vendor/sqlx-core/src/error.rs` | `55156f21a8ea65ae39e506ad8bb14ecc3d15e079` | `a8191d776db3566a0a1e07c167395dd5dc8c6bc278795f2a70b0a163d2b296f9` |
| `crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs` | `02f73c97955b0283d12dc7de651ae87729da4aba` | `5138b90d4a4e3e7d6bba977668b4067b386f93e0492416d18c4240910de2bb04` |
| `crates/oo-connectors/vendor/sqlx-postgres/src/connection/mod.rs` | `4e61132e2c77e07d657b25d6f12c0c81f39f3529` | `ef0094ea0ede101f95151d656eba14f73860deed1bf125e4b651dd08a5020cb6` |
| `crates/oo-connectors/vendor/sqlx-postgres/src/connection/stream.rs` | `27c3db92ebd16e0b83e40f14a35f3803e9b4ca04` | `137af21f23c52793b4b3e3b6b7aaafd760d57f0a0583407c09a6c18e7c1cc69a` |
| `crates/oo-connectors/vendor/sqlx-postgres/src/lib.rs` | `a6ce2f3a54f6712249030dacb38a7fceaa9722eb` | `bbf43d753b0f86b0b8105cac9bcf6ae282e81194ae26eb2856f67355529c0093` |

`product-blobs.sha256` is a `sha256sum -c` manifest. Author `hashes.txt` from a120 is a narrative identity record and was not used as a checksum file.

## Run

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, local inspect `os=linux arch=arm64`, `--pull never`. Container `oo-a122-msrv-review`, bridge `oo-a122-eef831aa-net`, volumes `oo-a122-eef831aa-cargo` and `oo-a122-eef831aa-target`. HostConfig: privileged false, CapDrop ALL, no-new-privileges, memory and swap 8589934592, NanoCpus 4000000000, PidsLimit 4096, OOMKilled false, no host network, no socket mount, HOME not mounted. Capacity before start: running rust builders 0, own builders 0, host cargo/rustc/rustdoc/rustup 0.

`rust-toolchain.toml` at the commit requests channel 1.98.1. The container set `RUSTUP_TOOLCHAIN=1.94.0` and refused any other rustc. Recorded toolchain: rustc 1.94.0 (`4a4ef493e` 2026-03-02), host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, cargo 1.94.0 (`85eff7c80` 2026-01-15). Clippy was downloaded for 1.94.0. `airgap_claim=no`: `cargo fetch --manifest-path Cargo.toml --locked` updated the crates.io index and downloaded crates into the empty cargo volume. Later phases passed `--offline`.

| Phase | Exit |
| --- | --- |
| toolchain | 0 |
| snapshot and product blobs | 0 |
| clippy component | 0 |
| `cargo fetch --locked` | 0 |
| `cargo test -p oo-connectors --lib` | 0, 49 passed, 0.00s |
| `cargo clippy -p oo-connectors --all-targets -- -D warnings` | 0 |
| `cargo test --test pg_wire_bounds` | 0, 13 passed, 6.07s |
| product blobs after probe overlay | 0 |
| `cargo test --test pg_wire_bounds_review` | 101, 12 passed, 2 failed, 16.19s |
| `cargo clippy --test pg_wire_bounds_review -- -D warnings` | 0 |
| docker run | 101 |
| inspect | 0 |

Container `SECONDS` at the end of `inside.sh`: 65. Commands are in `linux/review-1/invocation.txt`. Raw output is `linux/review-1/console.log`.

## What the two failures are

`accounting_noop_reserve_offset_and_zero_assembly` reached a real meter trace and then died on a bad comparison. `disable_during_partial_does_not_accept_corrupt_field` died because the probe required a reject that the opt-in contract does not require. Details are in `FINDINGS.md`.

The 12 passing review tests, the author 13, and the library 49 are left as recorded. They were not re-run.

## Accounting that was measured

Frame: one `CommandComplete` of 12000 bytes. Five-byte header absorbed, future dropped, then the body.

While parked on the header, `reserved_additional` rose by 3823 and `reserve_calls` by 1. `view_capacity_peak` stayed 8192. Event count stayed 2. That 3823 is not an event. The event is pushed only after the socket read returns (`buffered.rs` 492–502), and the future was dropped inside the read.

After the body arrived, the new events were:

```
logical=12000 reserve=3823 read_cap_before=5 avail_cap_before=8172 avail_len_before=0
avail_cap_after=8172 view_after=8177 read_len_before=5 returned=8172

logical=12000 reserve=3823 read_cap_before=8177 avail_cap_before=0 avail_len_before=0
avail_cap_after=8192 view_after=20738 read_len_before=8177 returned=3823
```

Printed totals for the post-park window: reserved 7646, capacity growth 8192, no-op reserve argument 3823, `rounding_peak` 4369, `assembly_bytes` 0. The probe then asserted `7646 > 8192` and panicked at `pg_wire_bounds_review.rs:733`. That comparison is wrong. Growth of 8192 is the real second allocation. 7646 is two copies of the 3823 argument (the resumed no-op, plus the argument of the allocation that actually grew). Adding the pre-event 3823 gives 11469 counted against 8192 bytes of new available capacity.

`rounding_peak` 4369 is `8192 - 3823`. bytes 1.12.1, when it abandons a shared buffer, sets the new capacity to `max(requested, original_capacity)` (`bytes_mut.rs` 787). The original buffer is 8192, the request is 3823, so the new view is 8192. The 4369 bytes are extra view capacity from that floor. This run did not call `malloc_usable_size`. Zero would be the wrong rounding figure for this call. The author's 11834-byte tail, where `rounding_peak` was 0, was a different size and was not re-executed here.

`view_after` 20738 matches the same bytes policy: the shared vec is doubled from 8192 to 16384, the 15-byte handshake offset stays outside the window (`16384 - 15 = 16369`), and the new buffer keeps `8192 - 3823 = 4369` after the 3823-byte split. `16369 + 4369 = 20738`. The first event's windows sum to 8177 against a lifetime view peak of 8192, which is the same 15-byte hole.

`assembly_bytes` stayed 0. The second event starts from `available_capacity_before=0` with 8177 bytes already in `read`. Those two views are not contiguous, so `unsplit` copies (`bytes_mut.rs` 964–972). The field comment at `buffered.rs` 53–54 says this path does not make a copy. `PATCH.md` lines 128–131 say the same.

Rejected prefixes in the passing tests did not reserve. A 1600-null DataRow is 6407 bytes: `logical_peak=6407`, `reserved_additional` unchanged, `view_capacity_peak=8192`, `bytes_read=6422`, `overread_bytes=6412`, no event whose logical need or reserve argument is 6410. The 1601-column header stops at `last_logical_need=7` with `bytes_read=22` and `reserved_additional` unchanged. Declared length `4 + 2 + 1601 * 4 = 6410` is not a reserve. Control and unknown frames with a finite `NOLEAK` tail or a declared length of 2000000000 stop at decision byte 5, `reserved_additional` unchanged, view peak 8192. The spare read absorbs the short tail that was actually sent. It does not allocate the declared body.

Twenty-four retained `CommandComplete` frames left `accepted_backing_peak` at 8192, equal to the pre-test peak, while `trace_bytes` grew to 2304. 2304 divides the 72-byte event record. The event log is not the frame buffer. Holding the frames did not multiply the backing peak.

The one-shot 20011-byte DataRow and its RSS sampler sit after the panic. They did not run. RSS was not measured.

## Protocol limits this run proved

Message length is the Int32, including itself, excluding the type byte. Value length is the field payload, excluding its 4-byte prefix. Row payload is the natts word plus prefixes plus payloads. `try_new` rejects `max_message_len < 4` and `max_value_len > max_row_payload` with `InvalidArgument("incoming bounds are inconsistent")`. It accepts zero columns, zero value, and zero row. It also accepts a row cap above the message cap. The message check still fires first on the wire.

Checked edges that passed: Notice body `NOTICE` at message cap 10 accepted and cap 9 rejected at byte 5; value cap 3 accepts `abc` and rejects a declared length 4 at byte 11; two 1-byte fields at row cap 12 accepted, and a second field that would make the row sum 13 rejected at byte 16 with that body not sent; column cap 2 accepts two columns and rejects natts 3 at byte 7. NULL `-1`, empty payload, and `Z` are delivered. Value cap 0 accepts an empty field and rejects a 1-byte field at byte 11. Lengths `-2` and `i32::MIN` are `NegativeLength` at byte 11. A field that runs past the declared message is `Truncated` at byte 11. A natts word that does not fit (`message_len` 5) is `Truncated` at byte 5. A zero-column row whose declared frame continues past the natts word, with `EXTRA` actually sent, is `Remaining` at byte 7. An empty row (`message_len` 6) is accepted. Lengths 0, 1, 2, and 3 are `ShortLength` at byte 5, including with bounds unset. `u32::MAX` is `Overflow` at byte 5 via the `u32` checked add. Row cap 0 rejects the empty row at byte 7.

Formats `N E S d A T C Z 1` and `0x7f`, over the message cap, are `Limit(Message)` at byte 5 before type classification. Unknown `0x7f` under the cap with bounds set is `BadType` at byte 5. A later write after poison does not increase `bytes_read`. DataRow over the message cap is `Message` at byte 5, before natts. Two complete frames buffered ahead of a huge Notice are delivered, and the third recv is the message fault with no further read. An accepted row and a following value-cap reject share the buffered read; `last_logical_need` is 11 and `reserved_additional` is unchanged.

Fragmented cancels at header and field-prefix offsets resume in order and then deliver the following `EmptyQuery`. A tighter cap, or enabling bounds, applied to an already buffered over-cap header rejects with no further read. A legal header followed by a valid body still completes after `set_incoming_bounds(None)`. Poison blocks `recv_backend` and `ping` (2s bound). `set_incoming_bounds(None)` and a wider bound do not clear `incoming_synchronized() == false`. `close_hard` after a poisoned `ping` made the peer read exactly `S\x00\x00\x00\x04`. `close_hard` with an empty write buffer made the peer read zero bytes. Peer drop after a 5-byte header is an I/O error, not `IncomingWire`. A second connection still reads a small row. `Error` display and debug for the bounded rejects contain no `NOLEAK`, `EXTRA`, or `SECRET`, and no ASCII digit.

`stream.rs` has no `unsafe`. The new error display is static (`error.rs` 69–78).

## Unexecuted branches

On this arm64 run, `message_len + 1` fits in `usize` whenever it fits in `u32`, so `stream.rs` 398–403 did not run. `u16` natts times 4 plus 2 fits in `u32`, so the `checked_mul` overflow at 448–449 did not run. The `prefixes_end` overflow at 454, `usize::try_from(flen_u)` failure at 503, and `body_end` overflow at 506 did not run. A `u32` row-sum overflow needs two large field lengths and would buffer the first payload before the second check; that case was not sent. No `u32::MAX` body and no `i32::MAX` payload were sent. The 20011-byte one-shot inside the accounting test did not run.

## Left open

`crates/oo-connectors/src/error.rs` `map_driver` (188–213) has no `Error::IncomingWire` arm. The catch-all at line 212 returns `SourceError::protocol("driver")`. Catalog and probe opt-in are future work. `executor.rs` 254–271 still sends `Execute` limit 0 and an unnamed `Close`. `docs/10-security.md` TB7 is source write and source DoS. These client caps are not a backend `palloc` limit. Named portals, paging, Arrow, decode, whole SP03, integration, dual-arch, supply, and release were not in this run. This review does not close the 87-item goal.
