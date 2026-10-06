# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a295`. Source commit `524c6f5197a0e31e8d9e8f3e06510c7ee3d1d823`. Source tree `8b97319e7c47dc091187b3c5c103805b5e0d7080`. Crate tree `2a408d25eca5a076b1f6295b7ce40b8797f70526`. Fixed archive `c8588faab992af1193ff58dbc489a0b117796aa0`. Parent before the source commit `226eedeed6b7625669580116e3cab6ebd270738f`.

This directory is the development record and the four-pass review of `tools/session-release`. The parent ledger was not edited. This file does not accept RT-D04 for the parent, does not close SP11, and does not certify a physical release. `physical_release_certified` stays false. Containment is `local_fixture`. The successful fixture leaves the session `releasing`.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/commit.txt` and `source/session-release/`
- `linux/impl-5/` for the decisive test and Clippy exits
- `linux/impl/`, `linux/impl-2/`, `linux/impl-3/`, and `linux/impl-4/` for preserved failures
- `linux/precode/` for the fixed-archive smoke before this crate
- `pg/up.exit` and `CLEANUP.txt`
- `MANIFEST.sha256`

## Decisive exits

`linux/impl-5` container `c6b26f5abe2bc163fbec0ad7659f5a0a0e06ba3d56421ddf55b7fc5d32b368cd`. `docker-run.exit` 0, `test.exit` 0, `clippy.exit` 0, `signal-probe.exit` 0. Lib 9 passed. Live 9 passed in 15.00 s. Lock subset 1 passed. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a295-eef831aa-net`.

`linux/precode/docker-run.exit` 0. `pg/up.exit` 0. No host port.

## Re-run

Pinned Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Postgres image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `RUSTUP_TOOLCHAIN=1.94.0`. One builder, 6 GiB, 1 CPU. One private database. Archive `c8588faab992af1193ff58dbc489a0b117796aa0` for `Cargo.lock`, `tools/session-store`, and `tools/session-transition`, then overlay this crate. Keep the committed `tools/session-release/Cargo.lock`. Do not run `cargo generate-lockfile` or `cargo update`.

```text
cargo test --locked --offline --all-targets -- --test-threads=1
cargo clippy --locked --offline --all-targets -- -D warnings
```

Set `OO_ROOT_LOCK` to the archived root lock and `OO_A295_FIXTURE` to the fixture directory that holds `endpoint.txt`, `ca.crt`, and the password files. Passwords stay in those files and in stdin JSON. They are not process arguments.
