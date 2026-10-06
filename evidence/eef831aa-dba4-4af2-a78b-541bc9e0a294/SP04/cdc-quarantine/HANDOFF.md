# Handoff

## Owned paths

- `tools/cdc-quarantine/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a294/SP04/cdc-quarantine/**`

Fixed archive `c8588faab992af1193ff58dbc489a0b117796aa0`. Contract `cdc-quarantine-v1` in `tools/cdc-quarantine/CONTRACT.md`.

## What the next reader can rely on

`linux/overlay-3` passed `cargo test --locked --offline --all-targets` and `cargo clippy --locked --offline --all-targets -- -D warnings` on Linux arm64 Rust 1.94.0, 6 GiB, 1 CPU. Raw exits are in that directory. `linux/overlay` and `linux/overlay-2` are the failed compiles and stay as they finished.

`durable_quarantined` enables `local_prerequisite` only. ACK flags stay false. A paused collection is persisted in the control manifest and recovered. The crate does not apply a target schema and does not issue a target TRUNCATE.

## What is still open

A fenced writer and a control offset are not integrated. Upstream ACK remains false. The parent ledger and the acceptance denominator were not updated. This handoff does not accept CDC-D03 for the parent count and does not claim a release.

The SIGKILL evidence is a real child signal and a name-level recovery check. It is not a proof of torn file bytes.

## Cleanup

`cleanup.txt` records removal of this task's cargo volume, target volume, network, and `/tmp/oo-a294-eef831aa-dba4-4af2-a78b-541bc9e0a294`. Pinned images were left in place. Foreign containers were left in place. No prune.
