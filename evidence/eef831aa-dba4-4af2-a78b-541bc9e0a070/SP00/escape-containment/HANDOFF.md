# Handoff

a070 escape-containment correction. One harness change. No push and no pull request.

## Identity

Parent at commit time: `3c6977a70567ae3eeb300d0b4a202945f946355a`.

The commit and tree are in `COMMIT.txt` in this directory, filled after `git rev-parse` of the owned-path commit. A follow-up commit may add only that file and the hash lines.

Archive the Linux run actually executed: commit `304fcf9f116bbbabe0f91a2caca5df5949ef476c`, tree `723cd717083509697b221a773cc8f3c61a0261a4`, parent `fb2d1d19c21b510dfd6231c3b67d0efbf4285491`, plus the owned harness overlay. Snapshot tar sha256 `334d6b3f25b7d91b482135138c10f842dc789d6b6890013a77c2af2e4528f2fa`, 8109056 bytes.

## Exits

| Run | fetch | test | clippy | probe / cli | container |
| --- | --- | --- | --- | --- | --- |
| Baseline `0b36ef12-091e-4604-bfb4-f6ceb7ebff53` | 0 | 101 | 0 |  | 1 |
| Linux attempt 1 `aeda77c1-bace-43a7-8eb1-9788aaf83910` | 0 | 101 | 0 | probe 2 | 1 |
| Linux attempt 2 `85b9cabe-c5f4-484c-aedd-ba016ebede1e` | 0 | 101 | 0 | probe 0 | 0 |
| Linux CLI `9a65e30f-0b0b-4edd-a90e-ff2c5b71234a` |  | cli 0 |  | 16 passed | 0 |
| Host lib, final snapshot |  | 101 |  | 32 passed, 2 license failures |  |
| Host clippy, final snapshot |  |  | 0 |  |  |
| Host CLI |  |  |  | 16 passed, exit 0 |  |

Test exit 101 on the package run is the pre-existing `supply.license` pair. Linux attempt 2’s container exited 0 because those were the only test failures and the probe passed. The CLI harness was run separately after cargo stopped at the lib failure.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Toolchain override `1.94.0`. CapDrop `["ALL"]`. Privileged false. Network bridge on the package runs, none on the CLI follow-up. Tmpfs `/tmp` is `rw,nosuid,nodev,noexec,size=2g`.

## Cleanup

`cleanup.txt` records `docker rm` and `docker volume rm` exit 0 for the four a070 containers and four a070 volumes. Logs stay in this directory. No prune.

## Do not treat as signed

Full SP00, secret gate, protected CI, runsc, release, amd64.

macOS still has no verified containment backend. Execute-local there must keep failing closed. cgroup v2 was not armed in the Linux proof; do not require it until the posture can create a child cgroup and the populated check is part of the same settle.

`Cargo.lock` was dirty from another session and was left untouched. a063 owns the root lock. Do not add a dependency or flip `unsafe_code` to land a supervisor. The C compiler path is the one that ran.
