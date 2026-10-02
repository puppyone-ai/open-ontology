# a096 — fixed Rust containment supervisor

Session `eef831aa-dba4-4af2-a78b-541bc9e0a096`. Reviewer grok-4.7, independent, no subagents. The moving worktree was not the review source. No source, manifest, lock, or foreign evidence file was edited. Nothing was staged or committed.

## Decision

Approve the Rust containment slice and the single ISC allow shape on commit `f1a6005971e3dc3075094c87ed670fc9d25e9ae0`.

This approval is scoped to those two objects. It leaves the advisory-tool gate pending. It does not approve full SP00, secret rotation, protected CI, runsc, a full supply-chain pass, or release.

No experiment in this review produced `required_satisfied` or `containment=verified` with a live descendant, a vacuous positive Linux command, fail-open tamper handling, output growth after return, or the death of the foreign sleep. The open items below are bounds, not a request for changes on this slice.

## Identity

| Field | Value |
| --- | --- |
| Commit | `f1a6005971e3dc3075094c87ed670fc9d25e9ae0` |
| Tree | `86c64f512d67da47ae379a53e34342de8e317f25` |
| Parent | `7143b2d9a7888d242983e5b9c10ef597246d5848` |
| Subject | `xtask: pin the Rust containment supervisor` |
| Ancestor of HEAD at this check | yes, HEAD `8c73abb719012a204ab2bf2a8c633ee794f1b45a` |
| Archive | `/tmp/oo-a096-eef831aa-dba4-4af2-a78b-541bc9e0a096/archive` |
| Archive check | 3718 blobs, missing 0, mismatch 0, extra 0 |

The commit changes `crates/xtask/src/{containment,deny_toml,runner_capture,suites}.rs`, `testing/harness/README.md`, the new `testing/harness/containment-supervisor` crate, and the a082 evidence tree. It adds no `.c`, `.h`, or `.cc` file. `ci/deny.toml` blob `367598cb3768b8c8af698041cd0f84ce6f8fb7be`, root `Cargo.toml` blob `c78ee5d0575a11cd6078cf0055cdd2c673edf583`, and root `Cargo.lock` blob `c8715026efdfd09a36b7a5e67fa7be6757f26dbb` are unchanged from the parent.

## What ran

One linux/arm64 Rust builder. At launch, one foreign rust-builder was already running (`oo-a091-msrv-051d4062`) and the host `cargo`/`rustc` count was 0, so this task took the second slot. The builder was `oo-a096-rust194-075afd80-ece4-4e25-bb21-21cdf591841f`, container `0c26259d5ee5348e2bc6a5c69b606acb33568fd3912000004d2c890f03f0461d`.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, platform `linux/arm64`. Inside: Debian 13 (trixie), `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `cargo 1.94.0 (85eff7c80 2026-01-15)`, host triple `aarch64-unknown-linux-gnu`. `RUSTUP_TOOLCHAIN=1.94.0` overrode the snapshot `rust-toolchain.toml` channel `1.98.1`. Identity log time `2026-10-02T11:19:51Z`.

Flags: `--pull never`, `--user 0:0`, `--cap-drop ALL`, `no-new-privileges`, memory 8g, memory-swap 8g, cpus 4, pids-limit 4096, `--init`, read-only `/src`, private volumes `oo-a096-cargo-075afd80-ece4-4e25-bb21-21cdf591841f` and `oo-a096-target-075afd80-ece4-4e25-bb21-21cdf591841f`, tmpfs `/tmp`. No docker socket, host network, host HOME, shared cache, or prune.

Fetch, then the bridge was disconnected. `offline-network.txt` records `network_connect=closed` and `gaierror` name resolution failure. Offline tests and Clippy ran after that.

Integer phase exits, all 0:

| Phase | Exit |
| --- | --- |
| identity | 0 |
| clippy-component (`rustup component add clippy --toolchain 1.94.0`) | 0 |
| workspace-fetch (`cargo fetch --manifest-path /src/Cargo.toml --locked`, no `-p`) | 0 |
| helper-fetch (`cargo fetch` of the helper manifest `--locked`, no `-p`) | 0 |
| xtask-test | 0 |
| xtask-clippy | 0 |
| helper-test | 0 |
| helper-clippy | 0 |
| helper-rebuild | 0 |

`xtask` lib 39 passed, 0 failed, 3.61s. `xtask` cli 16 passed, 0 failed, 17.41s, including `execute_local_shim_is_required_satisfied_and_replay_is_not` and `output_limit_is_exact_and_stops_storage`. `xtask` clippy `-D warnings` finished in 8.13s. Helper lib 3 passed. Helper integration 8 passed, 0.78s. Helper clippy finished in 0.71s. Helper release rebuild finished in 1.11s. Smoke script wrote `SMOKE_SCRIPT_DONE 2026-10-02T11:25:15Z`.

The helper integration tests execute `CARGO_BIN_EXE_containment-supervisor` (the debug test binary). The xtask suite and the independent probe execute the pinned release ELF. Those are different files. The probe is the check of the committed bytes.

The image contains `/usr/bin/gcc` and `/usr/bin/cc`. The pinned ELF's dynamic `NEEDED` entries are `libgcc_s.so.1` and `libc.so.6`. During a live `/bin/sleep 2`, the helper's child comm was `sleep` and the helper comm was the truncated `containment-sup`. Helper source has one `Command::new` of its own executable for `--launch`, then `execvp` of the payload. It does not spawn `cargo`, `rustc`, or `cc`.

`#![forbid(unsafe_code)]` is present on the workspace `Cargo.toml`, `crates/xtask/src/lib.rs`, `crates/xtask/src/main.rs`, the helper `Cargo.toml` lint, `src/lib.rs`, and `src/main.rs`. The helper calls `set_child_subreaper`, `set_pdeathsig`, `execvp`, `waitpid`, `kill`, `killpg`, and `sigprocmask` through the safe `nix` 0.31.3 wrappers. xtask does not call `PR_SET_CHILD_SUBREAPER`. The pre-existing `testing/spikes/p05-pg17-parser` still has its own `prctl` call. That file is outside this commit. This review does not approve it.

