# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a307`. Fixed commit `72dc9e2db841de4de368216bbbe13d738f850ea9`. Crate tree `2a408d25eca5a076b1f6295b7ce40b8797f70526`. Store tree `f55ad766e225ae8f0d7a3913e10c681986efaca9`. Reducer tree `a25733870f1ffac55c057dd399270905ccca9151`. Author source commit with the same crate tree: `524c6f5197a0e31e8d9e8f3e06510c7ee3d1d823`. Repo HEAD at launch: `e8660132793b09ed1239a8c849f4f9498a6f17fc`.

This directory is the independent review of `tools/session-release`. The author directory is not this review. The parent ledger was not edited. This file does not accept RT-D04, does not close SP11, and does not certify a physical release. `physical_release_certified` stays false. Containment is `local_fixture`. The successful fixture leaves the session `releasing`.

## Read first

- `REPORT.md`, `FINDINGS.md`, and `FROZEN.md`
- `linux/preoverlay/` for the decisive exits, including `test-offline-attempt.exit` 101
- `pg/up.exit` and `CLEANUP.txt`
- `MANIFEST.sha256` is the inventory written after these files were stable. Container `oo-a307-manifest` (`e1f27d3f5ee78658328cc55fcefffbe68f09d3083f00ea9ca5c8d93ed9884ac4`, network `none`, exit 0) counted 71 files. `linux/manifest-refresh` then hashed its own console while that file was still empty. The final rewrite uses the same Rust tool with stdout outside this directory, so the console logs stay unchanged during the walk.
- Author context, not approval: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a295/SP11/session-release/`

## Decisive exits

`linux/preoverlay` container `39723a3ca4e11034c503bd995ef4702740f8734ae290113e423271f06b0cc60d`. `docker-run.exit` 0, `test.exit` 0, `clippy.exit` 0, `signal-probe.exit` 0, `lock-triples.exit` 0, `unchanged.exit` 0. Library 9 passed. Binary 0 tests. Live 9 passed in 15.16s. Lock subset 1 passed. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a307-eef831aa-net`.

`pg/up.exit` 0. Container `ee87546724a7af21ef108210878cbc32e2e98d92679988db4b295742344902b7`. No host port.

The first offline test exited 101 because the private cargo volume was empty. `cargo fetch --locked` exited 0 and the retry exited 0. Both attempts are kept.

## Re-run

Pinned Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Postgres image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `RUSTUP_TOOLCHAIN=1.94.0`. One builder, 6 GiB, 1 CPU. One private database. Archive `72dc9e2db841de4de368216bbbe13d738f850ea9`. Keep the committed locks. Do not run `cargo generate-lockfile` or `cargo update`.

```text
cargo test --locked --offline --all-targets -- --test-threads=1
cargo clippy --locked --offline --all-targets -- -D warnings
```

Set `OO_ROOT_LOCK` to the archived root lock and `OO_A295_FIXTURE` to a fixture directory that holds `endpoint.txt`, `ca.crt`, and the password files. Passwords stay in those files. They are not process arguments.
