# DL01-W1-35 fixed driver-bridge review

Verdict: approved for one bounded supervisor child.

The frozen source `7de78381be5f692ae53c2559b759ce00ad807303` meets the original process, Arrow, cancel, deadline, pin, input, crash, and descriptor exits that the card requires of this worker. The approval is that supervisor. Oracle, a native driver, an arbitrary typed schema, an OCI image pin, kernel-network enforcement, the source-consumer contract, DBX-W04, and AT-SRC-10 stay pending.

Session `5e87b548-7b74-4da7-9bca-4be1d16a310f`. Executor Grok 4.7 / xhigh. Prior review of source `cdba5f3992104dba9d70ed9e3b50358c6df19772` remains `request_changes` in commit `e93a62dedc5910603377765209ecfaca1e158542`. That failure is unchanged on disk: `observations/env-fd.txt` still records `saw_canary_fd=1`.

## Identity

| Object | Value |
| --- | --- |
| Base archive | `577160c22de63adb9b38e0147f3bc2da893c4d9d` |
| Author content | `7de78381be5f692ae53c2559b759ce00ad807303` |
| Content tree | `6091b9a968ace5aa08010bde3384ca2d67786c94` |
| Content parent | `1b5e2c4e950aaab7b0615fcf3258a197ce09f602` |
| Content subject | DL01-W1-04: close inherited descriptors before exec. |
| Evidence seal | `01a7d6cffaaf013ef56377514cab445ebf72599e` |
| Seal tree | `c217d7826acfb156f978049acc1e5c039037f1d9` |
| Seal parent | the content commit |
| Tool tree at content, seal, and report-time HEAD | `5e82d3faa56225b281498d0fae518f703c6c40b6` |
| Previous tool tree | `46b8c15eb90bcf3a2e4e8c7f3ee486505d564a53` |
| Report-time HEAD | `cbf03fe0ef4443c27b2e7f2eaee539a519ea8250` |
| Overlay-time HEAD in `linux/fix-proof/provenance.txt` | `131279c54572f612d10945ccb5a17a3a9c638f9e` |

Content is an ancestor of the seal and of report-time HEAD. `git diff` of `tools/source-driver-worker` from content to the seal is empty, and from content to report-time HEAD is empty. The author seal packet `evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/driver` rehashed from Git blobs: 634 files checked, `packet_check_fail=0`.

The compile input was `git archive` of the base plus `git archive` of the tool at the content commit, in `/tmp/oo-dl01w1-35-fix-5e87b548`. The base archive does not contain `tools/source-driver-worker`. Host `shasum -a 256 -c` of the 13 tool blobs against that overlay exited 0. The worktree of the tool was dirty and was not compiled or hashed.

## Compile gate

One Linux arm64 Rust 1.94 builder, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, `--cpus 1`, `--memory 6g`, `CARGO_BUILD_JOBS=1`. Admission under `/tmp/oo-dl01-wave1-resources.lock`: own rust builders 0, global rust builders 0, ancestor containers 1 (`oo-a419-product-run` on the same image, left alone), host cargo/rustc 0, memory free 38 percent, Docker memory 101060706304 bytes.

Container `9ad285718944988d6cbabb8b6062022fd2b5b0cead9d3496891df67127f71322` (`oo-dl01w1-35-fix`). Kernel `6.12.76-linuxkit`. `rustc 1.94.0 (4a4ef493e3a1488c6e321570238084b38948f6db) 2026-03-02`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, cargo `1.94.0 (85eff7c80 2026-01-15)`. OOMKilled false. Memory 6442450944. NanoCpus 1000000000. CapDrop ALL.

| Phase | Exit |
| --- | --- |
| source sha256 | 0 |
| fetch offline, then fetch | 0, 0 |
| build bins | 0 |
| author `cargo test` | 0, 24 passed, 0 failed, 8.98s |
| clippy `-D warnings` | 0 |
| ldd / vendor scan | 0, vendor match file empty |
| first harness | 101, 28 passed, 1 failed |
| ENOSYS preload | 0, marker absent |

The container wait exit is 101 because that harness fixture failed. The fixture failure is recorded below and was rerun. Author, clippy, ldd, and ENOSYS had already finished.

Binaries, debug profile, `linux/fix-proof/bin-sha256.txt`:

- parent `301445e5c0e790681d2bfc40c1dad933f2639049da19fef52a1df4ff3b5a10f1`
- child `f1e64eb92894e1e8b73dc638568d979affcb3fd369d0206b71b78d71b88bdde9`
- `oo-net-probe` `eb4a948bb4757b4a24b44a0acbf811127ea0cdfb49c707e5a2f82cb99deb6688`