Full matrix stays at the integration node. This arm64 run is not that matrix.

## Pinned artifact and rebuild

Host archive and in-container identity agree:

- Path `testing/harness/containment-supervisor/bin/containment-supervisor`
- sha256 `3569f4e45f88cdedef55c6eb5b23e2c4eb9e9dc1c14690085ddf72681a95628e`
- 716792 bytes, mode `0755`, not a symlink
- ELF64, `e_machine` 183 (`EM_AARCH64`), `EI_CLASS` 2, interpreter `/lib/ld-linux-aarch64.so.1`
- BuildID `8be0bf71de654f275b00cc1ae99198623bc62022`
- Helper `Cargo.lock` sha256 `14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1`, 1217 bytes

On this Darwin arm64 host, executing that ELF returned exit 126, `exec format error`. That is a platform limit. It is not a Linux cleanup result and it is not a positive containment seal. No linux/amd64 execution was run.

Release rebuild on the same container toolchain, locked and offline:

- rebuild sha256 `de3a29ff655ca86a1a295632f5fec0512a0afc76b311456958006b60b5590899`
- 716792 bytes, `same_bytes false`
- 31 sections on each side
- `.text` 337640 bytes both, 930 bytes differ
- `.rodata` pinned 32904, rebuild 32888, 24857 bytes differ
- `.data.rel.ro` 12304 both, 445 differ
- `.note.gnu.build-id` 36 both, 20 differ
- `.eh_frame` 49808 both, 1018 differ
- `.got` 1344 both, 1 differ

The unique `.rodata` registry strings are the same `nix` 0.31.3 source paths with different cargo homes: pinned `/cargo-home/registry/...`, rebuild `/cargo/registry/...` (`CARGO_HOME=/cargo` in this container). The pin still matches the committed file. This is a source-to-artifact relation for Rust 1.94.0 on this image. Generic reproducibility is outside the result.

`assess_file` rejects a missing path, a symlink, a non-file, mode bits `022`, a file that is not owner-executable, a uid that is neither 0 nor the current euid, a symlink parent, a world-writable parent without the sticky bit, and a digest mismatch. `blocks_pass` treats `helper-missing`, `helper-mismatch`, `helper-untrusted`, `launch-failed`, and `protocol-malformed` as blocking. The xtask lib suite that contains those checks passed. Imported captures stay integrity records. `gates.rs` states that `required_satisfied` for local engineering comes from `gates --execute-local`, and the details object keeps `protected_github_policy`, `release_authority`, and `ci_passed` false.

## Adversarial probe of the pinned ELF

