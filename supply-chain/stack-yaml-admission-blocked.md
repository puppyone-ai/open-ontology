# Stack YAML admission: blocked (S0-M-04 / E015)

Assessment time: 2026-11-03T20:28:03Z. This is not an admission record.
No YAML dependency is added by this card. `oo-stack` ships JSON only.

Candidate: crates.io `serde-saphyr` 1.3.0, already used by oo-core. Existing
`supply-chain/core-dependency-delta.json` records publication
`2026-09-16T16:41:36Z`, MSRV 1.89, features `serialize,deserialize`, defaults
disabled, `MIT OR Apache-2.0` (Apache-2.0 relied upon), and crate checksum
`b8050abb251097357e24aff63ba2c52a6309ecb7d23a5474023df960a02694d8`.
Those are historical records, not a fresh verification by this executor.
The candidate is older than seven days; age is not the blocker.

Neither `cargo-audit` nor `cargo-deny` is installed. Availability evidence:
`evidence/a-s0s1-20261104/yaml-scanners.txt`. No current advisory scan was run;
known-exploitable-high-severity status is **unknown**, not false. A full current
license/transitive-license audit and advisory disposition are therefore not
claimed. Policy section 4.1 does not allow marking the candidate audited.
No exception or invented reviewer approval is used.

Follow-up: the supply-chain owner must collect official publication metadata,
read and hash LICENSE/NOTICE for the chosen exact version and compiled
transitives, record SPDX/obligations, run and retain advisory scans, and fill
all policy section 1 fields before enabling YAML for this crate. Reusing the
core's historical record alone is not the new complete audit requested here.

The new crate uses only versions already in Cargo.lock: serde 1.0.229
(derive), serde_json 1.0.151 (arbitrary_precision), libc 0.2.189. Defaults
remain enabled as in their existing CLI selection. The lock delta adds only
the local oo-stack package. Model egress uses the Python standard library;
no pip/npm/runtime package is added. This statement does not renew the
workspace's existing supply-chain approvals.
