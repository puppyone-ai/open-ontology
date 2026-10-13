# Findings

## Prior request is closed on this source

Review `e93a62dedc5910603377765209ecfaca1e158542` requested a change because a non-CLOEXEC canary opened in the parent was visible in the child. On source `cdba5f3992104dba9d70ed9e3b50358c6df19772` the observation was `saw_canary_fd=1`, `saw_canary_env=0`. That file is still `observations/env-fd.txt`.

On source `7de78381be5f692ae53c2559b759ce00ad807303` the same assertion passed. `observations-fix/env-fd.txt` records `saw_canary_fd=0`, an empty environment, and three sockets. The author test `inherited_pipe_file_and_socket_stay_in_parent` passed, and the independent file, pipe, socket, and fd-40 probe passed with caller flags unchanged.

No new request_changes item is opened. The original exits that this card assigns to the supervisor passed on the frozen source.

## Isolate safety

Unsafe code is confined to `tools/source-driver-worker/isolate`. The supervisor crate and its three bins keep `forbid(unsafe_code)`. The hook is `pre_exec` of `close_range(3, UINT_MAX, CLOSE_RANGE_CLOEXEC)` in the forked child. The flag value 4 matched libc on this builder, checked by `debug_assert` before spawn. The parent process does not close caller descriptors and does not change their flags.

`ENOSYS` fails closed. An `LD_PRELOAD` that makes `close_range` return `ENOSYS` produced `ParentError::Config("isolate")` and left the worker marker absent. The preload did not wrap the author suite or the review suite. The kernel in the builder is 6.12.76-linuxkit, so the native syscall is present. The `ENOSYS` result is from the preload, which the checker verified in-process before spawn.

The exec error pipe returned `ENOENT` for a missing path and `EACCES` for a mode `000` file, each in 0 ms. The first harness's `0755` text file is a fixture miss: glibc `execvp` hands a shebang-less file to `/bin/sh`, spawn returns `Ok`, and the shell reports `w35: not found`. That stderr is kept under `linux/fix-proof/`. The corrected probe is `linux/fix-harness-rerun/` and `observations-fix/exec-error-pipe.txt`.

## Pin classification

The executed local file is pinned by SHA-256 inside the library, and the child checks `current_exe`. A flipped file does not run. The CLI also rejects a request whose `image_digest` string differs from the flag, status 2, token `image_digest`, before the request is parsed. Those are different gates. The OCI parent image was not the object either gate hashed. The OCI pin stays pending.

## Schema and the Oracle blocker

The schema fingerprint is `442ab712912be03a68a4d459c7a9c23c1b66fb4fcb35aeb491c483dd7ed71769`. The columns are `k:int64` and `v:utf8`. Metadata, field metadata, and float64 were rejected. Encoding a value as a JSON string in `v` leaves it a string. NUMBER, temporal, and binary values are outside this schema. `driver-artifact.bin` is hashed and was not executed. `oracle:select-1` is `read_unsupported`. Fallback was not executed. This is not an Oracle driver and not a native child.

## Lineage byte length

A batch whose `arrow_bytes` was 1 was accepted, pulled, and finished. The parent checks a nonzero length inside the cap. The behavior is recorded in `observations-fix/lineage-arrow-bytes.txt`. It is an observation. It is not an added exit and not a waiver.

## Network

`kernel_network` on the clean receipt is `pending`. The author assertion of that constant passed and is not a route proof. The prefix-35 accept-counter harness from the previous round was not repeated. Its listeners counted accepts. They were not a database. No config boolean was graded.

## What stays pending

The source-consumer contract is pending before source integration. DBX-W04 stays open. AT-SRC-10 stays pending. The file-worker parent at `crates/oo-connectors/src/file_worker_parent.rs:910` still clears the environment and does not call `close_range`. This review did not edit that file.

## Preserved failures

Author proof-1 through proof-8, the proof-9 compiler run, the first review's fetch and harness logs, and this round's harness exit 101 remain in their original files.