Probe script and `probe-result.json` are in `raw/`. A foreign `/bin/sleep 180` (pid 5203, starttime 288990, comm `sleep`) was alive with that same starttime through every case. Signals were pid plus starttime only.

| Case | Result |
| --- | --- |
| positive `echo hello-a096` | `containment=verified`, `note=ok`, `exit_code=0`, `cleanup_incomplete=0`, `output_limited=0`, `cgroup=0`, stdout `hello-a096\n`, elapsed 0.0773s |
| setsid then wait, 30 iterations | verified 30, recorded pid gone 30, alive 0, inconclusive 0, max elapsed 0.1311s, foreign starttime unchanged |
| immediate setsid, 5 iterations | verified 5, gone 5, alive 0, max elapsed 0.1275s |
| inherited stdout held by a setsid writer, limit 4096 | verified, `note=output-limit`, writer pid 5571 gone, size stayed 4096, inode 95921, sha256 unchanged after the return |
| limit `0` | helper exit 82, `containment=unavailable`, `spawned=0`, `note=protocol-malformed`, `cleanup_incomplete=1`, stdout file not created |
| SIGTERM during `sleep 30` | verified, `note=stopped`, cleanup 0, direct pid 5579 gone, elapsed 0.2989s |
| exact 16MiB | size 16777216, `output_limited=0`, `note=ok`, `exit_code=0`, inode and sha256 stable |
| one byte over 16MiB | size 16777216, `output_limited=1`, `note=output-limit`, helper status `exit_code=0`, inode and sha256 stable |
| fast 32MiB | size 16777216, `output_limited=1`, `note=output-limit`, helper status `exit_code=0`, inode and sha256 stable |
| SIGKILL of the supervisor after the setsid grandchild was recorded | helper pid 5608 return -9, status file absent, grandchild pid 5613 starttime 289828 still alive, foreign sleep still alive. Probe then `kill_owned` that grandchild by pid and starttime and got `killed` |
| 40 double-forks of `sleep` | verified, `note=ok`, cleanup 0, elapsed 0.0852s, `survivor_sleeps` empty |

The positive Linux command sealed with cleanup proven. It was not marked unavailable.

The helper status for the over-limit cases still says `containment=verified` and `exit_code=0` with `note=output-limit`. Seal refusal is in xtask: `supervise` rewrites a 0 exit to -1 when `output_limited` is set, `blocks_pass` is not where that refusal lives, and `gates.rs` adds `gate.output_limit` so `required_satisfied` is not issued. `suites.rs` `supervise_fixture` likewise turns `output_limited` into an error. The passing `output_limit_is_exact_and_stops_storage` test is that layer. `OUTPUT_LIMIT_BYTES` is `16 * 1024 * 1024`.

`note=stopped` is a successful settle, not a bypass. `note_fails` allows `ok`, `stopped`, and `output-limit`. `note_blocks` does not list `stopped`. The runner's own deadline sends `SIGTERM` to the supervisor pid and sets `timed_out`, and a timed-out command cannot seal. An external stop that the helper finishes settling, with a zero payload exit, can pass `blocks_pass` because the descendant check already passed.

After the probe, `/proc/5203`, `/proc/5613`, and `/proc/5608` were absent. `foreign_cleanup` in the JSON is `still-alive` because `kill_owned` still saw starttime 288990 50ms after `SIGKILL`, which is what an unreaped zombie looks like. The foreign sleep was not killed by the supervisor.

Cgroup mkdir of `/sys/fs/cgroup/oo-a096-probe` returned `errno=30` (`EROFS`). The positive echo sealed with `cgroup=0`. The branch that arms a cgroup and then fails `cgroup.kill` was not executed. In the source, an armed cgroup whose kill write fails, or whose `cgroup.events` `populated` is not `0`, sets `cgroup-populated`, and that note blocks a pass. This container's read-only cgroup mount is a capability limit. It is not universal host isolation and it is not runsc.

## Settle proof

`settle_step` returns `Settled` only when the children snapshot is complete, the pid count is 0, and `waitpid` returned real `ECHILD`, on two consecutive observations. `EINTR` is not `ECHILD`. A truncated read of exactly 64KiB, an unreadable children file, or a nonempty list resets the counter and can set `list-unreadable`. After `Settled`, `confirm_empty` requires one more empty list and one more `ECHILD`. `SETTLE_PASSES` is 40 with 20ms between passes. Three bare `ECHILD` results are not the proof. The implementation matches that statement.

What the proof covers when the supervisor stays alive through the confirm:

