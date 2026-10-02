# HANDOFF — a113 helper record trust

Review the local commit whose parent is `d67795e397657087495a32317938ecdd7a6cb028` (tree `36225d138fc3f3bb1cbaea34d80b68eee7101034`). The fixed SHA and tree are that commit. This handoff is inside it, so it cannot name its own SHA. No push, pull request, release, or customer handoff was made. Checklists stay with the coordinator.

## What changed

Allowed paths only:

- `testing/harness/helper-evidence-check/src/check.rs`
- `testing/harness/helper-evidence-check/src/json.rs`
- `testing/harness/helper-evidence-check/src/lib.rs`
- `testing/harness/helper-evidence-check/src/trust.rs` (new)
- `testing/harness/helper-evidence-check/tests/trust.rs` (new)
- `testing/harness/helper-evidence-check/README.md` (new)
- `testing/harness/containment-supervisor/provenance/README.md` (anchor section appended)
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a113/SP00/helper-record-trust/` (this evidence)

`Cargo.toml` blob `4abcbae92e54a958c36ecef8ffcf370d464a305b` and `Cargo.lock` blob `9497313a6057015f461ae74a7c8bd5f48a8bcceb` are unchanged. `tests/closure.rs` blob `b23f1a07e4be84583258cbd65cde9b1dea8f90a0` is unchanged. No root manifest, deny policy, helper runtime, audit-record body, license bytes, inventory, historical scan, ledger, spec, or plan edit.

## What the green run showed

Linux arm64, Rust 1.94.0 (`4a4ef493e`), image digest `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, container `7e674b394cfa`, `2026-10-02T20:48:46Z`. Smoke exit 0. Unit 7, closure 25, trust 11. Clippy lib+bin exit 0 and `cargo clippy --test trust --test closure -D warnings` exit 0. Case log `linux/probe/trust-cases.tsv`.

Checker-spawn rows to read first:

- aligned adoption+age forge: exit 8 `invalid-time`
- aligned age with the real adoption instant: exit 9 `age-floor`
- aligned Apache choice, including nix native entries: exit 4 `wrong-dual-choice`
- `status=unverified` with a rewritten inventory digest: exit 19 `fake-claim`
- depth 64 object/array/mixed (63 containers inserted under the record root): exit 0, OK line, `audited=false`
- depth 65 and 20000 for the same three shapes: exit 22 `bound`, signal none
- canonical bundle: exit 0, `records=5 audited=false`

## Retain as historical RED

a109 cases, unfixed source blobs equal to `abd074af`, binary sha256 `c2a7b9bae81198010c0cfa6b682a71087a2df3541c14c16fd7df7f17d4ca8694`:

- `paired-adopt-not-lock-commit` miss, exit 0
- `paired-choice-apache` miss, exit 0
- `depth-64` observe, exit 0 (old boundary; 64 inserted containers is depth 65 now)
- `depth-20000` observe, exit 134, stack overflow abort on the unfixed parser
- `audit-status-unverified-aligned` miss, exit 0

Author attempts 1–3 stay red. See `linux/attempt-*/README.txt` and their `phases/`. Attempt 2's `--all-targets` failure is `timeutil.rs:112` `clippy::assertions_on_constants`. That file was not part of this slice.

## Flags and unsigned limits

`audited=false`, `admission=false`, `fresh_current_scan=false`, `release_signoff=false`. No fresh scan was claimed. Positive stdout stays the complete-for-review line.

Unsigned for this slice: unknown keys, hard links, manifest binding, pre-stat TOCTOU, hash-subprocess timeout, hash-child environment. Proved depth behavior is the 64/65/20000 object, array, and mixed spawns plus the malformed escape, UTF-8, and duplicate-key spawns. Do not read that as full hostile-input robustness, scanner output, legal review, or release signoff.

## Cleanup proof

`linux/docker-rm.txt`, `network-rm.txt`, `volume-rm.txt`, `tmp-removed.txt`, `docker-ps-owned-after.txt`, `hostconfig.txt`, `builder-count-before.txt`. Only the a113 container, network, two volumes, and `/tmp/oo-a113-9a2b3688-d861-4e56-8481-c08d2c3508d0` were removed. a112 and the exited a064 containers were still present afterward.
