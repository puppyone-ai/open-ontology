# Findings

## Preserved failures

`linux/impl` exited before compile. `version.exit` 0, `docker-run.exit` 1. `console.log` ends with `cp: setting permissions for '/work/src': Permission denied` and the same line for `/work/tests`. The launch script ran `chmod -R a-w` on the archive, and `inside.sh` used `cp -a` onto the virtiofs `/work` mount. Container `a37c3a10614db13d1252e00e36019d777060a3d69a32b0015c2d4cbb582811b9`. No `Cargo.lock` was produced. The scripts and console log were left as they ran.

`linux/impl-2` compiled. `lock.exit` 0, `fetch.exit` 0, `test.exit` 101, `clippy.exit` 101, `docker-run.exit` 101. Container `715297750fd77addd970b25734bcf73b15db2ed343fa1e68ecc7f35710a5fef9`. `tests/cli_roundtrip.rs` used `env!("CARGO_BIN_EXE_session_transition")`. Cargo on this image did not define that underscore name while compiling the integration test. Clippy rejected three nested `if` statements in `src/reduce.rs` (`collapsible_if` at the bound-tuple check, the workspace token check, and the release mismatch check) and the `0..16` index loop in `src/sha256.rs` (`needless_range_loop`).

`linux/impl-3` ran the tests. `test.exit` 0 with the same 15 passing tests as the final run. `clippy.exit` 101, `docker-run.exit` 101. Container `916daba2fe3d50a87cc0b07cb196ba9d5b9b78864aba9e71e4f2a7847ff64e46`. The only Clippy error was unused `now` on `bind_at` in `tests/handcomputed.rs`. The event stores the lease, and the caller already passes `now_ms` separately.

## Fixes that the green runs include

`linux/impl-2` and later copy with `cp -R` and do not clear archive write bits. Readonly `/src` is the seal.

The CLI test resolves `option_env!("CARGO_BIN_EXE_session-transition")` and then the underscore form. The hyphen form is the one this Cargo provides. Both CLI tests passed.

The three `if` blocks are `if let` chains. SHA-256 fills the first 16 words through `iter_mut().take(16).enumerate()`. `bind_at` takes only the lease. `CompleteRelease.steps` is `Box<ReleaseSteps>` so the event enum stays within Clippy’s size lint. JSON is still `{"type":"complete_release","steps":...}`. `ApplyError` carries a refusal or generation overflow, and the `SessionState` moves in the `Ok` path.

`linux/impl-4` and `linux/review-1` then passed tests and `cargo clippy --locked --offline --all-targets -- -D warnings`.

## Behavior notes

Decision order is the contract order. Time moving backward is judged before the journal, so a replay with an earlier `now_ms` is `time_backward`. A journal hit is judged before the tuple check, so the same event id with a different principal is `event_id_conflict`. A new id whose bound tuple differs is `new_session_required` before the CAS compare, so a wrong expected revision does not hide the tuple change. Idempotent replay ignores `expected_revision`.

Generation overflow is detected in `begin_bind` and `takeover`. If the journal is already at 1024, that path reports `dedup_exhausted`. The handcomputed overflow fixture has an empty journal, so it reports `generation_overflow`. Revision overflow is checked after the journal cap.

`Reason::S8NotAbsent` serializes as `s8_not_absent`. The test asserts that string.

A valid requested state has generation 0, so `begin_bind` reaches `generation_overflow` only if a caller supplied a state the validator would already reject. The reachable overflow in the tests is takeover at `u64::MAX`.

The 1024-entry journal is a fail-closed precursor limit. It is smaller than an eight-hour trace that also records turns and 60 tool calls per turn. The reducer does not drop entries to make room.

S8 `actual_absence` is stored from the caller. The output flag `physical_release_certified` stays false on the closed state. This crate’s CAS compare uses the numbers in the supplied state. It does not talk to PostgreSQL.