`ldd` of parent and child: `linux-vdso`, `libgcc_s`, `libm`, `libc`, `ld-linux-aarch64`. Cargo tree links `source-driver-isolate` to `libc 0.2.190`, and also reaches `libc 0.2.190` through `getrandom` and `cpufeatures`. No Oracle, ODBC, or JVM library is in the tree or the vendor scan.

A second builder, `2f56dc3b1a94e6a29ed0c7ba2b785ab521d9af654b33e99836136b5b8d7962a2` (`oo-dl01w1-35-fixrerun`), ran only the corrected harness against the same overlay and volumes. Admission again: own 0, global 0, ancestor 1, host cargo 0, free 38 percent. Exit 0. 29 passed, 0 failed, 24.66s.

## Descriptor isolation

The new crate `source-driver-isolate` is the only unsafe surface. `linux/fix-proof/unsafe-lints.txt` shows `forbid(unsafe_code)` on the supervisor library and on the parent, child, and net-probe bins, plus `Cargo.toml` `unsafe_code = "forbid"`. The isolate crate is `deny(unsafe_code)` with `allow(unsafe_code)` on `spawn_isolated`, `mark_cloexec_above_stdio`, and `duplicate_without_cloexec`.

`spawn_isolated` on Linux checks `libc::CLOSE_RANGE_CLOEXEC == 4` in the parent, then registers `pre_exec(mark_cloexec_above_stdio)`. That function calls `close_range(3, c_uint::MAX, 4)`. The success path is one syscall. The hook is a function pointer and captures nothing. It runs in the forked child after Rust 1.94 has placed stdio on fds 0, 1, and 2, and before `execvp`. The parent descriptors are not closed and their flags are not changed. `CLOSE_RANGE_CLOEXEC` leaves the exec-status socket open until exec, so a failed exec can still report an errno. Closing the range would drop that socket.

The error path uses `io::Error::last_os_error` and `from_raw_os_error`. Rust 1.94 stores an OS code inline in `io::Error`. This review treats the success path as the async-signal-safe syscall. The three observed failures returned the OS errno to the parent. A formal POSIX listing of `std::io::Error` is not claimed.

`duplicate_without_cloexec` is `F_DUPFD` from 3. It is a test helper. It is not on the spawn path.

On a non-Linux target, `spawn_isolated` returns `ENOSYS` and does not spawn. The parent maps that errno to `ParentError::Config("isolate")` and any other spawn error to `Config("spawn")`. Both return before a child image runs.

The builder kernel is 6.12, so `close_range` exists there. ENOSYS was forced with `LD_PRELOAD` of `harness/enosys-close-range.c`, which returns -1 and sets errno to ENOSYS. The preload was applied only to `w35-enosys-check`. That process first called `close_range` itself and required ENOSYS, then called `spawn_isolated` on a worker whose only action is `/bin/touch` of a marker, then called `ParentConfig::start`. Results in `linux/fix-proof/enosys.stdout`: `spawn_isolated_enosys_ms=0`, `parent_isolate_ms=0`, `marker_absent`. Stderr is empty. Exit 0. There is no second worker and no path that execs with the inherited table left open.

Exec-failure probe, decisive run `observations-fix/exec-error-pipe.txt`: missing path errno 2 (`ENOENT`) in 0 ms, mode `000` regular file errno 13 (`EACCES`) in 0 ms. Root still receives `EACCES` when the file has no execute bit. The first harness run's mode `0755` text file is preserved as a fixture miss, not as a product failure. glibc `execvp` runs a shebang-less text file with `/bin/sh`, so spawn returned `Ok` and the shell wrote `w35: not found`. That log is `linux/fix-proof/harness-test.stderr`. The canary assertion was not changed.

## Independent process and Arrow values

Author suite, all 24 named tests in `tests/bridge.rs`, including `inherited_pipe_file_and_socket_stay_in_parent` and `cancel_kills_group_grandchild`: 24 passed. The review harness canary `child_stdin_env_and_fd_boundaries` still asserts `saw_canary_fd == 0` and passed. `observations-fix/env-fd.txt` shows an empty environment, three sockets, `saw_canary_env=0`, `saw_canary_fd=0`. The historical file `observations/env-fd.txt` still shows the canary path and `saw_canary_fd=1`.

`caller_descriptors_stay_open_and_do_not_leak` opened a file, a pipe, a Unix socket, and `F_DUPFD` of the file at fd 40, cleared `FD_CLOEXEC` on all four, and recorded `F_GETFD` and `F_GETFL`. After the executed probe wrote `fds.txt`, the same numbers were still open and both flag pairs were unchanged: file `(0, 131074)`, pipe `(0, 0)`, socket `(0, 2)`, high fd `(0, 131074)`. The child link list did not contain the file path, `pipe:[8969099]`, or `socket:[8969100]`. The parent then read `w35-file-canary`, `w35-pipe-canary`, and `w35-socket-canary`. The first row was `k=35011`, `v=w35-quartz`. The receipt was Arrow IPC. Fallback was false.

