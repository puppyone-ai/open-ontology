# a135 fixed TLS-tail review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a135`. Reviewer ran the tests. No subagents and no worktrees.

## Verdicts

Driver tail and cap path: **APPROVE_LIMITED**.

On `bd77b2520bcc3d63517bc49309002f25b58544b1`, `ReadBuffer::committed_init_len` drops the zero tail that `BytesMut::init_mut` leaves after a short `Read`. A representative cap-set of 512 short frames, caller-dropped and caller-retained, stays at view capacity 8192 for frames 0, 1, 2, 3, 4, 63, and 511. That is under the 24 KiB fuse. A parked cancel and a connection reset each leave one reserve of 8192 and a later peak of 16384, then stop. The 8 GiB short-frame exhaustion was not repeated.

This approval removes that driver-specific growth blocker.

Full reader gate: **unsigned**. R3 and R4: **unsigned**.

`pg_stream_live` and `pg_stream_bounds` were run once on this combined tree and exited 0. That is finite-path integration compatibility of the corrected driver with the Arrow `stream.rs` already in `bd77b252`. It does not sign the full reader, and it does not replace an R3 or R4 run. No matrix and no license tests were run.

`fetch_all` of 12000 rows is a driver functional read. It is not a connector row-rate proof and not a bounded Arrow proof.

## Source under review

| Item | Value |
| --- | --- |
| Fixed commit | `bd77b2520bcc3d63517bc49309002f25b58544b1` |
| Parent | `6a6d2166ae6843a1d9bee043f1b3f4443fb30fbd` |
| Author evidence | `3a5478ac` (session a133) |
| Author input the proof compiled | `79515fcf7e7032c8c212540eaae4b48a5957e3a2` plus a three-file overlay. That snapshot did not compile `stream.rs`. |
| Arrow `stream.rs` git blob in `bd77b252` and in `75b993d0` | `e7b5a6cc1714c67e818899788a09e0315b93b3ed` |
| `buffered.rs` content sha256 | `b56fe942de40260bfaf39e83f32af8630ded0d739bc758480940099124cfe7d5` |
| `buffered.rs` git blob | `3ba2b87507096bab77464eaab7797712e4bf02cc` |
| `stream.rs` content sha256 on the private snapshot, before and after probes | `47c8262a4a40f303c430435005d246a3f9531b504f8f6a579b83f0ceb89c0898` |
| `bytes` | 1.12.1, lock checksum `fc652a48c352aef3ea3aed32080501cf3ef6ed5da78602a020c991775b0aff04` |

The private snapshot was a git archive of the product paths at `bd77b252`: workspace manifests, `crates`, `.cargo`, `rust-toolchain.toml`, and `contracts/source/pgsql/empty-catalog.json`. The overlay test was copied into that snapshot at probe time. The snapshot mount was read-only. Production source, manifests, lock, spec, and ledger were not edited.

`rust-toolchain.toml` at that commit says channel 1.98.1. The review set `RUSTUP_TOOLCHAIN=1.94.0`. The builder printed `rustc 1.94.0 (4a4ef493e 2026-03-02)` host `aarch64-unknown-linux-gnu`.

## Images and isolation

Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `--pull never`.

