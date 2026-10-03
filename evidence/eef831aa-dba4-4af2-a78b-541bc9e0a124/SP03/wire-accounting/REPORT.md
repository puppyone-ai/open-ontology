# Receive allocation accounting repair

Author measurement for rereview. This is not an independent acceptance, and it is not an R2-W checkbox.

Commit `ff783ac933b86e0ff068181afe0258764c5d2e5d`, tree `3a5554c7e8ff09555924675b60cb287155398caf`, parent `1150151bf82a1c72997b1c3aaacb306d47e16cd0`. Subject `driver: repair receive allocation accounting`. Six paths. `sqlx-postgres` `src/lib.rs` blob `a6ce2f3a54f6712249030dacb38a7fceaa9722eb` is unchanged, because the new fields sit on types that file already reexports.

Fixed input named by the card: commit `8c8e24a0a6c098fe34ff43cd5f98f4a3549a6117`, tree `16c3fa867bbb41e836864b692f2d65411c566b45`, parent `fef1eafa95dc70b1744107eaced13629ac75a669`. Product bounds commit `a90c86b405febebfdc70f8169be42dc4de4c3dfc`. Coordination commits between that product commit and this parent do not change `crates/`, `Cargo.toml`, `Cargo.lock`, or `rust-toolchain.toml`.

## Sequence

The baseline container started from a `git archive` of `57f6003239ceb375722c5b67aa36647a0a2a1e7f` (tree `ed9363157c7061db8d419af588a4a2e0abb2e6c8`, parent `ac029a210fbf05840842ebd3ff120448f68a2588`). `linux/baseline/inside.sh` exits 2 if `pg_wire_accounting.rs` is already in that archive. `snapshot.exit` is 0, so the archive did not contain the new test.

Working-tree edits to the meter began while that container was still fetching. The archive was already sealed, so the baseline result is the unfixed tree. It is not a smoke that finished before the edits, and it did not run `pg_wire_bounds` or `pg_wire_accounting`.

Baseline then finished. Fetch, library, and Clippy exited 0. Elapsed 41s inside the container.

The final container archived the same `57f60032` and copied the six owned working-tree files onto it. Host `shasum -a 256 -c` of `linux/final/snapshot-files.sha256` matched those files, and `sha256sum -c` inside the container exited 0 before the build (`snapshot.exit` 0). After the container exited, the same manifest still matched the workspace. Those bytes are the commit above. Coordination commits `cc7a8497` and `1150151b` landed after the overlay and do not touch `crates/`.

No cargo phase failed. No second RED oracle was run. `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a122/SP03/wire-bounds-review/` is unchanged, including the 12-pass/2-fail probe.

## Identity

| Path | Git blob | sha256 |
| --- | --- | --- |
| `crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs` | `f41e0e4dd6baca062fb332ab66da3f5e9c0600fd` | `24b22aa55ecfd1f739fa87a0e95b86bd3f4ab7d4f26305fd51eda85f73476687` |
| `crates/oo-connectors/vendor/sqlx-postgres/src/connection/stream.rs` | `2089b5d675e2d8811334d187f8675407ce7a8d89` | `583c0aa7ffba55726f9c74a4d2275966c5e8d5444ad076dfcb48259a11a96981` |
| `crates/oo-connectors/vendor/sqlx-postgres/src/connection/mod.rs` | `4c2bb3cb45ce330fdac4c0a37294c96644fe41df` | `a2057da12adbf73d83e20282bdcea21ce373d1b0c00f63595ace2124710afdf5` |
| `crates/oo-connectors/vendor/PATCH.md` | `fb933d000c88289075a0a81d634780159f4e6743` | `24ea8ccbc3e3596b76891401bffecbd2679ca7ebaff949a2218dc5454c4c3720` |
| `crates/oo-connectors/tests/pg_wire_bounds.rs` | `c27e7df9ff875ea7022c1bfa9cebceea3caed0fd` | `d234aa8b51caf746a5c1f4de85fbd560e9652af7a6b7e5badfdefbd6bf0697d8` |
| `crates/oo-connectors/tests/pg_wire_accounting.rs` | `93b2f85ff041dfec638a16656e94b200c76d2251` | `404c6a7b9cb4579aeb6e1ce27dc2696fcfef144109201bc15111e6103a5595cb` |

