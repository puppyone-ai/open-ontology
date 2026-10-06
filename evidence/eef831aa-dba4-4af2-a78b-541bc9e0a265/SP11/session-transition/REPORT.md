# Session transition report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a265`. Frozen unit RT-D01 / SP11-W01A. Fixed base `567d621e2e52b7e0011bdb025dc9337e92ecc5a6`. This directory and `tools/session-transition` are the owned paths. The parent development ledger was not edited.

The crate is a pure reducer. `transition` reads the caller-supplied state, verified identity tuple, expected owner generation, expected revision, event id, monotonic `now_ms`, and event. It returns the next state or the same state with a refusal. `database_used`, `auth_provider_used`, `runtime_used`, and `physical_release_certified` stay false.

## Decisive runs

Precode, before product Rust existed: `linux/precode`. `version.exit` 0, `archive.exit` 0, `absence.exit` 0, `probe-rustc.exit` 0, `probe-run.exit` 0, `docker-run.exit` 0. Probe stdout: `msrv_smoke_ok idle=1800000 absolute=28800000 turn=600000 tools=60`. Container `80d4c757439035260f1c1dae51d01df0798c2d1b5ef88b20dc6070e3573ca5b4`. Network mode `none`. Rust `1.94.0` `aarch64-unknown-linux-gnu`.

Final development run: `linux/impl-4`. `version.exit` 0, `lock.exit` `present`, `fetch.exit` 0, `test.exit` 0, `clippy-component.exit` 0, `clippy.exit` 0, `docker-run.exit` 0. Container `22d44184b738f387a8d5ae2b4b46516a9a84310f01e6387e8fe77633c76dac15`. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, OOMKilled false, privileged false.

Independent review: `linux/review-1`, fresh target volume `oo-a265-eef831aa-target-review`, sealed lock, `cp -R`, readonly `/src`. Same exits 0. Container `903956deba937579d8b19c95ecdfe1438878ebbd2bf5573058249b2103e8a5fa`.

`test.stdout` on both green runs:

- lib `sha256::tests::known_answers`: 1 passed
- bin: 0 tests
- `cli_roundtrip`: 2 passed
- `handcomputed`: 12 passed
- doctests: 0

Handcomputed names: `cancel_then_begin_turn_is_stopped`, `changed_actor_beats_stale_and_keeps_workspace`, `full_journal_does_not_drop_and_still_replays`, `happy_path_revisions_are_hand_counted`, `identical_replay_is_idempotent_and_conflict_refuses`, `overflow_does_not_wrap`, `reason_names_and_bounds`, `releasing_stops_admission_and_close_needs_s8_absence`, `stale_revision_and_generation_leave_the_snapshot`, `takeover_heartbeat_and_turn_timeout`, `time_boundaries_refuse_at_the_exact_instant`, `tool_sixty_is_the_last_admitted_start`. The batch finished in 1.80s. Clippy stdout is empty. The command was `cargo clippy --locked --offline --all-targets -- -D warnings`.

## Pins

Direct dependencies are `serde = "=1.0.229"` with `derive` and `serde_json = "=1.0.151"`. `linux/review-1/lock-pins.txt` records:

- serde 1.0.229 `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba`
- serde_core 1.0.229 `67dca2c9c51e58a4791a4b1ed58308b39c64224d349a935ab5039aa360942a48`
- serde_derive 1.0.229 `e7a5d71263a5a7d47b41f6b3f06ba276f10cc18b0931f1799f710578e2309348`
- serde_json 1.0.151 `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14`

Those four checksums match the repository root lock. `Cargo.lock` SHA-256 `9cb340c0cbbc4b0cb561c05796e72702d0714eee16b7515f0e2f3600d58c6adc` is the same in the crate and in `linux/review-1/Cargo.lock`. Root `Cargo.toml` and `Cargo.lock` were not edited. Transitive crates are the ones those pins require: itoa 1.0.18, memchr 2.8.3, proc-macro2 1.0.107, quote 1.0.47, syn 3.0.6, unicode-ident 1.0.26, zmij 1.0.23.

Source blob hashes match `linux/review-1/source-sha256.txt`.

## Budget

Development builders used: 5. Precode, `linux/impl`, `linux/impl-2`, `linux/impl-3`, `linux/impl-4`. Review builders: `linux/review-1`, then the manifest container. Each run recorded `own=0 global=0 host_cargo=0` before start. One builder, 6 GiB, 1 CPU, 0 PostgreSQL. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

## Contract behavior covered by the tests

Phases exercised: requested, binding, ready, running, idle, releasing, closed. Happy path revisions are 1 through 6. A mismatched expected revision or generation returns `stale` and leaves the snapshot. An identical event id and fingerprint returns `idempotent` even when `expected_revision` is still the pre-image. A reused id with a different owner or principal returns `event_id_conflict`. A changed project, principal, role, claims hash, policy, knowledge, or config returns `new_session_required` and keeps `ws-1`. Idle at 1800000, absolute at 28800000, and turn at 600000 refuse. One millisecond earlier admits. The 60th tool start is revision 11 with `tool_starts` 60. The next start is `tool_boundary` and the count stays 60. Takeover at `u64::MAX` is `generation_overflow`. Heartbeat at revision `u64::MAX` is `revision_overflow`. Takeover one millisecond early is `lease_held`. Takeover at the expiry instant moves generation 1 to 2, enters releasing with `owner_lost`, and clears tools. Heartbeat leaves `last_activity_at_ms` unchanged. Turn timeout at 600000 with an open tool ends in idle, `timed_out`, failure none. Begin turn while releasing is `admission_stopped`. Close requires every S1–S11 record. S8 with `actual_absence` false is `s8_not_absent`. A matching close reaches phase closed, revision 5, `physical_release_certified` false. A recorded S8 that differs from the completion payload is `release_mismatch`. A full journal of 1024 entries refuses `h-extra` with `dedup_exhausted`, keeps `e0`, and still replays `e0`. A supplied journal of 1025 entries is `state_invalid`.

CLI: begin bind exit 0, stdout revision 1. Resubmit of that state with the same event id and `expected_revision` 0 is `idempotent`, revision 1. A fresh requested state with `expected_revision` 7 exits 2 and stderr `session-transition: refused stale`, stdout revision 0. Trailing JSON exits 1, outcome `malformed`, and the stdout object has no revision field. `--help` exits 1.

## Limits

The journal cap is 1024. An eight-hour session with turn and tool traffic can exceed that cap. Exhaustion returns `dedup_exhausted` and leaves the journal unchanged. A later release event that was never journaled fails the same way. Replay of an id already in the journal still succeeds.

The lease window is at most 30000 ms. Boundary equality is a refusal. `now_ms` moving backward is `time_backward` before any replay match. Release completion stores the caller’s S1–S11 records and the caller’s S8 absence flag. `physical_release_certified` stays false. The reducer has no database connection, authenticator, or runtime.