- A child whose parent is still alive is not on `/proc/self/task/*/children` yet. The live parent is. An empty list cannot happen while that parent remains a child of the supervisor. After the parent exits, the grandchild is reparented to this subreaper and the next pass sees it.
- `read_children` walks every `/proc/self/task/*/children` entry, so thread children are in the snapshot. A bad task entry fails the list and blocks the seal.
- `parse_stat` uses the last `)`. A parenthesis in `comm` cannot shift `ppid` or `starttime`. `comm` is compared across the two reads and is not a kill key. The unit test covers `(we)ird)` and a short line.
- Pid reuse with a different starttime across the two reads is `Unreadable`, which becomes `identity-uncertain` and blocks a pass. Pid 1 is never signaled.
- `set_child_subreaper(true)` runs on the helper process for that one command. xtask is not a subreaper. The attribute dies with the helper. It is not a process-global subreaper left on the machine.

What the proof does not close:

- After `confirm_empty` returns and before the process exits, including `join_bounded`'s 500ms wait, a fork already in flight can be reparented here or, if the supervisor exits first, to init, and the status can already say `verified`. The 40-wide fork storm returned verified with no surviving `sleep` in 0.0852s. That run did not hit the window. The window is still in the code.
- A grandchild in another pid namespace, or one adopted by an intermediate subreaper that exits after the last confirm, is outside the snapshot.
- `kill_pid` sends `killpg(pid, SIGKILL)` and then `kill(pid, SIGKILL)` after the identity check. A same-uid process that has joined that process group can be signaled. The probe's foreign sleep, in its own group, survived. This is not `pkill` and not a `comm` sweep. The deadline path uses `/bin/kill` or `/usr/bin/kill` with `-TERM` and the supervisor pid, and it refuses pid `<= 1`.
- A pid that is reused in the gap between the second `stat` read and the kill syscall can be signaled. That gap was not demonstrated.
- `PDEATHSIG` `SIGKILL` is set on the `--launch` child only. A `setsid` grandchild does not receive it. `SIGKILL` of the supervisor before `write_status` left grandchild 5613 alive and wrote no status. xtask treats a missing status as `status-missing` with `cleanup_incomplete`, which blocks `required_satisfied`. The return is bounded and unverified. It did not seal.

Same-uid private copy: `launch_supervisor` hashes the pinned bytes, writes `helper-<pid>-<seq>` mode `0700` under `CARGO_TARGET_DIR/oo-xtask-contain` (or beside the current executable), hashes that path again, then `Command::new(path).spawn()`. The kernel executes the path, not the buffer that was hashed. A writer with the same uid can replace the file between the second hash and `execve`. A different uid cannot write the `0700` file or directory. This container ran as uid 0, so another uid-0 process in the same mount namespace is inside that model. No racer was run. The committed pin is unchanged by the existence of the window. This assessment is from the code and the container uid, not from a repeated footnote.

## Supply

Helper lock graph, checksums matched to the crates.io version objects (HTTP 200), `yanked` false:

| Crate | Version | Checksum | `created_at` | rust-version | License | Age at `2026-10-02T11:25:15Z` |
| --- | --- | --- | --- | --- | --- | --- |
| nix | 0.31.3 | `cf20d2fde8ff38632c426f1165ed7436270b44f199fc55284c38276f9db47c3d` | `2026-05-11T07:01:18.650221Z` | 1.69 | MIT | 3460h |
| libc | 0.2.189 | `3eaf3ede3fee6db1a4c2ee091bf8a8b4dccdc6d17f656fb07896ee72867612f2` | `2026-07-21T21:33:28.018110Z` | 1.65 | MIT OR Apache-2.0 | 1742h |
| bitflags | 2.13.2 | `3ded4057c258ba199e2d26386d3af3780957ecaee6c4ef4041c6b4b8b97c0b06` | `2026-09-10T01:11:38.500809Z` | 1.56.0 | MIT OR Apache-2.0 | 538h |
| cfg-if | 1.0.5 | `4e7648175b45a9a48536d676f68d918270699102aa8dab5496df06904c914600` | `2026-09-16T17:36:11.278420Z` | 1.32 | MIT OR Apache-2.0 | 378h |
| cfg_aliases | 0.2.2 | `f079e83a288787bcd14a6aea84cee5c87a67c5a3e660c30f557a3d24761b3527` | `2026-07-16T14:57:16.699789Z` | field absent | MIT | 1868h |