Clean completion in `observations-fix/clean-receipt.txt`: 3 rows, 1 batch, fingerprint `442ab712912be03a68a4d459c7a9c23c1b66fb4fcb35aeb491c483dd7ed71769`, `kernel_network=pending`, admits-import, fallback, and vendor-library flags false. Rows were `35011/w35-quartz`, `35012/w35-slate`, and `35013` with `v` absent. Stdin contained `secret://w35/source/reader` and omitted `secret://w35/target/hold`. The executed probe digest was `sha256:ff703bd9ae77e9154c9adecba061cafb1ede92de9f3fc4a008ee0bff28f2a16b`. The builder image id is recorded beside it and is not a pin input. `driver-artifact.bin` was hashed. `artifact_executed=false` and `fallback_executed=false`.

Wide cancel: `progress=1`, `cancel_ms=11`, signal 9. Group cancel: old pid 119, grandchild 121, new pid 123, `cancel_ms=26`, the stale pull errored, and the next pull returned `35077/w35-after-cancel`. Zero-length frame: malformed in 405 ms. Declared length `0x7fffffff`: malformed in 453 ms, parent deadline 8000 ms. Lineage `arrow_bytes` of 1 was accepted. The parent checks that the length is nonzero and inside the cap. That observation is not a new exit.

Schema metadata, field metadata, and a float64 column were rejected. Missing, symlink, and digest-mismatched drivers were refused before exec. A symlink worker was refused. Duplicate keys and Agent DB, model, and Gateway credential material were refused before spawn. Exit 0 without completion is not a receipt. Completion plus a nonzero exit is not a receipt. `finish` before end is a protocol failure. `oracle:select-1` returns `read_unsupported`. The artifact and `fallback.sh` were not executed.

## Pin classification

Two different checks exist.

The library hashes the file that will be executed and the regular file `driver-artifact.bin`. `observations-fix/pin-library.txt`: the request digest matched the original file, the bytes on disk had been flipped, the flipped file was not executed, and the builder image id is not the digest input. `oci_id_is_not_match_digest_input=true`.

The CLI compares the caller JSON strings for `image_digest`, the driver digest, and the volume with the process flags before `CallerRequest::try_from_slice`. `observations-fix/pin-cli-string.txt` and `pin-cli-file.txt` both exit with status 2 and token `image_digest`. That is a caller-string gate. It is not an inspection of an OCI image that was pulled or run.

The OCI parent image pin stays pending. `KernelNetwork::Pending` stays a constant. The author test that asserts the constant is not a network proof. The earlier prefix-35 accept-counter harness was not rerun and is not graded again. Listeners in that historical run counted accepts. They were not a database.

## Scope a later consumer can use, and the blocker

Usable now: one supervisor child for one pinned local executable, a regular-file artifact digest, Arrow IPC columns `k:int64` and `v:utf8`, the target credential ref omitted, the child environment cleared, and every descriptor at or above 3 marked close-on-exec in the forked child before exec. Caller descriptors stay open with the same `F_GETFD` and `F_GETFL` values. Stdio and the batch protocol still return rows.

The `v` column is utf8. A JSON document stored in that string is still a string. Oracle NUMBER, temporal, and binary values are outside this schema. `driver-artifact.bin` is hashed and is not loaded. No native Oracle child is present. The parent links libc and libgcc, not a vendor driver.

The source-consumer contract remains pending before source integration. The file-worker parent in the base archive, `crates/oo-connectors/src/file_worker_parent.rs` `spawn_child` at line 910, still uses `env_clear` and does not call `close_range`. This review did not edit it. DBX-W04 remains open. AT-SRC-10 remains pending. No whole-product claim is made.

## Preserved failures

- Previous source `cdba5f39`, harness rerun 2: 26 passed, 1 failed, `child_stdin_env_and_fd_boundaries`, `saw_canary_fd=1`. Logs under `linux/harness-rerun2/` and `observations/env-fd.txt`.
- Author proof-1 through proof-8 and the later proof-9 compiler run stay in the sealed author packet. This review did not rewrite them.
- This round's first harness exit 101 stays in `linux/fix-proof/harness-test.stdout` and `harness-test.stderr`. The decisive harness log is `linux/fix-harness-rerun/`.

## Cleanup

Both builders were removed by their launch scripts. At `2026-10-09T18:54:44Z` the owned network `oo-dl01w1-35-net` (`741311d15092`) was removed, exit 0. Volumes `oo-dl01w1-35-cargo` and `oo-dl01w1-35-target` and the pinned Rust image remain. No foreign container was removed. `linux/cleanup.txt` from the previous round was left as it was. Record: `linux/fix-proof/cleanup.txt`.