PostgreSQL `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Server text: `17.10 (Debian 17.10-1.pgdg12+1)`.

Private network `oo-a135-eef831aa-net`, private cargo, target, home (`HOME=/tmp`), CA, and tmp. No host network, no docker socket, no host home, no shared cargo cache, no auth env on argv, no auth stores in evidence, no private keys in evidence. Rust containers: `--cap-drop ALL`, no new privileges, memory 8 GiB, swap 8 GiB, 4 cpus, pid limit 4096, no published ports. Postgres: memory 1 GiB, caps `CHOWN DAC_OVERRIDE FOWNER SETUID SETGID` only, no published ports. `OOMKilled=false` on smoke, the green probe, and the measurement replay.

One a135 rust builder at a time. Host `cargo` / `rustc` / `rustdoc` / `rustup` count was 0 at each start. Slot B was not given an a135 overlay.

## Smoke, before probes

`smoke/worst.exit=0`, `lib.exit=0`, `clippy.exit=0`, `docker-run.exit=0`.

- `cargo test --locked --offline -p oo-connectors --lib -- --test-threads=1`: **57 passed, 0 failed**. Includes `model::tests::empty_catalog_fixture_matches_the_frozen_hash`.
- Empty catalog content sha256 `044022fcabb7587632133cb8f81c3665292c32acc7af3432b5bfe4d066679a3c`. Marker `empty_catalog_fixture=ok`.
- `cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings`: exit 0, finished in 12.07s.
- `sqlx-postgres` printed two unfulfilled `#[expect(dead_code)]` warnings (`message/flush.rs:11`, `message/mod.rs:76`). They are in the path dependency. The oo-connectors deny did not fail.

Author a133 recorded lib **55** on `79515fcf` plus the three-file overlay, which did not compile `stream.rs`. This archive's 57 includes `stream::tests::budget_bounds_are_closed` and `types::tests::export_arrow_fixture` along with the rest of the lib list in `smoke/lib.stdout`.

## Contract

`ReadBuf::init_mut` for `BytesMut` (`read_buf.rs:16-26`) zero-fills the spare with `put_bytes` and returns the whole slice, so `len == capacity`. `std::io::Read` then writes `n` bytes at index 0 and does not shrink the buffer.

Call sites:

- `tls_rustls.rs:50` `self.state.reader().read(buf.init_mut())`
- `rt/rt_async_io/socket.rs:14` `self.get_ref().read(buf.init_mut())`
- `rt/rt_tokio/socket.rs:14` and `:37` `try_read_buf`, which appends

`committed_init_len` (`buffered.rs:535-540`): if `pre_len >= len_now` or `len_now - pre_len <= n`, keep `len_now` (append, or the length did not grow past the write). Otherwise keep `n.max(pre_len)`. The unread prefix stays. The zero tail past that point is discarded. `read` applies this at `buffered.rs:585-592`, after `socket.read` returns and before EOF handling and `advance`.

`Socket::read` (`socket/mod.rs:75-80`) retries `WouldBlock` and returns any other error immediately. A reset after `init_mut` therefore skips the truncate. A cancel that drops the future before `read` returns also skips it (`buffered.rs:582-584`). The next successful short read is what truncates. Capacity is not shrunk by `truncate`, so the view meter still reports the live windows, and a reserve already noted is not refunded.

The 5-byte frame ask fits in the initial 8192 spare, so `reserved_additional` is still 0 at the moment that parked read is dropped. `init_mut` has already committed the spare. The next short read reserves once.

## Independent plateau

Overlay `overlay/a135_tail_probe.rs`, run from the private snapshot. Fuse `PLATEAU = 24 KiB`. `MESSAGE_CAP = 32`. `FRAMES = 512`. Frame type `D`, 2-byte body. Product blob and bytes lock were checked inside the test.

Measurement replay (`probe/measured/`, `--nocapture`), exits 0, `OOMKilled=false`. Same numbers the green gate run asserted, now printed:

| Probe | View peak | Reserve | Curve |
| --- | --- | --- | --- |
| 512 dropped, cap 32 | 8192 | 0 | `0:8192 … 511:8192` |
| 512 retained, cap 32 | 8192 | 0 | `0:8192 … 511:8192` |
| Caller-retained payload | 1024 bytes | | distinct from the 8192 view |
| Append peer | 8192 | | flat through frame 511 |
| 3-byte init and 3-byte append | 8192 / 8192 | | split-frame order held |
| Oversize header, cap 32 | 8192 | | logical peak 5, bytes read ≤ 5, body not read |
| EOF then 512 frames | 8192 | 0 at EOF | charge not rolled backward; later frames stay at 8192 |
| Parked 5-byte cancel, then 512 frames | 8192 at the drop, 16384 after the frames | 0 at the drop, 8192 after | one reserve, then the warmup fuse holds |
| 9000-byte parked cancel, no data | 16384 | 8192 | open reserve row, read not completed |
| Reset after `init_mut`, then 512 frames | 8192 at the error, 16384 after the frames | | no further doubling |

