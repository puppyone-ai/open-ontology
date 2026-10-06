# Findings

## Preserved harness failure

`linux/proof` `docker-run.exit` is 2. `console.log` records `PHASE lock_subset_exit=1`, `registry triple list drifted`, and `registry summary is not 11/0/0`, then `worst_phase_exit=2`.

`lock-subset.stderr` is one line: `lock_subset: oo-analysis 0.1.0 has no registry source`. The first `tools/lock_subset.rs` rejected every package that lacked a registry source, except the local package name `session-transition`. The root `Cargo.lock` at `9951cda7f128e30f1f6ce02e409d542d20a9e4fb` contains workspace path packages. `oo-analysis` 0.1.0 is one of them. Parsing stopped, stdout was empty, and the oracle compare exited 2.

The script continued. Offline fetch on the empty private cargo volume exited 101. Online fetch exited 0. The 15 author tests exited 0 before the overlay copy. The 11 overlay tests exited 0. Clippy `-D warnings` exited 0. `product-unchanged.exit` is 0. Those files were not rewritten.

`linux/lockcheck` runs the corrected parser on a fresh archive. A root lock skips a package whose source is not `registry+`. A crate lock still rejects a non-registry package other than `session-transition`. `lock-subset.exit` 0. The stdout is the 11 triples in `oracle/registry-triples.txt` plus `summary registry_triples=11 missing=0 mismatch=0 direct_pins_present=true`. The tool fails the run when the registry count is 4 or fewer, when any triple is missing or mismatched, or when a registry package has no checksum. Container `1abf212136a4115c291ba5736b0f22109cfab82d179135e2accc2b8a51023631`, `docker-run.exit` 0, network `none`.

The first parser source was replaced in `tools/lock_subset.rs` before that second container. The failed behavior remains in `linux/proof/lock-subset.stderr` and the proof exit files.

## What the runs showed about the crate

No author test and no overlay test failed. Clippy stdout is empty.

`sha256_known_vectors` passed the empty digest, `abc`, the 56-byte NIST message, the 112-byte NIST message, one million `a` bytes, and the begin-bind fingerprint preimage. The fingerprint preimage is the compact JSON object with fields `event_id`, `identity`, `event` in that order. `now_ms`, the expected numbers, and the state are not in that preimage. The product hash of that literal matched `a0bab5c228e31c54a6bb0b10cb29666a891fc089ef098fe344b3640866f16a27`, which is also the journal hash on the applied begin bind.

The compare that the overlay calls stale CAS reads `expected_revision` and `expected_owner_generation` against the supplied state. A mismatch returns `stale` and the state bytes are unchanged. `database_used` is false. There is no database client in the crate.

`identity_is_not_authentication` applies begin bind for principal `not-a-directory-user` with a `claims_hash` of 64 zeroes. `auth_provider_used` is false. An extra `signature` field is rejected by `deny_unknown_fields`. A role change is `new_session_required` even when the expected revision is also wrong, and the workspace id stays `ws-1`.

`release_is_caller_declared_and_closed_is_immutable` stores the caller's S8 `actual_absence` true and sets `physical_release_certified` false on the closed output. S8 false is `s8_not_absent` and does not close. After close, begin turn, cancel, heartbeat, takeover, record release step, complete release, and tool start are `closed`. Replay of the closing event id is `idempotent`.

`full_journal_refuses_release` puts 1024 entries on a releasing snapshot. S8 false is `s8_not_absent`. A complete release whose steps would otherwise close is `dedup_exhausted`. The journal stays at 1024 and the release steps stay empty. `physical_release_certified` stays false.

`now_ms` below `last_now_ms` is `time_backward` for an event id that is already journaled. Equality is allowed. Heartbeat does not move `last_activity_at_ms`. Turn timeout is not what the overlay used for the 600000 boundary; that case is `turn_boundary` on a tool start. The author's `takeover_heartbeat_and_turn_timeout` remains in the 15-test smoke.

A supplied lease already stored on a snapshot is not re-measured by these tests when the event does not set a new expiry. The 30000 ms window is asserted where the overlay sets a new lease, including the heartbeat at revision `u64::MAX` with expiry `30_001`.

## Image contents that were not used as an oracle

`linux/proof/python-present.flag` is 1. The proof script detected `python` with `command -v`. No Python process hashed data or produced an expected value. The SHA-256 checks are the product function and the Rust overlay literals.

## Product tree

`product-unchanged.exit` is 0. The sealed `Cargo.lock` was compared in the container and was not written back. Root `Cargo.lock` and `tools/session-transition` were not part of the owned edit set.
