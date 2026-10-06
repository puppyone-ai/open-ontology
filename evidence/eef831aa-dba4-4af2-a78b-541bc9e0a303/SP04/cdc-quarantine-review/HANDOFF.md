# Handoff

## Owned path

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a303/SP04/cdc-quarantine-review/**`

Fixed archive `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`. Contract `cdc-quarantine-v1` in `tools/cdc-quarantine/CONTRACT.md`. Product source was not modified.

## What the next reader can rely on

`linux/preoverlay` passed the author suite: 1 library test, 0 bin tests, 14 `tests/proof.rs` tests, total passed 15, plus clippy `-D warnings`, on Linux arm64 Rust 1.94.0, 6 GiB, 1 CPU. Raw exits are in that directory. The first offline fetch on the empty volume exited 101. The online fetch exited 0.

`linux/overlay-4` passed `cargo test --locked --offline --test review_proof` (8 passed) and `cargo clippy --locked --offline --all-targets -- -D warnings`. `linux/overlay`, `linux/overlay-2`, and `linux/overlay-3` are the failed review-test attempts and stay as they finished, including `review_proof.failed.rs` in each.

`durable_quarantined` enables `local_prerequisite` only. ACK flags stay false. A paused collection is persisted in the control manifest and recovered as a sorted union. The crate stores the control event. It does not apply a target schema and does not issue a target TRUNCATE.

The child registry lock is an unchanged 11-package subset of the root lock. Checksums are in `linux/overlay-4/lock-subset.txt` and `linux/preoverlay/lock-subset.txt`.

## What is still open

A fenced writer and a control offset are not integrated. Upstream ACK remains false. The parent ledger and the acceptance denominator were not updated. This handoff does not accept CDC-D03 for the parent count and does not claim a release.

The SIGKILL evidence is a real child signal and a name-level recovery check. It is not a proof of torn file bytes.

`ControlEvent` and `Rejected` are not re-exported. External callers use the parsed value through `put`, `replay`, and `identity_preimage`, and they read `Status`.

## Inventory gap

`linux/manifest/inventory.sha256` is the first `MANIFEST.sha256`. `source/product-pass1.sha256` is the first product hash. The close pass rewrites `MANIFEST.sha256` and `source/product.sha256`. After that hash, the close pass writes `linux/manifest-close/docker-run.exit`, `hostconfig.txt`, `console.log`, and `container-removed.txt`. `cleanup.txt` is written later. Those five files are outside the final inventory. The hasher also skips the output file it is writing.

## Cleanup

`cleanup.txt` records removal of this task's containers (`oo-a303-preoverlay`, `oo-a303-overlay`, `oo-a303-overlay2`, `oo-a303-overlay3`, `oo-a303-overlay4`, `oo-a303-manifest`, `oo-a303-manifest-close`), volumes `oo-a303-eef831aa-cargo` and `oo-a303-eef831aa-target`, network `oo-a303-eef831aa-net`, and `/tmp/oo-a303-eef831aa-dba4-4af2-a78b-541bc9e0a303`. Pinned images stay. Foreign containers stay. No prune.