Product sha256 printed by the test: `b56fe942de40260bfaf39e83f32af8630ded0d739bc758480940099124cfe7d5`. Bytes version `1.12.1`, checksum `fc652a48c352aef3ea3aed32080501cf3ef6ed5da78602a020c991775b0aff04`.

The green gate (`probe/probe.exit=0`) ran the same nine unit tests without `--nocapture`: 9 passed, TLS filtered. Author `pg_wire_tls_tail` on this tree (`probe/author-tail.exit=0`): 8 passed, `tls_pg_sixty_thousand_small_rows` filtered. The author's 60000-row test was not repeated.

## TLS verify-full

Private PostgreSQL 17.10, `sslmode` verify-full, caller CA. 12000 rows. The host oracle (`probe/oracle.txt`, Python `hashlib`, not the Rust test):

- sum `72006000` (`n*(n+1)/2`)
- bit_xor `12000`
- row sha256 of big-endian `i32` ids `ec867603e6dd3cbddf278bc24c3b97d2ef1ddfe82509661190acec0804352d89`

Admin connection `oo-a135-admin` set `log_statement=none`, built `oo_a135.small`, and ran `ALTER ROLE oo_a135_reader SET log_statement=all`. That session was dropped before the reader connected. The reader role is not a superuser and did not issue `SET`.

Measurement replay, test `tls_verify_full_moderate_rows`, 0.27s, exit 0:

- rows 12000, per-row id equals `index+1`
- hash `ec867603e6dd3cbddf278bc24c3b97d2ef1ddfe82509661190acec0804352d89` (matches the oracle; the digest was hex-encoded once)
- admin aggregate count, sum, and bit_xor matched the same oracle before the reader connected; the reader repeated that aggregate after the row read
- view peak **8192** bytes
- RSS **8924** KiB
- caller-retained **48000** bytes (`12000 * 4`)
- those three integers are pairwise distinct
- view under the 1 MiB fuse, RSS under the 512 MiB fuse
- reader audit: 2 SQL lines (`app=oo-a135-reader`), 0 write-verb lines
- the test asserts 0 reader `SET` lines and that the log contains neither password; the test passed
- a scan of this evidence tree found no password bytes and no private-key marker

12000 rows already matched content, order, and the flat 8192 view. The 60000-row size was not required.

The first TLS attempt hashed the finished digest a second time (`aa227293…` versus oracle `ec867603…`) after the per-row ids had already matched. That failure is preserved under `probe/attempt2/`. It was the overlay, not the driver.

## Combined finite path, once

`probe/stream-live.exit=0` (1.32s) and `probe/stream-bounds.exit=0` (1.58s). `probe/worst.exit=0`. `probe/docker-run.exit=0`. `probe/blob-after.exit=0`. `buffered.rs` and `stream.rs` hashes matched before and after.

Live (`probe/stream-live-outcome.txt`):

- catalog max rows 128
- oracle before writer: parent 4, child 3, empty 0, zero 2, payload 59
- executes 13, reserved rows 13, consumed rows 9, decoded payload 59, oracle payload 59
- arrow preimage 132, arrow assembly 5528
- wire view capacity peak **8192**, wire accepted logical payload 12522, wire assembly 35
- parent batches 2, child batches 2, zero batches 1
- admin sees the late parent row
- finish, abort, and drop each released the reader backend
- SQL audit: 216 `oo-pg-source` / `oo_a131_reader` statement lines, 2 fixture-writer writes, 0 reader DML/DDL

Bounds (`probe/stream-bounds.stderr`):

