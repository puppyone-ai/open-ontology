# Dispositions

a062 terminal review `request_changes` is the source. These rows are the harness response. They are not a new product gate.

| a062 point | Disposition |
| --- | --- |
| Early parent exit plus `setsid` double-fork left children alive while `required_satisfied` was true. 15/15 repeats. | Fixed for the Linux supervisor. The independent probe ran 15 immediate exits and 15 parent-wait exits. Every immediate case reported `containment=verified` with `sleep 121` count 0. Every recorded grandchild pid was gone at return and 300 ms later. The lib tests `early_setsid_exits_before_the_grandchild_is_recorded` and `early_setsid_double_fork_is_reaped_or_refused` passed on Linux. A pass requires settled `waitpid` ECHILD from that command’s subreaper. |
| Held stdout inode kept growing after return. | Fixed. Capture is a pipe into a file capped at 16777216 bytes, closed and fsynced after reap. The probe’s side file was the same size at return and 300 ms later. Captured stdout stayed at 16777216 for the hold, the exact `dd`, the extra byte, and the fast overshoot. |
| Passing tests did not prove containment. | The Linux branches require `containment=verified`, a real spawn, and dead descendants. They fail if Linux only refuses. macOS asserts `containment=unavailable`, `spawned=false`, and no leaked sleep. Execution is not skipped and not `#[ignore]`. |
| Malformed or partial listings, and a changed start token, were described as a kill failure. The a062 injection did not reproduce that. | Not claimed as a reproduced EPERM kill. A partial or unreadable child list does not by itself succeed. Stable ECHILD is the success condition. A start-token mismatch on the fallback owned-tree path sets identity-uncertain and does not kill the reused pid. Unknown cleanup stays incomplete. |
| `/proc` sampling, process groups, and start tokens cannot prove there is no unobserved `setsid` descendant after immediate reparenting. | The execute-local proof is the per-command subreaper plus settled reap. Listings are only a way to find children to signal. The global test process is not a subreaper. |
| Output bound must be storage, not only a retained prefix in memory. | Each stream stops at exactly 16777216 bytes on disk. One extra observed byte sets `output_limited` and the Rust layer forces a nonzero exit when the child exit was 0. `gate.output_limit` blocks the seal. |
| Foreign processes must be spared. No kill by global name. | The supervisor signals a pid only when `/proc/pid/stat` shows that pid’s parent is the supervisor. The probe’s foreign `sleep 180` was alive after every contained command. Fixture cleanup in tests kills pids that fixture recorded, or the marker `sleep 121` after a failed assertion. |
| Imported captures stay non-authoritative. | Linux execute-local replay still requires `gate.unverified` and rejects `required_satisfied`. On macOS the refused capture has exit -1, so import reports `gate.job_failed` / `captured status is nonzero` before that note. The replay still cannot seal. `runner_authority=local-engineering` is set only after a command actually spawned. |

## Attempt that did not land

Linux attempt 1 (`aeda77c1-bace-43a7-8eb1-9788aaf83910`) compiled no supervisor because the canonical gcc basename is `aarch64-linux-gnu-gcc-14`. Containment was `unavailable`. That is preserved. It is not evidence that signaling failed.

## Left in place

- macOS execute-local fail-closed. Follow-up is a per-command ownership primitive that can be verified, then the same seal rule. Requirements were not weakened to make macOS pass.
- cgroup v2 stays optional. This container posture could not mount a writable cgroup. Success does not require it when ECHILD has settled.
- `ci/deny.toml` license allow-list drift. Two xtask tests still fail. Out of scope. Assertions unchanged.
