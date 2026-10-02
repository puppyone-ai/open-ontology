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

## Reviewed baseline anchor

Session `eef831aa-dba4-4af2-a78b-541bc9e0a113` binds helper-lock adoption in `testing/harness/helper-evidence-check/src/trust.rs`. The checker does not read that instant from the inventory, the records, or a live git command.

The anchor commit is `f1a6005971e3dc3075094c87ed670fc9d25e9ae0`. Its author time and committer time are both `2026-10-02T19:10:37+08:00`, which is `2026-10-02T11:10:37Z`. The helper lock sha256 is `14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1`. Those three facts were read from git before the constants were kept. A later baseline needs a reviewed code and evidence change. Paired claim files cannot move it.

Completed age is recomputed from each crate's `version.created_at` to that adoption instant. The stored `age_days_at_adoption` has to match.

The reviewed choice on the three `MIT OR Apache-2.0` crates is MIT. nix and cfg_aliases keep a null choice. Both upstream bodies and both obligation rows stay required. An Apache-2.0 choice paired across the inventory, the record, and the nix native entry is rejected for this baseline even when the license files match. This is the choice recorded for these crates. It is not a judgment that Apache-2.0 is forbidden elsewhere.

The retained a101 `scan-audit.out` has no `status` member. The checker rejects a present status, including `unverified`, any other non-blank string, and a malformed value. It does not write a status into that file, and it does not treat the file as a fresh scan. A matching inventory digest does not bypass the adoption, choice, or audit anchors. `audited`, `admission`, `fresh_current_scan`, and `release_signoff` stay false.

JSON nesting is limited to 64 containers. A scalar root is depth 0. An object or array root is depth 1. The checker stops before recursing into a 65th container and returns a bound error. A 20000-deep value is that same bound, not a process abort.

Unknown record keys, hard links, manifest digest binding, pre-stat time-of-check/time-of-use, hash-subprocess timeout, and the hash child's environment are not part of this fix. This directory still does not close AT-SEC-05, the whole graph, image, owners, platform libc, runsc, CI, secrets, customer, or release. The checker is not a scanner, a legal owner, or a corporate approval.
