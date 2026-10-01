# CI gates

`cargo xtask` is `cargo run --locked -p xtask --`. Exit 0 means the check succeeded. `phase_signoff` stays `in_review`. `protected_github_policy` stays false. This repository has no branch protection, no CODEOWNERS activation, and no appointed security owner. A 7-day security exception is invalid.

## Files

| Path | Role |
| --- | --- |
| `ci/gates.yaml` | PR manifest. `document: manifest`. |
| `ci/baseline/gates-baseline.yaml` | Bootstrap baseline. A different path from the manifest. |
| `ci/deny.toml` | cargo-deny 0.20 config. No clarify block. |
| `.github/workflows/pr.yml` | Active SP00 jobs, then the Rust aggregate. |
| `.github/workflows/nightly.yml` | Same active jobs. Cron `17 7 * * *`. Cancel is off. |
| `.github/workflows/release.yml` | `cargo xtask release --check`. Expected exit 3. No tag and no deploy. |

The manifest is never passed as its own `--baseline`. Aggregate resolves `BASE` inside the workflow from the event: `pull_request` uses `github.event.pull_request.base.sha`; `push` uses that same base when present; `schedule` and `workflow_dispatch` use `github.sha` and require it to equal `HEAD`. An empty or all-zero `BASE` exits 1. `git fetch` of that object is fatal. There is no `|| true` and no checkout copy of `ci/baseline/gates-baseline.yaml`.

If `BASE` has no `ci/baseline/gates-baseline.yaml`, aggregate exits 1. If that file exists and `BASE` has no `crates/xtask/src/gates.rs`, aggregate exits 3 and prints `trusted_base_checker=pending_external`. The candidate tree is not the authority. When both objects exist, aggregate extracts `git archive` of `BASE` and runs that commit's `xtask` with `--baseline-source git-object`. A same-PR edit of `gates.rs` cannot approve itself: the checker compares the working-tree file with the blob in `BASE` and returns `gate.checker_untrusted` when they differ.

Each foundation job writes `exit_code` and `log_sha256` to the step output before it exits. Aggregate binds those values to the job row. A success result with a nonzero exit or a bad log hash is rewritten to failure. Shell jobs cannot mark a gate successful.

Bootstrap `--check` without `OO_HARNESS_APPROVED_BASELINE_SHA256` matching the baseline bytes exits 3 (`gate.baseline_approval`). That variable is not a production fallback and is not set by these workflows. This repository's current docs `HEAD` does not contain the baseline file or `xtask`, and GitHub branch protection is not configured. Aggregate on that commit exits 1. `protected_github_policy` stays false. No owner login is invented.

## Active jobs

These are `required` from the first code change. `closes_acceptance` is false on every gate, so a green job does not close an acceptance test.

| Job | Command | Timeout |
| --- | --- | --- |
| `gate-docs` | `cargo xtask docs --check` | 30 |
| `gate-fmt` | `cargo fmt --all -- --check` | 30 |
| `gate-clippy` | `cargo clippy --workspace --locked --all-targets -- -D warnings` | 45 |
| `gate-unit` | `cargo test --workspace --locked` | 45 |
| `gate-contracts` | `cargo xtask contracts --check` | 45 |
| `gate-architecture` | `cargo xtask architecture --check` | 30 |
| `secret-scan` | `cargo xtask secrets --check` | 30 |
| `gate-dependencies` | `cargo xtask supply --run-tools` | 45 |
| `gate-evidence` | `cargo xtask evidence --self-test` | 30 |
| `aggregate` | `cargo xtask gates --check`, then pending if that exit is 0 | 30 |

The aggregate job writes a jobs file from `needs.*.result` with a shell script. Allowed result tokens are `success`, `failure`, `cancelled`, and `skipped`. The commit must be 40 hex characters. There is no success row for `gate-aggregate`. The checker evaluates that gate.

Those rows are an audit of the job outputs. They are not execution authority. `gates --check` does not return `required_satisfied` for an imported capture, including one whose producer is `coordinator` or `github-actions` and whose binding is a public hash. If the base checker still exits 0, this workflow exits 3 and records `authenticated_provenance=pending`. A nonzero checker exit is kept. No GitHub signing key is stored here. Local engineering success is `gates --execute-local`, which spawns the allowlist in that process. `release_authority` and `ci_passed` stay false.

`cargo xtask gates --check-manifest` validates structure and workflow text. Jobs are `not_supplied`. Exit 0 on that command is not a CI pass.

## Dormant gates

`pending` gates reject a success result: `gate-datagen-sp00`, `gate-customer`, `gate-http-sdk`.

`not_applicable_until_phase` gates have an effective phase, a latest phase, and a reason. They are integration A (SP04), security B (SP06), runtime C (SP10, due SP11), ontology D (SP07, due SP12), public E (SP12), performance F (SP14), operational (SP11, due SP14), release-state (SP10, due SP14), SBOM/sign (SP11), and the nightly slices for pipeline (SP04), sandbox (SP11), and benchmark (SP12). The nightly workflow does not run an echo command for them.

`effective_phase` is enforced when `current_phase` reaches it, even if `latest_phase` is later. Operational and release-state therefore fail `gate.effective_reached` at SP11. SBOM stays due at SP11 (`gate.expired`). Moving `latest_phase` or `effective_phase` later, or deleting either, is `gate.deadline_extended`. Deleting a baseline gate is `gate.removed`.

A changed path that equals a gate `path_prefix`, or sits under it, activates that gate in the same change. The file name `CAPABILITY_INTRODUCED` does not. A missing path is `gate.changed_missing`. An activated gate that is not `required`, or that has no allowlisted exit-0 job, is `gate.capability_inactive` / `gate.capability_unproven`. Untouched prefixes stay inactive.

## Runners and actions

Permissions are `contents: read`. Runners are `ubuntu-24.04`. Timeouts are on every job. Failure logs upload with retention 14 days and `if-no-files-found: ignore`. The cargo registry is not uploaded. Evidence files are not deleted.

Pinned actions:

- `actions/checkout` `3d3c42e5aac5ba805825da76410c181273ba90b1` (v7.0.1, 2026-07-17)
- `actions/upload-artifact` `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` (v7.0.1, 2026-04-10), `if: failure()` only
- `dtolnay/rust-toolchain` `02cb101ec7c40f2c49e1d9714d64511d8e1b74de` (2026-09-12), toolchain `1.98.1`, rustfmt, clippy

The cache action is unused. The key that is not applied is `cargo-lock-1.98.1-${{ hashFiles('Cargo.lock') }}`.

Workflow text rejects `pull_request_target`, `self-hosted`, `id-token:`, `contents: write`, `secrets.`, and `|| true`. The GitHub repository URL, real CODEOWNERS logins, and branch protection are still external prerequisites. These workflows have not been executed on GitHub from this author run. A local temp git repository, with the baseline committed before the candidate edit, is the positive `git-object` proof. It does not set `protected_github_policy`.
