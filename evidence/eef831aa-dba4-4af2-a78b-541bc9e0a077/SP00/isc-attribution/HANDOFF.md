# Handoff: a077 r-efi attribution correction

Parent of this commit: `raw/parent.txt`, from `git rev-parse HEAD` immediately before `git commit`. This file does not contain its own commit SHA.

## Owned paths

- `contracts/supply-chain/isc/NOTICE.md`, `policy.md`, `provenance.md`, and `ERRATUM.md`
- `contracts/supply-chain/isc/bodies/` — exact `r-efi` 6.0.0 `AUTHORS`, `r-efi` 5.3.0 `AUTHORS`, and rustls 0.23.45 `LICENSE-ISC`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/SP00/isc-attribution/`

`ci/deny.toml` still has the single `"ISC"` allow entry. `CDLA-Permissive-2.0` is not added. Root manifests and locks are untouched. `supply-chain/policy.md` is untouched. The a067 report, handoff, and raw exits are untouched.

## What a reviewer can check

`ERRATUM.md` quotes the false `b14c782` notice sentence and supersedes it. `AUTHORS` sha256 `d027e91dbc9cdbb2f1190068e498bd6b61cff022b6a032b191021ba658d96111` contains the MIT grant and, below it, the Red Hat, Microsoft, and David Rheinsberg copyright lines, then the author list. There is still no file named `LICENSE`, `COPYING`, or `NOTICE`. The README SPDX line is not the grant.

`fixtures/mit-or-gpl-missing-mit-text` is unchanged and is an expression-only deny probe. It is not the `r-efi` crate.

rustls `LICENSE-ISC` sha256 `7cfafc877eccc46c0e346ccbaa5c51bb6b894d2b818e617d970211e232785ad4`. Second line: `Copyright (c) 2016, Joseph Birr-Pixton <jpixton@gmail.com>`.

The OR-token mismatch between `supply-chain/policy.md` and `ci/deny.toml` remains separate pending work. Ring remains `Apache-2.0 AND ISC`.

Pre-edit check, archive `6fa6481267e5a4efac20831fa0b407a8cf028905`, Linux arm64, Rust 1.94.0: `cargo fetch --manifest-path Cargo.toml --locked` exit 0, `cargo check --manifest-path Cargo.toml --locked --offline -p oo-core` exit 0. Log: `pre-smoke/console.log`. The attribution checker exit is `raw/check-attribution.exit`.

cargo-deny was not reinstalled and the b2 graph was not re-run. a069 already recorded that ISC behavior. This commit does not approve SP00-W06, a full supply audit, release, or customer delivery.

The task container, volumes, network, and `/tmp/oo-a077-e844e982-ba5c-462c-b01a-27d3c63bf811` were removed. No push and no pull request.
