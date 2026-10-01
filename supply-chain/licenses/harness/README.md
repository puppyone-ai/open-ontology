# Harness licenses

`xtask` is Apache-2.0, the workspace license. It links two path crates, `oo-core` and `oo-arch-check`, which are Apache-2.0.

Direct registry dependencies are existing pins, not new crates:

| Crate | Version | Features | License on the pin |
| --- | --- | --- | --- |
| jsonschema | 0.57.0 | default features off | MIT |
| serde | 1.0.229 | derive | MIT OR Apache-2.0 |
| serde_json | 1.0.151 | arbitrary_precision | MIT OR Apache-2.0 |
| sha2 | 0.10.9 | default features off, std | MIT OR Apache-2.0 |
| thiserror | 2.0.21 | default | MIT OR Apache-2.0 |

`ci/deny.toml` allows Apache-2.0, MIT, BSD-3-Clause, Unicode-3.0, Zlib, Unlicense, and MIT-0. MIT-0 is the specific SPDX id of existing transitive `borrow-or-share` 0.2.4. The config has no clarify block. The security owner is not appointed, so a 7-day exception is invalid.

cargo-audit 0.22.2 and cargo-deny 0.20.2 are Apache-2.0 OR MIT. `cargo xtask supply --run-tools` installs them into a temporary `CARGO_HOME`. The texts below were copied from those pinned crate sources. `license_recorded` is true in `supply-chain/harness-dependency-delta.json`.

| File | sha256 |
| --- | --- |
| `cargo-audit-0.22.2-LICENSE-APACHE` | `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2` |
| `cargo-audit-0.22.2-LICENSE-MIT` | `29662666b44dff84977b46e05642cdef910bc3a93a17b5fd86e632bafa59cf21` |
| `cargo-deny-0.20.2-LICENSE-APACHE` | `8173d5c29b4f956d532781d2b86e4e30f83e6b7878dce18c919451d6ba707c90` |
| `cargo-deny-0.20.2-LICENSE-MIT` | `090a294a492ab2f41388252312a65cf2f0e423330b721a68c6665ac64766753b` |

The invocations that ran are `cargo-audit audit --json` and `cargo-deny --format json --config ci/deny.toml --locked check`.

Cargo license output does not close AT-SEC-05. `full_at_sec_05` stays false. JavaScript trees `console/` and `sdk-ts/` are absent. `xtask` has no `build.rs`. Image pins under `testing/infra/` are not an OS image scan.

chrono-tz 0.10.4 is an existing core pin. cargo-deny 0.20.2 did not reject it. No extra waiver was added for that package.