- refuse wide value and refuse row, both backends released
- slow consumer idle on `ClientRead`; log line count unchanged across the idle sleep (327 to 327); one new statement line after the next demand
- slow consumer released
- SQL audit: 333 reader statement lines, 0 reader DML/DDL

The log volume `oo-a135-eef831aa-pglog` was mounted read-only at `/pglog` inside the rust container. The log was truncated before this green run, after a secret scan of the previous log returned no password hits. The bounds outcome file was not written beside the live outcome file; the `--nocapture` stderr is the record.

## Composition already covered by the author

This review did not re-execute the author's wire-bounds, accounting, or portal suites. Their raw exits on the patched `79515fcf` snapshot are 0:

- `evidence/...a133/SP03/init-tail/proof/bounds.exit`
- `accounting.exit`
- `portal.exit`
- `tail.exit`
- `view.exit`
- `pg-test.exit`
- `pg-worst.exit`
- `lib.exit`
- clippy: `proof/clippy-worst.exit` is 0. There is no `proof/clippy.exit`.

Author counts in that report: bounds 13, accounting 12, portal 6, plaintext view 1, tail 8 with the TLS test filtered, lib 55. Scoped clippy exit 0, with the same `sqlx-postgres` expect warnings.

This review's clippy `--all-targets` compiled those integration tests on the combined `bd77b252` tree. The executed composition check is the one `pg_stream_live` plus `pg_stream_bounds` pair above.

## Embedded sqlx-core

This review did not run an embedded stripped `sqlx-core`, and it did not run workspace `cargo test -p sqlx-core`.

`sqlx-core` is a path dependency, not a workspace member. `oo-connectors` enables `sqlx` features `runtime-tokio` and `postgres` only, and `sqlx-core` features `_rt-tokio` and `_tls-rustls-ring-no-roots` (empty root store, caller CA). `tls-rustls-ring-webpki`, `tls-rustls-ring-native-roots`, and `tls-native-tls` are not enabled. The behavioral checks here go through `BufferedSocket` / `ReadBuffer::read` and through `PgConnection` on the public driver.

a133's embedded run is theirs: an isolated copy, unused optional dependency tables removed, bytes pinned to 1.12.1, 4 tests passed. That run also produced 151 unexpected-cfg warnings from the emptied feature table. This review does not adopt that count as its own.

## Failures kept

| Run | Exit | What failed |
| --- | --- | --- |
| First unit probe | 101 | Overlay assert `cancel dropped the reserve charge` at `a135_tail_probe.rs:151`. `reserved_additional` was 0 because a 5-byte ask does not reserve. Eight other tests passed. The full console was later overwritten. The captured excerpt is `probe/attempt1/recovered-panic-excerpt.txt`. `probe/attempt1/NOTE.txt` records the overwrite. The files now sitting next to that note are the later unit-green compile (unused `BufMut` warning, `probe.exit` 0). |
| Second probe, TLS | 101 | `probe/attempt2/tls.exit`. Per-row ids matched. The overlay hashed the digest twice. |
| Second probe, live and bounds | 101 | `probe/attempt2/stream-live.exit` and `stream-bounds.exit`. Live had already printed view peak 8192, payload 59, and release lines. Bounds had already printed both refusals. Both then failed opening `/pglog/postgresql.log` because the rust container did not have the log volume. `worst.exit` 101. |
| Green probe | 0 | Log volume mounted. Digest hex-encoded once. All five phases 0. |
| Measurement replay | 0 | Unit and TLS only, `--nocapture`. Stream tests were not run again. |

## What this review did not do

No production edit, no manifest or lock edit, no spec or ledger edit, no push, no release, no docker prune, no foreign container or volume removal. No 8 GiB allocation. No full R3, R4, matrix, or license run. No second combined stream run. The author's `fetch_all` of 60000 rows was not re-executed; 12000 was enough.

`hash-manifest.sha256` in this directory lists every file under this evidence tree except itself. a133's manifest is 21 lines and is a selective author inventory, not this review's file list.
