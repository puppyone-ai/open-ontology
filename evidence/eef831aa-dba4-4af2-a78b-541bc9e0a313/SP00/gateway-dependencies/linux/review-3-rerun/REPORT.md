# Review 3. Fresh-target re-run of the committed probe

Reviewed commit `773c461104729d6c289eae8640c7c2a28207c447`, archived with `git archive` and mounted read-only at `/src`. The working tree was not the compile input.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. rustc `1.94.0 (4a4ef493e 2026-03-02)`. `RUSTUP_TOOLCHAIN=1.94.0`. Container `83cb5ecac838c956bc82054f1219002d8804e3720eb7e693707e9b4eb6d51897`. `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`, network `oo-a313-eef831aa-net`. Target volume `oo-a313-eef831aa-review-target` was new. Cargo home reused `oo-a313-eef831aa-cargo` so the run could stay `--locked --offline`. `inside_ok=2026-10-07T10:52:12Z`.

| Phase | Exit |
|---|---|
| pin | 0 |
| lock SHA-256 | 0 |
| lockdiff build, selfcheck, subset | 0 |
| test | 0 |
| clippy `-D warnings` | 0 |
| docker-run | 0 |

`test.stdout`: 4 passed, 0 failed, 0.37s. The four tests are the body cap, the echo that ignores `Authorization` and the query string, the shared permit, and the 408 deadline. Clippy finished the probe in 8.43s. The container was removed. `linux/review-rerun/container-removed.txt` names `oo-a313-review`.

This re-run does not set the baseline to approved and does not link `oo-gateway`.
