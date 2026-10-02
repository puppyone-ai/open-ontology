# a070 escape containment

Foundation correction for the xtask harness. a062 `request_changes` stands: an early parent exit plus `setsid` double-fork could leave live children while `required_satisfied` was sealed, and a held stdout inode could grow after return. This change closes that seal.

The delivering commit’s parent, at the time of the commit, is `3c6977a70567ae3eeb300d0b4a202945f946355a`. The commit and tree hashes are written in `COMMIT.txt` after `git rev-parse`.

## What changed

`execute_command` and `gates --execute-local` spawn each allowlisted command under a per-command supervisor. The supervisor is an external C program compiled at runtime with a root-owned `/usr/bin` or `/bin` compiler (`cc`, `gcc`, `clang`, `gcc-N`, `clang-N`, or a Debian `<triplet>-linux-*-gcc[-N]` / `clang` name). The runner process does not call `PR_SET_CHILD_SUBREAPER`.

On Linux the supervisor, in its own process, sets itself as the child subreaper, places the direct child in a new process group, captures stdout and stderr through pipes, and writes at most 16777216 bytes per stream. Cleanup is `waitpid(-1, WNOHANG) == ECHILD` for three consecutive checks. A cgroup v2 child cgroup is used only when the supervisor can create one. `/proc` listings, process groups, and start tokens are not the proof. If the supervisor cannot be compiled, cannot arm the subreaper, or cannot show a settled reap, the result is `containment=unavailable` or `unverified`, `gate.containment` is raised, and `required_satisfied` stays false.

A retained 16 MiB prefix is the capture bound. An extra byte sets `output_limited` and blocks the seal. Exact 16777216 bytes with no extra byte can still seal.

macOS has no unprivileged subreaper in this slice. The supervisor writes `containment=unavailable` and does not exec. Execute-local does not seal there. `run_bounded` falls back to the old owned-tree helper only when the supervisor did not spawn, so fixture receipt still runs on macOS. That fallback is not execute-local proof.

Owned paths:

- `crates/xtask/src/containment.rs` (new)
- `crates/xtask/src/lib.rs`
- `crates/xtask/src/owned_process.rs`
- `crates/xtask/src/runner_capture.rs`
- `crates/xtask/src/gates.rs`
- `crates/xtask/src/suites.rs`
- `crates/xtask/tests/cli.rs`
- `testing/harness/README.md`
- this evidence directory

No root manifest, lock, toolchain, CI, other crate, spec, or ledger edit. `#![forbid(unsafe_code)]` is unchanged. No new crate dependency.

## Source under test

Fixed snapshot, `git archive` of `304fcf9f116bbbabe0f91a2caca5df5949ef476c` (tree `723cd717083509697b221a773cc8f3c61a0261a4`, parent `fb2d1d19c21b510dfd6231c3b67d0efbf4285491`) plus the owned harness files. xtask did not change between that archive commit and `3c6977a70567ae3eeb300d0b4a202945f946355a`.

- snapshot tar sha256 `334d6b3f25b7d91b482135138c10f842dc789d6b6890013a77c2af2e4528f2fa`
- snapshot tar bytes `8109056`
- file inventory sha256 `391806b13735e436a950943aae09c30e34417defcc785c2d8facbfe8724488c3`
- image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `rustc 1.94.0 (4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`, `RUSTUP_TOOLCHAIN=1.94.0`

Pre-edit baseline, same image and posture, commit `efb61d62803073b3f98f5fe1885877824f504f80`, tree `da264380bef75579f2691504a653b1c49878978b`, run `0b36ef12-091e-4604-bfb4-f6ceb7ebff53`, tar sha256 `8710dff50264830f9d636dd0ba706efe12de5b0fbdd6d043b305fc12504d0318`. `fetch=0 test=101 clippy=0`. The 101 is the two pre-existing license tests.

## Checks

Host, final snapshot, `--locked --offline`, target dir outside the repo:

