# Failed attempts

Each directory is the log from the run that failed. The replacement was a later phase on the same PostgreSQL and the same Rust builder. None of these directories is a current exit 0 phase.

| Directory | Exit | Cause |
|---|---|---|
| `linux/smoke-attempt-1/` | docker-run 1 | Lock check expected `package_names=1 source_lines=0`. Both child locks are `211/208`. |
| `proof/fetch-attempt-1/` | 101 | `oo-core` was mounted without the workspace root `Cargo.toml`. No test ran. PostgreSQL stayed up. |
| `proof/solo-attempt-1/` | 101 | `InstallDrift { check: "public_acl" }`. The owner row in `nspacl` was counted as an extra grant. |
| `proof/solo-reject-attempt-1/` | 101 | Sibling manifest `42501` became `Query`. The view read used `::regclass`. |
| `proof/solo-replay-attempt-1/` | 101 | Activated `oo_r_calendar` `USAGE` was counted as a foreign grant. |
| `proof/solo-replay-attempt-2/` | 101 | Reading a missing `runtime_allow` aborted the transaction (`25P02`). |
| `proof/cal-first-attempt-1/` | enum 101 | No `SELECT` on `install_manifest`, so the digest read was not adoptable. |
| `proof/cal-first-attempt-2/` | enum 101 | Non-owner `GRANT USAGE` returned success and granted nothing. `read` was `42501`. |
| `proof/cal-first-attempt-3/` | enum 101 | Identity arguments were `role_name text` while the lookup required `text`. |
| `proof/cal-first-attempt-4/` | enum 101 | Argument name `role_name` was ambiguous with `runtime_allow.role_name` (`42702`). |

`WHY.txt` in each directory is the short cause. The green `proof/*.log` files outside these directories are the runs after the corresponding repair. `resume-proof-6.sh` rebuilt only `oo_a365_cal_first` after attempt 4 and is the run whose `proof.exit` is 0.
