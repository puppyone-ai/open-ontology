# a082 report — Rust containment supervisor

Session `eef831aa-dba4-4af2-a78b-541bc9e0a082`. Task `execution/prompts/00l-rust-containment-supervisor.md`. Implementer only; no subagents. This report is the a082 handoff record. a070's historical report and handoff are unchanged.

Input commit `5ef3d6d819fd72fdd5e86f79eb9e719910f7e6e0`, tree `61350c8287dcaf1689f0181a92dfc68793959fe6`, parent `9c7b98e4f7e92ab99a064ea4a839afe30319e4df`, stamp `0fcc2f9`. The validation snapshot is a git archive of that input plus the owned overlay. The user repository moved after `a0883ed558a`. This commit's parent is `7143b2d9a7888d242983e5b9c10ef597246d5848`. `crates/xtask` and `testing/harness` have no committed delta between the input and that parent. Root `Cargo.toml` / `Cargo.lock` do (sqlx path patch). Those root files were not staged. a073 remains the sole mainroot lock writer. a070's sentence that a063 owns the root lock is stale and is corrected only here.

a093 is the connector writer. a092 pipeline work and a090 / a091 / a094 readonly reviews were live. No foreign source, index, manifest, or lock was edited.

## Artifact

Prebuilt helper, Linux `aarch64-unknown-linux-gnu`, Rust 1.94.0 (`4a4ef493e` 2026-03-02), image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

| item | value |
| --- | --- |
| path | `testing/harness/containment-supervisor/bin/containment-supervisor` |
| sha256 | `3569f4e45f88cdedef55c6eb5b23e2c4eb9e9dc1c14690085ddf72681a95628e` |
| bytes | 716792 |
| mode | 0755 |
| magic | `7f 45 4c 46` |
| embedded `SUPERVISOR_SHA256` | same digest |
| `provenance/artifact.sha256` | same digest |
| helper `Cargo.lock` sha256 | `14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1` |

Source sha256 after copy-back:

| path | sha256 |
| --- | --- |
| `crates/xtask/src/containment.rs` | `182b11f120c4fc1683eb98cf28bc4b0d231d3edd72d5640a4c19ac6569cdd600` |
| `crates/xtask/src/deny_toml.rs` | `cf84b6d6dab6f5afbb385ff1220a79aaf695aeda837259e8def5e74c98fe738c` |
| `crates/xtask/src/runner_capture.rs` | `13dd65e16a87e397c9be53c22228852b1a5b93a428f8480a889b8881a2922059` |
| `crates/xtask/src/suites.rs` | `0d7be0f50a320fe51aad0c1329b2294df9e61d0cced592b65f68b02a483b61d8` |
| `testing/harness/README.md` | `47d53bfb3e32aa9c5043468f9bc02b794c9eec2e28d1946d9e6c5adb9edca267` |
| `testing/harness/containment-supervisor/Cargo.toml` | `4c0ed714c5c5a0dfed0230fef59d6fabe5d5b4c77b3da7e01a5574cc16b82767` |
| `testing/harness/containment-supervisor/src/lib.rs` | `e9810bd659ccc2ac8adc44dc67631744d1ab40b681bdc20b9dbeb29927cb54d5` |
| `testing/harness/containment-supervisor/src/main.rs` | `7c25da40ba3aece4f1d38ea45a8f777a1c53a685bf3a9b1ed8514898e97e51c7` |
| `testing/harness/containment-supervisor/tests/branches.rs` | `6d67ed3c600025e181a2515c4d89159aa2334ff4bc211d2ba968617c1cc1997d` |
| `provenance/artifact.sha256` (file hash) | `9dc01f0abe60c60a5ac76a844c6631cafaad3c69e4feb2770288be0e051691c6` |
| `provenance/README.md` | `f78e8518121ab1f53c16742d32bbcdb5e332c0b14c31fd35b130b1c6f04b2246` |

The helper crate is a standalone workspace (`resolver = "3"`), `#![forbid(unsafe_code)]`, and is not a workspace member. Root `unsafe_code = forbid` and the xtask forbid are unchanged. Dependency pin is `nix = 0.31.3` with default features off and feature `signal` only. The helper calls `set_child_subreaper`, `set_pdeathsig`, `execvp`, `waitpid`, `kill`, `killpg`, and `sigprocmask` through those safe wrappers, plus `std::process::Command` and `process_group(0)`. No `fork`, no `pre_exec`, no `unsafe` block. The helper build is the engineering artifact. Runtime execution does not invoke cargo, rustc, or cc.

