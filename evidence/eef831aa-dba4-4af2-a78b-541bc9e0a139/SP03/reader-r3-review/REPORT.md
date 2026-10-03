# REPORT — a139 fixed reader R3 review

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a139`. Sole reviewer, no subagents and no worktree.

Fixed source `58afa97ded85a7dd89c72532bfc8973c45255d7c`, tree `543fd103532a8366ce92a0b4eaea4e089eb76d2d`, keys commit `8bda0cd48976b23d268d125b34bc8130d94b8253` contained in that commit. Prior evidence `85d34ae6fdd7407ccb0d9cc2863e155c44410c73` was read and not modified. Review used `git archive` of `58afa97`. Each `workspace-head.txt` records archive-time HEAD `02b9847159a68ced1443615cbb47bcb5872724b0`. The evidence commit sits on the workspace HEAD at commit time.

Verdicts are in `FINDINGS.md`. Keys: APPROVE. Cancel and unpublished-run: REQUEST_CHANGES. R4, the full reader, and the SP03 parent stay unsigned.

## Images and isolation

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `--pull never`. Container user `0:0`, `--cap-drop ALL`, no-new-privileges, memory 8 GiB, cpus 4, pids 4096, `HOME=/tmp`. Private network `oo-a139-eef831aa-net`. No host network, docker socket, or HOME mount. Cargo and target volumes `oo-a139-eef831aa-cargo` and `oo-a139-eef831aa-target`, labeled `oo.owner=a139`.

PostgreSQL image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `linux/arm64`, server `17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a139-pg`, no host port, memory 512 MiB, cap-drop ALL plus CHOWN, DAC_OVERRIDE, FOWNER, SETUID, SETGID. Database `oo_a139`. Roles `oo_a139_reader` and `oo_a139_writer`. `pg/up.exit=0`, `pg/init.exit=0`. Password files stayed under `/tmp/oo-a139-eef831aa-dba4-4af2-a78b-541bc9e0a139/secrets` mode 0700. `pg/pointer-keys.txt` lists pointer fields only.

One owned Postgres and one owned Rust builder at a time. Host `cargo` / `rustc` / `rustdoc` / `rustup` count was 0 at the author launch.

## Exits

| run | docker | lib | clippy | keys | cancel | review |
| --- | --- | --- | --- | --- | --- | --- |
| `linux/smoke` archive only | 0 | 0 (63) | 0 | | | |
| `linux/author` author tests once | 0 | | | 0 | 0 | |
| `linux/probe` first overlay | 101 | | | | | 101 compile `E0382` |
| `linux/probe-2` drop reproducer | 101 | | | | | 101 |
| `linux/probe-3` drop reproducer plus wait detail | 101 | | | | | 101 |

Smoke host config: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a139-eef831aa-net`, `Memory=8589934592`, `CapDrop=[ALL]`. Author and probe host configs match those isolation fields. Smoke elapsed 42 s inside the container. Author elapsed 31 s (keys 1.11 s, cancel 22.56 s). Probe-3 test runtime 6.47 s.

Archive identity for smoke, author, and probe: base `58afa97ded85a7dd89c72532bfc8973c45255d7c`, tree `543fd103532a8366ce92a0b4eaea4e089eb76d2d`. Author archive has no `pg_reader_r3_review.rs`. Probe archives add only that overlay file.

## Oracles

Author keys: `linux/author/keys-outcome.txt`.

Author cancel: `linux/author/cancel-outcome.txt`. Deadline 19783 ms against a 60 s statement timeout. `finish` after the flag is `cancelled`. Killed run SQLSTATE `57P01` does not finish. Fresh generation 9 is unmixed.

Probe-3 key phase: `linux/probe-3/review-outcome.txt`. Uuid-only primary key plans as one unkeyed chunk (`uuid_pk_only_unkeyed_resumable_false`). Drop phase panics in `linux/probe-3/console.log`.

Probe-3 reader SQL, counted from the postgres log after the containers were still up: `linux/probe-3/sql-audit-offline.txt`. 430 reader statements, zero DML, DDL, COMMIT, OFFSET, or COLLATE. One `SELECT pg_cancel_backend`. The in-test audit function did not reach its assertion.

## Attempts kept

`linux/probe/console.log` and `linux/probe-2/console.log` were not overwritten. The overlay fix for `E0382` is only in the later overlay source. a138 failed consoles that were overwritten before this review are still missing.

## Cleanup

`cleanup.txt` records removal of this handle's containers, volumes, network, and `/tmp/oo-a139-*` paths. No prune. No foreign containers.
