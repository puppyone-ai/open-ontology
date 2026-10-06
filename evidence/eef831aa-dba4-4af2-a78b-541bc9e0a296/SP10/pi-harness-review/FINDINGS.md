# Findings

Verdict: **APPROVE_LIMITED_DEV_ADAPTER**. The full harness stays open. Author exits in the a287 packet are not this review's evidence.

## Frozen checks that held

The fixed-archive author suite passed before the overlay was added: 5 unit tests, 17 contract tests, and clippy `-D warnings`, all `--locked --offline` on Rust 1.94.0.

All 14 committed child registry triples match `tests/fixtures/registry-triples.txt` and are present in the fixed root `Cargo.lock` (373 registry packages). The author's lock test compares the child lock with the triples file only. This review also opened the root lock.

Source commands are `prompt`, `get_messages`, `switch_session`, and `abort`. `child.rs` refuses `export` and `restore` with `export and restore are not commands`. `lib.rs` and `live.rs` do not call `.command("export"` or `.command("restore"`.

`RunscApproval::Absent` and `RunscApproval::Presented` return `ErrorKind::Unsupported` before any stub file is created.

The independent stub tests covered monotonic sequences from 0, command-id matching, unknown events, command and turn deadlines, the 16-event bound, the 60-tool stub labeled `not_upstream_real`, stdout EOF, abort acknowledgement with a direct-child wait, and transcript round-trip through `get_messages` and `switch_session`.

The fresh Pi 1.0.4 process, with Node v22.23.3 and the local fixture provider, mapped the fixed bash tool turn through `TurnFinished`, left abort and stdout EOF without `TurnFinished`, and restored equal `get_messages` arrays from the session file. Pin hashes, the git tag MIT text, the npm integrity, and the published day 2026-10-05 are unchanged. Prototype age on 2026-10-06 was 1 day.

## Product limits, not failures of this unit

`export_transcript` and `restore` are harness methods. They are not child commands.

The frozen live driver keeps stdout from the first process only. `linux/pi-live-2/restore/raw.ndjson` contains `prompt` and `get_messages` and does not contain `switch_session`. `Harness::restore` still has to receive a successful `switch_session` with `cancelled` false before it requests `get_messages`. The live restore exit is 0 and the message arrays match. The stub command log is where `"type":"switch_session"` was asserted.

`--network none` on this Docker host creates kernel fallback devices (`gre0`, `sit0`, `tunl0`, and the other names in `linux/pi-live-2/net-ifaces.txt`) plus the `bonding_masters` sysfs file. In the live container those devices carried 0 packets. The IPv4 route table had no data rows. IPv6 routes named only `lo`. That is the observed none netns. It is not a network grant and not a sandbox certificate.

The published npm tarball has no LICENSE file. The git tag MIT text matches the pinned digest. This matches the earlier prototype reviews. The pin is not promoted.

`LocalProcessNotSandbox::acknowledge()` returns the statement `local-process-not-a-sandbox-certificate`. The statement does not certify a sandbox, an identity, or a proxy.

Docs/09 events outside this contract, production runsc, signed runtimes, auth, and the parent harness are outside this verdict. The plan checkbox stays the parent's.

## Preserved checker and test failures

These stay as written. They are not product defects.

`linux/preoverlay-msrv/source-oracle.stderr` is `review_oracle: lib refusal missing`. The refusal sentence is in `src/child.rs`. `linux/oracle-2` compiles the corrected checker. `linux/oracle-2/source-oracle.stdout` is `source_ok`.

`linux/overlay/test.stderr` is the three `expect_err` errors. `Harness` does not implement `Debug`. `linux/overlay/a296_boundary.failed.rs` is that source.

`linux/overlay-2/test.stdout` shows sequences `[0, 1, 2, 3]` against an expected `[0, 1, 2]`. The reorder stub emits `turn_start` before the matching response, then `write_turn` emits another `turn_start`, the delta, and `turn_end`. `linux/overlay-2/a296_boundary.failed.rs` is that source. Clippy on that run exited 0. `linux/overlay-3` expects the four events and passes. The acceptance id remains `c1`.

`linux/pi-live/live-oracle.stderr` lists the kernel fallback interfaces. Mode exits in that directory are 0. `linux/pi-live/review_oracle.compiled.rs` is the checker that required only `lo`. `linux/pi-live-2` records the route tables from the container that ran Pi. `linux/pi-live-oracle` accepts that evidence together with `NetworkMode=none`.
