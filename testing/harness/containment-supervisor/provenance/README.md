# Containment supervisor artifact

This directory records the prebuilt helper that `xtask` executes. The build is an engineering artifact build. `xtask` does not run `cargo`, `rustc`, or a C compiler to start a harness command.

| Field | Value |
| --- | --- |
| Package | `containment-supervisor` 0.1.0 |
| Locator | `testing/harness/containment-supervisor/bin/containment-supervisor` |
| Digest | `provenance/artifact.sha256`, one lowercase sha256 line. The same digest is embedded in `crates/xtask/src/containment.rs` as `SUPERVISOR_SHA256`. |
| Platform | `aarch64-unknown-linux-gnu`, Rust 1.94.0. The checked-in file is that Linux binary. |
| Trust | Regular file, not a symlink. Mode is owner-executable and not group or world writable. Uid is 0 or the caller's euid. The parent directory is not world-writable unless it is sticky. `xtask` checks this on every spawn. There is no environment path and no compiler allowlist. |
| Private copy | Before exec, `xtask` copies the verified bytes to a 0700 file under `CARGO_TARGET_DIR` or the directory of its own executable, re-hashes that copy, and executes the copy. `/tmp` is not used. The copy is removed after the helper returns. |
| TOCTOU | A same-uid writer can replace the copy after the second hash and before `exec`. The check does not close that window. A different uid, a symlink, or a group-writable file is rejected. |
| macOS | `xtask` returns `containment=unavailable` and `note=platform` without executing this Linux binary. A host-built helper would do the same until a supported ownership primitive exists. |

`artifact.sha256` is the pin. A missing file, a digest mismatch, or a failed ownership check does not spawn the payload and cannot satisfy a gate.

The helper's `Cargo.lock` is its own lock. It is not the product workspace lock. License texts for the resolved crates are in `licenses/`.
