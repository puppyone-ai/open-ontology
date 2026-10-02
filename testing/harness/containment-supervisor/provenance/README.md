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

## License evidence closure

Session `eef831aa-dba4-4af2-a78b-541bc9e0a105` added unaudited dependency records for the five registry crates in this helper lock. `audited` is false. `license_record_status` is `complete-for-review`. `admission`, `release_signoff`, and `full_at_sec_05` are false. The records do not close AT-SEC-05.

Canonical upstream bytes live under `licenses/<package>-<version>/<original basename>`. The flat files already in `licenses/` were left as they were.

| Package | Expression | `dual_license_choice` |
| --- | --- | --- |
| libc 0.2.189 | `MIT OR Apache-2.0` | MIT |
| bitflags 2.13.2 | `MIT OR Apache-2.0` | MIT |
| cfg-if 1.0.5 | `MIT OR Apache-2.0` | MIT |
| nix 0.31.3 | MIT | null |
| cfg_aliases 0.2.2 | MIT | null |

The MIT choice on an OR expression keeps the Apache-2.0 file and the Apache-2.0 obligations beside the MIT file. cfg_aliases keeps `NOTICES.md` and a byte-identical `NOTICE`. The `NOTICE` name is the locator. The copy does not change the Katharos copyright or the `tectonic_cfg_support` MIT notice. See `licenses/cfg_aliases-0.2.2/PROVENANCE.md`.

The helper manifest license is Apache-2.0. That first-party license is not one of the five records.

`testing/harness/helper-evidence-check` binds each record to the helper lock, `ci/deny.toml`, the canonical license bytes, and the retained a101 cargo-audit 0.22.2 / cargo-deny 0.20.2 outputs. Those outputs are the historical snapshot at RustSec `117edb3bed98e9be112f277b7615eea3252e7c43` (`2026-10-02T10:58:33+02:00`), recorded at `2026-10-02T12:12:14Z`. The snapshot is a historical scan.

Open gates remain open: named owner, security exception, protected CI, runsc, image, whole graph, secret rotation, release, full AT-SEC-05, and the platform C library reached through libc.
