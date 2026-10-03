# HANDOFF — a132 receive-view shrink review

**APPROVE LIMITED** for the panic removal and the meter adjustment in `79515fcf7e7032c8c212540eaae4b48a5957e3a2`, tree `b78f84497484f6232aecf0b51ffcc6d27be9fd7c`.

The production stream integration gate stays closed. Four `init_mut` reads, 252 bytes, bytes 1.12.1, raised the live view from 8192 to 32643 with frames dropped and to 40706 with frames retained. A message cap of 32 left the dropped-frame peak at 32643. An append-shaped reader stayed at 8192. Declared length 100000 was rejected at logical need 5 with the view still 8192. The 512-frame 8 GiB kill in a129 was not repeated. The embedded test's 128-frame bound is not acceptance of the stream.

a131 stays on the frozen baseline until a later review opens this gate. The 60000-row catalog read and a TLS production read were not run.

## Read first

1. `FINDINGS.md` — meter lines, the 4-read table, the cap comparison, and the truncate proposal.
2. `REPORT.md` — image, toolchain, exits, including the first 101s, and the gate.
3. `console.log` — bounds 13, accounting 12, portal 6, and `PROBE_BEGIN` through `PROBE_END`.
4. `smoke-rerun-console.log` — lib 55 passed after the empty-catalog fixture was restored to the archive.
5. `clippy-rerun-console.log` — scoped clippy exit 0. `clippy-rerun-login-miss.log` is the PATH miss, exit 127.
6. `smoke-rerun-reason.txt` — why the first `lib.exit` and `clippy.exit` are 101. `OOMKilled=false` on every container.
7. `buffered-copy-after.txt` — the probed `buffered.rs` matched the archive. bytes moved from 1.11.1 in the package lock to 1.12.1 by the pin. That run is not `cargo test -p sqlx-core`.

## Fix proposal, not applied

In `ReadBuffer::read`, after `socket.read` returns `n`, truncate `available` to its pre-read length plus `n` when `init_mut` has committed a longer tail. Do that on EOF as well. Then repeat a bounded `init_mut` proof, cap set, frames dropped and frames retained, and show the live view stops doubling. The message cap does not do that job.

## Cleanup

`cleanup.txt`: network `oo-a132-eef831aa-net` and volumes `oo-a132-eef831aa-cargo`, `oo-a132-eef831aa-target` removed. `/tmp/oo-a132-eef831aa` removed. No `docker prune`. a131's exited container was left in place.