All five are older than 168 hours. `cfg_aliases` has no `rust-version` in the version object. None is invented here. The helper crate's own `rust-version` is 1.94.0. `nix` is pinned `=0.31.3`, `default-features = false`, feature `signal` only. In the fetched `nix` 0.31.3 `Cargo.toml`, `signal = ["process"]`, `socket = ["memoffset"]`, and `rust-version = "1.69"`. `cfg_aliases` is a build dependency of `nix` and is in the lock for that reason.

The crate document for `nix` has `crate.created_at` `2014-11-11T22:15:48.567559Z` and 167 `created_at` strings. Version `0.31.3` is the newest version entry and its own `created_at` is `2026-05-11T07:01:18.650221Z`. The version URL is the stamp that matches the selected crate. The crate-level stamp is the wrong one. That is the a082 parser bug, confirmed on the host response. Index pubtime lines were not isolated. Checksums above are from the version objects, compared with the lock.

Vendored license files match the registry crate bytes: nix LICENSE 1097, libc MIT 1066, libc Apache 9723, bitflags MIT 1071, bitflags Apache 10847, cfg-if MIT 1057, cfg-if Apache 10847, cfg_aliases LICENSE 1076.

GitHub contents responses: `libc`, `bitflags`, `cfg-if`, and `cfg_aliases` are HTTP 404. `nix` lists one file, `RUSTSEC-2021-0119.md`. Those 404s and that listing are not an audit pass.

A shallow `advisory-db` clone at `117edb3bed98e9be112f277b7615eea3252e7c43` (`2026-10-02T08:58:33Z`, "Assigned RUSTSEC-2026-0319 to anymap2") has `crates/nix/RUSTSEC-2021-0119.md` only. Patched versions are `^0.20.2`, `^0.21.2`, `^0.22.2`, and `>= 0.23.0`. Unaffected is `< 0.16.0`. The affected function is `nix::unistd::getgrouplist`. `nix` 0.31.3 is inside the patched range. Helper source does not call `getgrouplist`. The clone has no `crates/libc`, `crates/bitflags`, `crates/cfg-if`, or `crates/cfg_aliases` directory.

`cargo-audit` and `cargo-deny` were not executed against this graph. The advisory tool gate stays pending. The 168h floor, the ISC allow entry, and the `ci/deny.toml` policy are unchanged.

## ISC shape

`ci/deny.toml` on this commit has one `ISC` entry in:

`Apache-2.0`, `MIT`, `ISC`, `BSD-3-Clause`, `Unicode-3.0`, `Zlib`, `Unlicense`, `MIT-0`.

`confidence-threshold` is 0.8. `advisories.yanked` is `deny`. `ignore` is empty. Wildcard bans stay `deny`. `unknown-registry` and `unknown-git` stay `deny`. No CDLA entry.

The only `deny_toml.rs` edits add `"ISC"` to `REVIEWED_LICENSES` and insert `ISC` into the allow list of `exception_and_low_confidence_fail`. That test still requires `supply.exception`, `supply.confidence`, and `supply.advisory_ignore`. `reviewed_shape_rejects_unknown_keys_and_weakened_values` still rejects a wildcard, `unknown-registry = "allow"`, `unknown-git = "warn"`, and unknown keys `skip-tree` and `private`. a078 approved the attribution wording and left this allow entry in place. The shape on `f1a60059` matches that single entry. This review does not reopen a078 and does not widen the license policy.

## Ownership sentence

a082's report says a073 remains the sole main-root lock writer. That sentence is stale for the current tree. The current root writer is a095 (CLI). a093 (source) and a092 (pipeline) remain live. This record supersedes the stale sentence. The a082 and a070 reports are left as they were.

## Cleanup of this review's builder

After the record's inputs were copied, `/work/release` was created in container `0c26259d5ee5348e2bc6a5c69b606acb33568fd3912000004d2c890f03f0461d`. It exited 0. The container and the two volumes named above were removed. No prune ran. No other rust-builder was running at that point.

## Outside this decision

Full SP00, secret rotation, protected CI, runsc, release authority, and a full supply approval are outside this decision. The post-confirm fork window, the same-uid hash-then-exec window, same-group `killpg` collateral, the unexecuted `cgroup.kill` failure branch, the non-byte-identical rebuild, the Darwin exec-format limit, and the pending `cargo-audit` / `cargo-deny` run are recorded bounds on the approval above.
