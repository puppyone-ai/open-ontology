# Findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a387`. These are author findings. They are not an independent acceptance.

## Production is unsigned

`PRODUCTION_SIGNED` is false. `DEPLOYMENT` is `localprocess`. Profile `production` checks `/usr/local/bin/runsc` and `/usr/bin/runsc` as files and does not execute them. Missing `runsc` refuses with `runtime_unavailable`. A present `runsc` still refuses with `production_driver_absent`, because this crate has no production driver. The live test observed exit 2 and no listening port. A localprocess close is the supported dev close this card can show. It is not a production signature.

## The reducer flag stays false

`physical_release_certified` on the session-transition state stays false after `CompleteRelease` in this flow. `CloseReceipt::acceptable` requires that flag to be false, phase `closed`, deployment `localprocess`, and eleven legal steps. If a transition receipt or the reducer state ever reports the flag true, `apply` returns incomplete `unexpected_certification` and writes checkpoint marker `certification_denied`. The live receipt asserted the flag false.

## A releasing body is not a close

`fixture_stage` persists S1–S4 and S8 through `session_release::run` and returns disposition `fixture_persisted`, phase `releasing`, retryable true. `acceptable()` is false. HTTP maps that to 409. The restart half of the live test calls `fixture_stage` in-process and only then lets HTTP finish. The 200 happens after `finish_stage` records S5–S7 and S9–S11 and `CompleteRelease` leaves phase `closed`.

## Unproven absence cannot close

`finish_stage` refuses a releasing session whose S1–S4 and S8 are not all confirmed. Reason: `unproven_resume`. It does not call `session_release::run` again. Resume inside session-release treats a live recorded pid as `harness_not_owned` and a missing pid as unproven absence; this crate does not paper over that. S8 `actual_absence` is legal only after workspace `symlink_metadata` is `NotFound` and `/proc/{pid}` is `NotFound`. If `/proc` cannot be read, the close stops. S5 `Skipped` is recorded only when the snapshot path is `NotFound`; a present snapshot returns 409 `snapshot_present_uncollected`. The live test hit that 409, deleted the file, and then closed.

Cancel of a ready session with no backend pid returns 409 `backend_absent`. The acceptance path sends a turn before cancel, so the backend `pg_sleep` and the harness pid both exist before release.

## Step order versus the reducer journal

Spec order records S6 and S7 before collect. `session_release::run` collects inside the call and exposes no hook. This crate revokes the gateway token, marks the owned proxy mapping deleted, and releases the owned permit before `run`. It records the reducer steps after the fixture returns exit 0 and phase `releasing`. The journal timestamps for S6, S7, and S10 are therefore after collect. The side effects are before collect. FINDINGS records that split so a reader does not treat the journal order as the side-effect order.

The token file is deleted only after a fresh lookup returns `Revoked` and `CompleteRelease` has succeeded. Replay may find the file already gone.

## Lockfile exception

The card forbids `cargo update`, a generated root lock, and root manifest edits. None of those happened. This crate is a new workspace under `tools/` and had no `Cargo.lock` on attempt 1. `linux/impl/inside.sh` ran `cargo generate-lockfile --offline` once. `attempt1/lock-offline.exit` is 0. Later runs saw `lock_present` and did not generate again. `tests/lock_subset.rs` requires every registry pin in this lock to match the session-store ∪ gateway-token-store checksum and the root `Cargo.lock` checksum, and requires session-release pins to already be in that union. Direct `Cargo.toml` dependencies on `axum`, `sha2`, and `ring` are rejected. Transitive lock package names from gateway-token-store are not a direct-dependency failure. Root `Cargo.toml` and root `Cargo.lock` were not edited.

## Clippy component

The pinned Rust image does not ship clippy. Attempt 1 clippy exit 1: `cargo-clippy is not installed for the toolchain 1.94.0-aarch64-unknown-linux-gnu`. Later `inside.sh` runs `rustup component add clippy` inside the disposable container when the component is missing. `clippy-component.stderr` on the final run is `info: downloading component clippy`, exit 0. The component is not installed on the host. Each new container downloads it again because rustup state is in the container layer. The final `cargo clippy --locked --offline --all-targets --keep-going -- -D warnings` exited 0. Package lints allow `too_many_arguments`, `cognitive_complexity`, and `uninlined_format_args`. `dbg_macro`, `todo`, and `unimplemented` stay denied. One contract test allows `assertions_on_constants` because the constants `PRODUCTION_SIGNED`, `DEPLOYMENT`, and `CONTRACT` are the assertion.

## Resources

One builder at a time, name `oo-a387-api`, 6 GiB, 1 CPU, CapDrop ALL, no new privileges, `HOME=/tmp`, private cargo and target volumes. One private PostgreSQL 17.10, 1 GiB, 1 CPU, no host port. Both were removed after the green run. This observation found no `oo-a387` container, volume, or network, and no host `cargo`. `/tmp/oo-a387-draft` was a partial source draft from before the crate existed; it held no private key. It was removed while writing this record. Foreign PostgreSQL containers were left running.

## Scope left with the producers

`session-release` still never sends `CompleteRelease` and still reports `local_fixture_not_certified`. This card calls it and then finishes in the owned adapter. Gateway token revoke is `gateway-token-store`, not a second token store. Session rows are `session-store` plus `session-transition`. Control-identity remains management OIDC and is not an agent session.
