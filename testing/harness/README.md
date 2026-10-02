# Harness

`xtask` is the engineering gate runner. It is not a product service, driver, or gateway. The production binary has no fault switch and no HTTP gate-disable path. Unknown `--disable-gates`, `--fault`, and `--enable-faults` exit 2.

```text
cargo xtask help
cargo xtask version
cargo xtask <command> --format json
```

Exit codes: 0 check succeeded, 1 failed, 2 usage, 3 pending, blocked, or not implemented. Exit 0 is not phase sign-off. JSON always reports `phase_signoff` `in_review` and `protected_github_policy` false.

## Commands

| Command | What it does |
| --- | --- |
| `contracts --check` | Compares checked-in schemas and runs the five maintained `oo-core` tests: `schema_contract`, `canonical_golden`, `fixtures_pg_mysql`, `frozen_corrections`, `boundary_corrections`. A zero-test cargo result fails. It does not write schemas. |
| `contracts --library-only` | Document compare only. Status `library_only`, exit 3, `full_gate` false. It does not satisfy a required gate. |
| `contracts --self-test` | Mutates an isolated copy and restores nothing in the shared tree. |
| `architecture --check` | Calls `oo-arch-check` on locked metadata. |
| `evidence --check --path FILE` | Validates evidence v1 and the artifact bytes. |
| `evidence --check --path FILE --adapter historical-draft` | Reads an a001–a022 draft. Exit 3. Does not rewrite it. |
| `gates --check-manifest` | Structure, monotonicity, workflow text. Jobs are not supplied. |
| `gates --check --jobs FILE --baseline-source bootstrap\|git-object\|git-show` | Structural check of an imported jobs file. `git-object` and `git-show` read `BASE:ci/baseline/gates-baseline.yaml` via git. A caller string is not evidence. An imported capture, including a recomputed coordinator binding, is `gate.unverified` and cannot be `required_satisfied`. Bootstrap still exits 3 on `gate.baseline_approval`. `OO_HARNESS_APPROVED_BASELINE_SHA256` is ignored and is not an appointment. |
| `gates --execute-local` | Spawns the nine allowlisted commands only through the prebuilt per-command supervisor at `testing/harness/containment-supervisor/bin/containment-supervisor`, with a deadline from 1 to 2700 seconds (`OO_XTASK_GATE_DEADLINE_SECS`). The test runner is not a subreaper and does not compile anything at runtime. On Linux the helper is the child subreaper for that command. It captures stdout and stderr through pipes and stops each stream at 16 MiB. The retained prefix is not a disk quota: an extra byte is `gate.output_limit`, and the capture file is not written past the limit. A pass requires `containment=verified` after a complete empty `/proc` children snapshot and `waitpid` `ECHILD`, twice, then the same pair again. Three `ECHILD` results alone are not that proof. One sample can still race a fork that has not been reparented, and this is not host isolation, a pid namespace, or runsc. `cgroup=0` is the normal result when the helper cannot create a cgroup, including under `--cap-drop ALL`, and it does not by itself fail a settled reap. An armed cgroup must report `populated 0`. The helper is executed only when its sha256 matches the embedded pin and `provenance/artifact.sha256`, and the file is a regular owner-executable that is not group or world writable. The bytes are copied to a private 0700 file and hashed again. A missing, mismatched, or untrusted helper is `unavailable` and cannot seal. A helper that does not finish cleanup is `unverified`. macOS has no unprivileged subreaper in this slice, so execute-local returns `note=platform` without executing the Linux binary. `runner_authority` is `local-engineering` only after a command actually spawned. An imported capture is not execution authority. A dirty tree is refused. Drift, a nonzero child, or a timeout fails the seal. Release and CI stay false. |
| `test --suite SUITE` | Implemented SP00 smokes, or exit 3 for a named unimplemented suite. |
| `test --suite integration-a --scale s --seed 42` | Spec 18.3 form. Reaches the suite and exits 3. `--database-fallback`, `--runtime`, `--fault-every-step`, and `--profile standard` do the same. Unknown switches and `--fault` exit 2. |
| `bench --suite public-e` | Exit 3 until benchmark E exists. |
| `datagen --check [--receipt PATH]` | `REVIEW_FROZEN` is not a receipt. A fixture receipt runs `OO_XTASK_CARGO` only when that executor, the manifest, and the dataset or output paths are outside the repo and hash-bound. `verify` needs `--dataset`. `generate` needs `--scale`, `--seed`, `--output`, and `--scope`. `export-sources` needs `--dataset` and `--output`. `list` needs `schema` or `fixtures`. Child nonzero or a deadline is exit 1. Child zero stays exit 3 and is not a W05 pass. |
| `infra --check [--receipt PATH --script PATH --run-id UUID]` | Hashes allowlisted scripts and does not execute them. A fixture receipt may run `bash SCRIPT --run-id UUID` only for an outside-repo hash-bound script. Docker stays off. |
| `docs --check` | Spec, plan, and governance checkers, plus harness whitespace. |
| `secrets --check` | Repository text scan. |
| `supply --check` | Legal deny config. Exit 3 until the tools run. |
| `supply --run-tools` | Installs pinned cargo-audit and cargo-deny into a temp cargo home. |
| `release --check [--digest HEX]` | Exit 3. Does not tag, promote, or deploy. |

## Suites that exist

`sp00-help-smoke` and `sp00-contract-fixture-smoke` are harness fixture suites. They do not stand in for integration A, security B, runtime C, release-state, operational, public E, ontology D, or performance F. Those return status `suite_not_implemented` and exit 3. The contract fixture smoke expects the library-only result (exit 3, one `contract.library_only` issue) and fails if schemas drift.

A fixture `datagen --check` spawns `OO_XTASK_CARGO run --locked --offline --manifest-path <outside-manifest> --bin oo-datagen -- <operation flags>`. The child runs outside the repo. A development receipt binds the a035 evidence digest, the datagen tree hash, and the coordinator binding. It does not spawn when that hash differs from the current tree or the ledger still shows the datagen writer running. `REVIEW_FROZEN` is not that receipt. W05 stays false.

A fixture `infra --check` runs `bash <outside-script> --run-id <uuid>` in an owned temp stage. Child nonzero is exit 1. Docker stays off. A development receipt binds the a038 inventory and does not start Docker. Full W05 integration is a later task. An appointed CODEOWNERS receipt does not authorize either local synthetic branch.

`secrets --check` walks hidden, ignored, untracked, and `target` paths. It skips only the `.git` directory. Symlinks, unreadable files, and files over 1 MiB fail closed and are not treated as clean. Exact sha256 allowlist entries stay `secrets.classified_pending`. Diagnostics do not echo the matched text.

`supply --check` parses `ci/deny.toml` and walks every registry package in each shipping lock, `Cargo.lock` and `bench/datagen/Cargo.lock`, including wasm crates, against the crates.io index cache. Age floor is 604800 seconds. A missing lock, a young package, a yanked release, or a checksum miss fails the check. When those are clean, the command exits 3 until `supply --run-tools` runs cargo-audit and cargo-deny on each shipping lock. `full_at_sec_05` stays false: JavaScript, native, and image OS scans are not integrated. Wasm is age-checked and is not a supported matrix target.

While other writers are active, run this crate with its own target directory:

```text
CARGO_TARGET_DIR=/tmp/oo-sp00-w05-target cargo test --locked -p xtask
CARGO_TARGET_DIR=/tmp/oo-sp00-w05-target cargo test --locked -p oo-arch-check
```

Do not format or test the whole workspace from this crate while core sources are still moving.