- `cargo test -p xtask --lib`: exit 101, 32 passed, 2 failed. Failures are `deny_toml::tests::reviewed_shape_rejects_unknown_keys_and_weakened_values` and `supply::tests::checked_in_deny_config_matches_the_reviewed_policy` (`licenses.allow is not the reviewed set`). Those failed on the pre-edit baseline. Assertions were not weakened. `ci/deny.toml` was not edited.
- `cargo test -p xtask --test cli`: exit 0, 16 passed. This host CLI run was on the snapshot before the Debian multiarch compiler-name fix. That fix does not change the macOS refusal path.
- `cargo clippy -p xtask --all-targets -- -D warnings`: exit 0 on the final snapshot.

Linux arm64, attempt 1, run `aeda77c1-bace-43a7-8eb1-9788aaf83910`, tar sha256 `d2c0d36220db448e62d96ae6bc3242813f271fe5fc8ed3d6c83b756b1cda4aa1`. `fetch=0 test=101 clippy=0 probe=2`, container exit 1. `/usr/bin/gcc` canonicalizes to `/usr/bin/aarch64-linux-gnu-gcc-14` (uid 0, mode 755). The name check rejected that basename, so the supervisor was never compiled and containment stayed `unavailable`. Preserved under `linux-arm64/`. The name check now accepts that Debian multiarch form. This attempt is not a kill failure and not a surviving `setsid` child.

Linux arm64, attempt 2, run `85b9cabe-c5f4-484c-aedd-ba016ebede1e`, same volumes as attempt 1, final tar hash above. Posture: `--pull never --platform linux/arm64 --init --user 0:0 --security-opt no-new-privileges --cap-drop ALL`, memory 8g, cpus 4, pids 4096, network bridge, tmpfs `/tmp:rw,nosuid,nodev,noexec,size=2g`, readonly `/src`, private cargo and target volumes, no socket, no host network, no host HOME. `fetch=0 test=101 clippy=0 probe=0`, container exit 0, OOMKilled false. Lib harness: 32 passed, 2 failed, same two license tests, 1.63s. Clippy exit 0.

Independent probe against `/target/oo-xtask-contain-c6dba532d425f131`, cgroup unarmed (`cgroup=0`):

- 15 immediate double-forks: parent exits before the grandchild pid is written. Each returned `containment=verified`, `spawned=1`, `cleanup_incomplete=0`, `note=ok`, and `sleep 121` count 0.
- 15 recorded double-forks: grandchild pid present in the pid file. Each pid was gone at return and still gone 300 ms later. Direct pid and grandchild pid differ.
- Held writer: side file `16777368` bytes at return and `16777368` after 300 ms. Captured stdout `16777216`. Writer gone.
- `dd` count 16: stdout `16777216`, `output_limited=0`, `exit_code=0`, `containment=verified`.
- One extra byte and `dd` count 32: stdout stayed `16777216`, `output_limited=1`, `exit_code=-1`, `containment=verified` (reap settled; the extra byte blocks the seal in the Rust gate).
- Foreign `sleep 180` was alive after the probe and was then killed by the probe. `leftover_sleeps=0`.

`cargo test -p xtask` stopped after the lib harness because those two license tests failed, so the CLI harness was a separate invocation. Run `9a65e30f-0b0b-4edd-a90e-ff2c5b71234a`, same snapshot and volumes, `--network none`, `--test cli`: exit 0, 16 passed, 16.78s, container exit 0. That includes the positive execute-local seal, imported-capture `gate.unverified`, failed child, deadline, setsid-with-wait, output limit, source drift, and self-test.

## Cleanup

After the logs were on disk, `docker rm` of the four `oo.owner=a070` containers and `docker volume rm` of the four task volumes exited 0. No `oo-a070` container or volume remained. No prune.

## Still open

Full SP00, the secret gate, protected CI, runsc, release, and amd64 are unsigned. cgroup v2 could not be created under `--cap-drop ALL`; the settled reap is the proof that ran. macOS execute-local stays fail-closed until a per-command ownership primitive exists there. The two license-policy test failures remain. `Cargo.lock` is dirty in the worktree from another session and was not staged. a063 remains the root-lock owner.