Helper tests ran the debug `CARGO_BIN_EXE`. xtask and the probe ran the release binary above. Attempt 2 and attempt 3 produced the same release digest because the helper library source did not change between those builds. That match is one toolchain result, not a bit-reproducibility claim.

## Supply

Attempt-3 `supply-check.exit` is 0. Lock checksums matched the crates.io version objects, `yanked` was false, and version numbers matched. Recorded MSRV: bitflags 1.56.0, cfg-if 1.32, libc 1.65, nix 1.69. cfg_aliases 0.2.2 publishes no rust-version. The in-container parser stored the first `created_at` in the response body (bitflags `2014-02-18`, cfg-if and libc `2019-07-19`, cfg_aliases `2017-01-27`, nix `2021-12-30`). Those stamps are not the version publish times.

A host read of each version object on 2026-10-02, same user agent, returned:

| crate | version `created_at` |
| --- | --- |
| bitflags 2.13.2 | `2026-09-10T01:11:38.500809Z` |
| cfg-if 1.0.5 | `2026-09-16T17:36:11.278420Z` |
| cfg_aliases 0.2.2 | `2026-07-16T14:57:16.699789Z` |
| libc 0.2.189 | `2026-07-21T21:33:28.018110Z` |
| nix 0.31.3 | `2026-05-11T07:01:18.650221Z` |

Each of those publish times is more than 168 hours before 2026-10-02. Checksums on that read match the lock. Yanked was false. bitflags 2.12.0 and cfg-if 1.0.2 were not selected.

nix advisory contents API exit 0 listed one file, `RUSTSEC-2021-0119.md`. libc, bitflags, cfg-if, and cfg_aliases advisory curls exited 22. That is a directory listing plus four HTTP 404s, not a full rustsec database pass. The helper source does not call `getgrouplist`.

Full license texts are in `testing/harness/containment-supervisor/provenance/licenses/` (11 files: Apache and MIT for bitflags, cfg-if, and libc; LICENSE and NOTICES for cfg_aliases; nix LICENSE; helper NOTICE and NOTICE-APACHE).

## ISC shape

`REVIEWED_LICENSES` is Apache-2.0, BSD-3-Clause, ISC, MIT, MIT-0, Unicode-3.0, Unlicense, Zlib. The exception fixture allow list includes that same ISC entry. Unknown-key, unknown-value, confidence, yanked, source, and forbidden-license assertions remain. `ci/deny.toml` was not edited. No CDLA entry and no wildcard exemption were added.

## Attempt 3 acceptance

Run id `a710bf86-79a0-4dee-a264-b89855676aff`. Container `oo-a082-rust194-a710bf86` (`6ff409df3c48`). At launch, `global_rust=1` (one foreign `oo.kind=rust-builder`), host cargo/rustc count 0, so one own builder was allowed. Posture: `--pull never`, `--platform linux/arm64`, `--init`, `--user 0:0`, `--security-opt no-new-privileges`, `--cap-drop ALL`, memory 8g, memory-swap 8g, cpus 4, pids 4096, bridge only until fetch finished, then the network was disconnected. Private cargo and target volumes. No socket, no host network, no host HOME, no prune.

Phase exit files under `attempt-3/`, read before copy-back:

| phase | exit |
| --- | --- |
| supply-check, helper-lock, helper-fetch, workspace-fetch, clippy-component | 0 |
| helper-test, helper-clippy, helper-build, pin | 0 |
| xtask-lib, xtask-cli, xtask-clippy | 0 |
| probe, fetch-exec, test-exec, docker-run, launch | 0 |
| advisory-nix | 0 |
| advisory-libc, advisory-bitflags, advisory-cfg-if, advisory-cfg_aliases | 22 |

Launcher exit 0 is the `launch.exit` file. The phase files above are the acceptance record.

