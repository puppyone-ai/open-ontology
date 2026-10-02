# a080 handoff

Fixed local commit for independent review. No push, no pull request, no release.

Reviewed base: `efb61d62803073b3f98f5fe1885877824f504f80`. Workspace HEAD at the start of this task was `3c6977a70567ae3eeb300d0b4a202945f946355a`. Other writers have dirty files on `master`. This commit stages only the P5 spike, `docs/adr/ADR-P05-parser-prototype.md`, and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a080/SP01/P5-ipc-correction/`. Root `Cargo.lock`, other crates, CI, and older evidence are not in the commit. a073 still owns the main root.

## Hashes

Image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Compiler `rustc 1.94.0 (4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`, cargo `1.94.0 (85eff7c80 2026-01-15)`.

Baseline manifest, before any spike edit: `Cargo.toml` `528f979780ff278b39e29eec285a9db3257d9f31c7eda93080527ad670dd5695`, `Cargo.lock` `c46ce7f6a90e7f2983eaa4bb1471fb45e275e38a220716175801d56981ce53b7`. The lock is still that hash. The full pre-edit tree is `raw/baseline-src-before.sha256`. Post-correction file hashes are `raw/delivery-hashes.txt`, written after this handoff.

Protobuf excerpt now `142a052f7d22404fb513ad9f8d03f78f27e797352292c3a60c5c2babcef8285e` (was `e8185daf5bcd355692052fbdc990ea2d8acef9b4c3a41a32f189f272667e9d6b`). Bison excerpt unchanged `e8ecf341ce94fb7ad19de4a4985374eb821ff84fd4dff943b10a022ca93fc7a3`. Upstream header `3c06432ca3233a730e7b20afc1bc5750dd3f8c298db9c99b5df03dc6e1a4740c`. License matches and the libpg_query commit check are in `REPORT.md`. Full supply approval is not signed.

## Exits

Baseline: apt-get update 100; fetch 0; test default 0; test hooks 0; clippy default 0; clippy hooks 0.

Correction: ipc run 1 exit 101; ipc run 2 exit 0; default tests 0; hooks tests 0; clippy attempt 1 exit 101 for both feature sets; clippy attempt 2 exit 0 for both. Observation ipc run exit 0 (`raw/fix-ipc-observed.log`). Corpus goldens were not reblessed. Default corpus passed in 83.28 s with `containment=proven` on spawned cases.

## Cleanup

`docker rm -f oo-a080-msrv-f669cc94` exited 0. `/tmp/oo-a080-f669cc94` was removed. The nine deb blobs were deleted after `raw/debs-sha256.txt` was written. No prune. No volume or network had been created for this builder. Other containers were left in place. At cleanup, `oo-a072` and `oo-a079` were not in `docker ps -a`; this task did not remove them.

## Review gate

Stop here. The ADR remains Proposed. R4, R5, R9, native 17.7 versus server 17.10, Gateway authorization, network isolation, supervisor-crash execution, amd64, and gVisor stay open. Details and the raw proof lines are in `REPORT.md`.
