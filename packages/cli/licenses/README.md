# Pinned upstream license supplements

These are original upstream texts missing from the published crate archives,
not canonical SPDX replacements or legal clearance. `manifest.json` records
immutable source URLs, SHA256 hashes and exact registry VCS identities. The
offline collector checks them and fails closed on changed or missing files.
For alternative license expressions, all upstream alternatives are retained.

Verified in `evidence/m-s6e-20261105/`:

| Crate version | Upstream release tag | License files |
| --- | --- | --- |
| delegate 0.13.5 | kobzol/rust-delegate `v0.13.5` | LICENSE-APACHE, LICENSE-MIT |
| flatbuffers 25.12.19 | google/flatbuffers `v25.12.19` | LICENSE |
| objc2-core-foundation 0.3.2 | madsmtm/objc2 `frameworks-0.3.2` | LICENSE.md |
| objc2-system-configuration 0.3.2 | madsmtm/objc2 `frameworks-0.3.2` | LICENSE.md |
| russh 0.65.0 | warp-tech/russh `v0.65.0` | LICENSE.txt |
| russh-cryptovec 0.62.0 | warp-tech/russh `v0.62.0` | LICENSE.txt |
| russh-util 0.52.0 | warp-tech/russh `v0.52.0` | LICENSE-2.0.txt |
| tiberius-macros 0.1.0 | tiberius-rs/tiberius `v0.13.0` (workspace release) | LICENSE-APACHE.txt, LICENSE-MIT.txt |

Except for russh-util, each tag resolves to the published crate's VCS commit;
the crate manifest or inherited workspace manifest confirms its version.
Tiberius has no separate macros-0.1.0 tag; its workspace release contains that
exact crate version.

The russh-util archive declares a dirty VCS commit
`3d09c20c52a542ffa327723d3a44cb1edd9b0ca4`, whose manifest still says 0.51.0.
Its `v0.52.0` tag resolves to `a404ae31ccb192ad4ef65ac8a65ad96bac2f77f6`,
whose russh-util manifest says 0.52.0. LICENSE-2.0.txt is byte-identical at both
commits. The supplement pins the published VCS record and packaged
Cargo.toml.orig hashes, explicitly including `vcs_dirty: true`; dirty records
remain rejected for every other entry. This is license-text provenance,
not a claim that the published source tree exactly matches a clean Git tag.
