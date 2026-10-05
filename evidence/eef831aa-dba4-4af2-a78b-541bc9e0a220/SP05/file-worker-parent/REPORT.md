# a220 file worker parent

Development UUID `eef831aa-dba4-4af2-a78b-541bc9e0a220`. This is the Linux proof for the trusted file-parser worker parent. It does not accept SP05-W02, AT-FILE-02, or the phase ledger, and it is not a release.

## Pins

| Item | Value |
| --- | --- |
| Fixed archive | `c2db9c430b822a824a1062b6c3c7345924df7dd8` |
| Parent of the source commit | `543580768dec60d127eb428c09e0a9a400b18272` |
| Source commit | `bf30b717029d0715e0a1abb7fc8ec02ab7630534` |
| Worker already in the archive | `6bd2455849da2527973c136968d4549ecd98d770` |
| Image | `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` |
| Toolchain | Rust 1.94.0, `aarch64-unknown-linux-gnu`, `RUSTUP_TOOLCHAIN=1.94.0` |
| Builder | one container, 6 GiB, 1 CPU, `CARGO_BUILD_JOBS=1`, `--cap-drop ALL`, no PostgreSQL |

Recipe origin is `recipes/ORIGIN.md`. The a204 launch hashes are recorded there. This run uses one container, 6 GiB, and 1 CPU. The git object was not modified. The private extract received the overlay only after the pre-overlay smoke exited 0.

## Decisive run

`linux/build06` exited 0.

| Phase | Exit |
| --- | --- |
| `docker-run` | 0 |
| archive smoke `file_parser_worker` | 0, 12 passed |
| overlay copy | 0 |
| `file_worker_parent` | 0, 8 passed |
| Clippy `-D warnings` on the lib and that test | 0 |

Host config: `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`, `OOMKilled=false`. Container `oo-a220-build06` was removed by the launch script.

Stall line from that run: `stall_elapsed_ms=801 kind=TimedOut exit=None signal=Some(9) group=Sent reaped=true alive=false`.

`linux/build05` is the same product bytes without `--nocapture` and also exited 0. `linux/build01` through `linux/build04` are preserved failures (compile, then Clippy, then the CSV protocol mismatch).

## What the parent does

`ParentConfig` accepts an absolute regular executable and a deadline of 1..=120000 ms. The clock starts before spawn and covers the hash, the request write, the frames, the kill, the reap, and the IO join. Socket waits are at most 20 ms. The child is `process_group(0)` with a cleared environment and no shell. `/bin/kill -KILL -- -<group>` runs only after the Linux `/proc` identity check. `Child::kill` still runs while `try_wait` returns `None`.

`ParentParseComplete` is returned only after a verified completion frame, a clean stdout EOF, exit 0, and that wait. `admits_import`, `published`, and `pg_run_complete` are false. The parent does not mint a public reader token and does not build a PostgreSQL `RunComplete`. `KernelRestriction::Unsupported` is not a containment certificate. A descendant that has called `setsid` is outside the group and is not killed.

Arrow IPC is decoded with `StreamReader`, one batch at a time. The caller must ask for the next batch before the parent reads the next pair.
