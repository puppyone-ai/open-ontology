# Findings

Verdict: **VERIFICATION_REPAIR_PRESENT**.

Same-session reading of `testing/spikes/p09-pi-rpc/src/drive.rs` and this evidence directory. This is not an independent review and it does not accept H-D01. The parent checklist was not edited.

## Clippy

The pinned image does not ship clippy. a279 recorded `clippy.exit` 127 because `--network none` blocked `rustup component add`, and the script wrote that status without a clippy binary. This repair installed the component on a private bridge, then ran the real command.

`linux/clippy-bridge/clippy.stderr` is the concrete lint:

`src/drive.rs:237` `clippy::collapsible_if`, denied by `-D warnings`. `clippy.exit` is 101. `clippy-install.exit` is 0. The components-after list contains `clippy-aarch64-unknown-linux-gnu`.

The edit collapses the nested `if` into `if let Ok(mut guard) = slot.lock() && guard.len() < 262_144`. The failure to lock still skips the write. A buffer that has already reached 262144 bytes still skips the write. The room calculation is unchanged.

`linux/clippy-after-1/clippy.exit` is 0. stderr ends `Finished dev profile`. stdout is empty. `test.exit` is 0: `ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s`.

The pre-edit smoke, on the unmodified archive of `16cd808d`, was the same 12 tests at Rust 1.94.0, `test.exit` 0, finished in 0.45s. That container did not have clippy and did not pretend to.

## Live behavior

`drain_stderr` is linked into `pi-rpc-live`, so the live driver was run again on Pi 1.0.4 commit `7c10bd4337495ee613f2224843ecdf349b80d1df`. Container `bef1bf9cafeb47dcb6b1d5455362ba2d02ed83c68512d7a6dffc460276322b85`. `NetworkMode=none`. `live.exit` 0. `live.stdout` is `live_ok`.

The abort transcript still has assistant text `fixture-reply`, `stopReason` `aborted`, and `errorMessage` `Request was aborted`. The session file still has `stopReason` `stop` and does not contain `public-dummy-fixture-token`. `switch_session` still returns `success` true and `cancelled` false. `export` and `restore` are still unknown commands. Those four response files match the frozen fixtures byte for byte.

The bare `pi --version` before `cargo run` exited 127 because that shell did not put Node on `PATH`. The driver process did, and its version check passed. That 127 is kept in `live/pi-version-pre.exit`.

## Hashes

Host `shasum` was not used for this inventory. `sha256_manifest` compiled with `rustc --edition 2024 -D warnings` and checked the empty digest and `abc`. All 473 lines of the a279 manifest match the fresh digests. All 25 lines of the a279 source inventory match the fixed commit archive. The edited tree differs only at `./src/drive.rs`.

The Node and npm pin script still uses `sha256sum` and `openssl`, because that is the reviewed install script. Those checks are in `linux/pi-reinstall`. They are not the evidence manifest.

## Left open

H-D01 is still pending parent acceptance. The spike does not implement the async harness trait. Tool, clarification, and artifact events were not part of this repair. v1.0.4 remains a prototype pin under the seven-day floor. No defect was found in the rerun relative to `CONTRACT.md`.