- Helper unit tests: 3 passed. Branch tests: 8 passed in 0.75s (`fork_chain_is_gone`, `immediate_setsid_double_fork_is_gone`, `recorded_setsid_double_fork_is_gone`, `protocol_malformed_does_not_spawn`, `exact_output_seals_and_one_extra_byte_stops_the_file`, `held_stdout_does_not_grow_after_return`, `held_stderr_does_not_grow_after_return`, `timeout_reaps_the_payload_and_returns_bounded`). Doc tests: 0.
- Helper Clippy `--all-targets -D warnings`: finished, exit 0.
- xtask lib: 39 passed, 0 failed, 3.65s. Includes `missing_tampered_and_untrusted_helpers_fail_closed`, `malformed_helper_protocol_does_not_spawn_or_touch_a_foreign_sleep`, `exception_and_low_confidence_fail`, `reviewed_shape_rejects_unknown_keys_and_weakened_values`.
- xtask CLI: 16 passed, 0 failed, 17.94s. Includes failed-child refusal, deadline reap with a spared foreign process, setsid descendant bounded return, output-limit refusal, path-kill absence, and shim `required_satisfied` with replay refused.
- xtask Clippy: finished, exit 0.
- Probe `probe_result=pass`. Foreign sleep pid 5043 starttime 68069 before and after. Exact 16MiB capture `exact_size=16777216 limited=0 exit=0`. Plus-one and fast overshoot stayed at `16777216` with `limited=1`. Identity checks used the payload pidfile and `/proc/<pid>/stat` starttime. The probe does not read `/proc/*/cmdline`.

Settle proof is two observations of empty `/proc/self/task/*/children` and `ECHILD` (not `EINTR`), then the same pair again. `cgroup=0` under `--cap-drop ALL` does not fail a settled seal.

## Preserved failures

| tree | file | exit |
| --- | --- | --- |
| run 1 (parent directory) | `launch.exit` | 1 |
| run 1 | `helper-clippy.exit` | 101 |
| attempt 2 | `launch.exit` | 1 |
| attempt 2 | `helper-clippy.exit` | 101 |
| attempt 2 | `xtask-lib.exit` | 101 |

Attempt 2 probe was already `probe_result=pass` with digest `3569f4e4…`. Its launch stayed 1 because helper Clippy and xtask lib were 101. Those files were not rewritten. Run 1 remains the license-shape and first-implementation failure record.

## Limits

- A child can still fork after the last empty-children confirmation and before the supervisor returns. Two clean observations are the proof that ran. Three historical `ECHILD` waits are not that proof.
- Grandchildren in another pid namespace are outside this supervisor's `/proc` view.
- `SIGKILL` of the supervisor before it reaps can leave grandchildren. The direct child uses `PDEATHSIG` via a `--launch` re-exec.
- The private `0700` copy is rehashed, then executed. A same-uid writer can replace that copy between the second hash and `exec`.
- cgroup setup is best-effort. `cgroup=0` is the expected result under `--cap-drop ALL`. A successful mkdir followed by a failed `cgroup.kill` is `unverified`. This is not host isolation, cgroup enforcement, or runsc.
- macOS `supervise` returns `unavailable` with note `platform` and does not exec this Linux binary.
- Kills use a known pid whose ppid, starttime, and comm match across two `/proc/pid/stat` reads. comm is compared so a parenthesis cannot shift fields. comm is not a kill key. Direct-child signals also require the starttime recorded at spawn. No `pkill` and no process-name sweep.
- Positive Linux seals use the pinned helper. A missing, mismatched, or untrusted helper fails closed. Imported captures are not runner authority.
- The full suite matrix stays at the integration node. This run is the owned xtask lib, xtask CLI, helper, and probe on the archived input.
- No SP00 acceptance, secret-gate acceptance, protected-CI acceptance, runsc acceptance, or release acceptance is claimed.
- Advisory coverage is the nix directory listing plus four 404s.
- The in-container age field is the first `created_at` in the body. Version age is the host read in the table above.

## Cleanup

After attempt 3, the EXIT trap removed container `oo-a082-rust194-a710bf86` and volumes `oo-a082-cargo-a710bf86` and `oo-a082-target-a710bf86`. Rechecked immediately before this report: `docker ps -a --filter label=oo.owner=a082` empty, `docker volume ls --filter name=oo-a082` empty, `docker ps --filter label=oo.kind=rust-builder` empty, and no host process with comm `cargo`, `rustc`, or `rustdoc`. No prune. No foreign container, volume, or process was signaled. The attempt-3 watcher was not running.