`linux/final/snapshot-files.sha256` is the `sha256sum -c` manifest of those six files.

## Runs

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, local inspect `os=linux arch=arm64`, `--pull never`. One builder at a time. Bridge `oo-a124-eef831aa-net`, volumes `oo-a124-eef831aa-cargo` and `oo-a124-eef831aa-target`. HostConfig for the final container: privileged false, CapDrop ALL, no-new-privileges, memory and swap 8589934592, NanoCpus 4000000000, PidsLimit 4096, OOMKilled false, network `oo-a124-eef831aa-net`, no socket mount, HOME not mounted. Capacity before the final start: running rust builders 0, own builders 0, host cargo/rustc/rustdoc/rustup 0.

`rust-toolchain.toml` requests channel 1.98.1. Both containers set `RUSTUP_TOOLCHAIN=1.94.0` and refuse any other rustc. Recorded toolchain: rustc 1.94.0 (`4a4ef493e` 2026-03-02), host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, cargo 1.94.0 (`85eff7c80` 2026-01-15). `airgap_claim=no`. The baseline `cargo fetch --locked` filled the cargo volume. The final fetch was `--locked --offline`.

| Phase | Baseline exit | Final exit |
| --- | --- | --- |
| toolchain | 0 | 0 |
| snapshot | 0 | 0 |
| clippy component | 0 | 0 |
| fetch | 0 (`--locked`, index updated) | 0 (`--locked --offline`) |
| `cargo test -p oo-connectors --lib` | 0, 49 passed, 0.00s | 0, 49 passed, 0.00s |
| `cargo test --test pg_wire_bounds -- --test-threads=1` | not run | 0, 13 passed, 6.08s |
| `cargo test --test pg_wire_accounting -- --test-threads=1` | not run | 0, 12 passed, 4.56s |
| `cargo clippy -p oo-connectors --all-targets -- -D warnings` | 0 | 0 |
| docker run | 0 | 0 |
| inspect | 0 | 0 |

Baseline container seconds 41. Final container seconds 23. Raw logs: `linux/baseline/console.log`, `linux/final/console.log`.

`cargo clippy -p oo-connectors` lints that package, including the integration tests. It compiles the vendored sqlx crates and does not apply their own Clippy deny lints as a separate phase.

## What was measured

`BytesMut::reserve` receives the deficit beyond `len` when that deficit is greater than spare (`capacity - len`). On the 12000-byte header-first frame the growth event recorded `reserve_argument=11995`, `spare_before=8172`, `reserved_growth=8192`, `view_before=8177`, `view_after=16369`, `rounding_peak=4369`. The argument and the growth are different numbers. While the body read was still parked, `bytes_read` was 20 (15-byte handshake plus the 5-byte header) and `reserved_additional` was already 8192. The retry left `reserved_additional` at 8192 and finished at `bytes_read=12015`.

The 20011-byte one-shot CommandComplete recorded `reserve_argument=11834`, `reserved_growth=11834`, `rounding_peak=0`, `assembly_bytes=0`, `read_len_before=8177`, `bytes_returned=11834`, `available_capacity_before=0`, `available_capacity_after=11834`. The same logical size as a DataRow in `pg_wire_bounds` recorded `assembly_bytes=8177` and the same growth 11834. 8177 is the buffered prefix. 0 means that run's pointer did not move. Neither figure is a second copy of the 20011-byte frame.

Twenty-four retained 1000-byte bodies sum to 24000 content bytes and 24120 wire bytes. `accepted_backing_peak` and `view_capacity_peak` stayed 8192. `accepted_logical_payload` was 24005. `reserved_additional` was 16095 across `reserve_calls=2`, with `reserve_argument_bytes=1721`, `rounding_peak=7187`, and `assembly_bytes=289`. The 8192 figures are one live window. The 16095 figure is the sum of live-window increases. Neither is the sum of the retained frames, and neither is RSS.

The event log stopped at 64 rows. `events_dropped` went from 1 to 4 after three more 6-byte frames, `trace_bytes` stayed 6656, and `messages` and `bytes_read` kept moving. A declared length of 2000000000 was rejected at `last_logical_need=5` with no change in `reserved_additional` or `reserve_argument_bytes`.

Details, the 12 accounting tests, and the branches this run did not execute are in `FINDINGS.md`.
