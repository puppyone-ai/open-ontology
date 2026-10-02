# a082 handoff

Stop here for an independent fixed-SHA review. No push, no pull request, no release, no new worktree.

## Commit scope

Parent at authoring time: `7143b2d9a7888d242983e5b9c10ef597246d5848`. The fixed commit is the one that adds this file. Owned paths only:

- `crates/xtask/src/containment.rs`
- `crates/xtask/src/deny_toml.rs`
- `crates/xtask/src/runner_capture.rs`
- `crates/xtask/src/suites.rs`
- `testing/harness/README.md`
- `testing/harness/containment-supervisor/` (manifest, lock, source, tests, provenance, licenses, pinned binary)
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a082/`

Root `Cargo.toml`, root `Cargo.lock`, `ci/deny.toml`, connector sources, pipeline sources, and other sessions' evidence are outside this commit. a073 remains the sole mainroot lock writer. a070's handoff still says a063 owns the root lock; that file was not edited.

## What the reviewer should check

Pinned artifact sha256 `3569f4e45f88cdedef55c6eb5b23e2c4eb9e9dc1c14690085ddf72681a95628e`, 716792 bytes, ELF, mode 0755. The same digest is `SUPERVISOR_SHA256` in `crates/xtask/src/containment.rs` and the sole line of `testing/harness/containment-supervisor/provenance/artifact.sha256`. Helper lock sha256 `14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1`. Remaining source hashes are in `REPORT.md`.

Attempt-3 phase files are all 0 except the four advisory curls that exited 22. Functional results: helper 3 unit + 8 branch, xtask lib 39, xtask CLI 16, both Clippy runs finished, probe `probe_result=pass` with foreign starttime 68069 unchanged and the 16MiB / plus-one / overshoot sizes in the report. Trust-boundary tests in that lib run include missing, tampered, and untrusted helper refusal.

Run 1 `launch.exit` 1 and `helper-clippy.exit` 101, and attempt 2 `launch.exit` 1, `helper-clippy.exit` 101, and `xtask-lib.exit` 101, stay on disk.

## Bounds the reviewer should keep

Per-command child subreaper inside the helper. xtask is not a process-global subreaper. Settle is two empty-children plus `ECHILD` observations, then a confirm. A fork in flight after that confirm, another pid namespace, and `SIGKILL` of the supervisor before reap can still leak. Same-uid TOCTOU remains between the second hash and exec of the `0700` copy. `cgroup=0` under `--cap-drop ALL` is expected and is not host isolation or runsc. macOS returns note `platform` and does not exec this Linux binary. The age field stored by `fetch.sh` is the first `created_at` in the crates.io body; version publish times were read separately and are listed in `REPORT.md`. Advisory evidence is one nix directory listing (`RUSTSEC-2021-0119.md`) and four HTTP 404s.

No SP00, secrets, protected CI, runsc, or release claim. Full matrix remains at the integration node. Own containers and volumes named for a082 were absent at report time. Foreign resources were not removed.
