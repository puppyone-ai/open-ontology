# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a302`. Implementation `5ce8b1c4186eb80f9b876a306ae94309f10933ae`. Fixed archive `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`, commit tree `5903f9c8f39b9cb426df9c5f88b22af316c7826b`. `tools/description-pack` tree `93e4006b3249fe4849d62f29e53689acb6a2bb6a` is the same at both commits. Repo HEAD recorded at the proof-1 launch is `72dc9e2db841de4de368216bbbe13d738f850ea9`.

This directory is the independent review of the description-pack command. The author session `a292` is not this review. The parent ledger was not edited. This file does not accept S5-D05 for the parent, does not accept publication, does not close SP07 or SP08, and does not certify a release.

The approval covers the tool behavior exercised by the raw exits below. An applied approve copies label and description onto a new compiler seal and leaves an existing grain in place when overwrite is false. Reject, defer, and a provider failure leave the original seal and do not copy candidate prose into the pack. Wrong review hash, current schema, label CAS, and decision card hash refuse without a document. A description that names a hidden column is dropped for that role. The permitted role keeps the bounded UNREVIEWED fence. Explicit classification is a separate reviewed-effective value. Curated PII and draft overlay PII stay distinct, and draft effective PII stays null. Occupied dataset review is `unsupported` at seam `review.dataset_decision_occupied`.

The contract and the sources share commit `5ce8b1c4`. The author's record, kept by the plan ledger, is that the contract was written after the first source files. This review does not invent an earlier freeze.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/` and `source/fixed-commit.txt`
- `overlay/a302_sealed.rs`
- `tools/lock_subset.rs` and `linux/proof-1/lock-subset.stdout`
- `linux/proof-1/` for the 8-test smoke, the 7-test overlay, both Clippy runs, and the 117-triple subset
- `CLEANUP.txt`, `MANIFEST.sha256`, `tools/sha256_manifest.rs`, `origin/recipes.txt`

## Decisive exits

`linux/proof-1` container `a1485edc63b52902f6437dcc2788a82e7e77120c641b214349714a4df2c7548c`. `docker-run.exit` 0. `smoke-test.exit` 0, `smoke-count.txt` 8, `clippy-pre.exit` 0, `test.exit` 0 with 7 passed and 0 failed, `clippy.exit` 0, `lock-subset.exit` 0, `lock-summary.exit` 0, `fetch-offline.exit` 101, `fetch.exit` 0, `product-unchanged.exit` 0. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a302-eef831aa-net`.

`linux/proof-1/capacity.txt` recorded `own=0 global=1 host_cargo=0` before start. PostgreSQL was not started.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0`. `HOME=/tmp`. One builder, 6 GiB, 1 CPU, `--cap-drop ALL`, `--pull never`. `git archive 25dfc9022780c3deff12f37b7ad7ea4ed1fb6480` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `tools/description-review`, and `tools/description-pack`. Mount the archive read-only. Move `rust-toolchain.toml` aside. Keep both committed locks. `cargo fetch --locked`, then the author `cargo test --test pack` and Clippy, then copy `overlay/a302_sealed.rs` and run `cargo test --test a302_sealed`. Do not run `cargo update` or `cargo generate-lockfile`. Copy the child lock into the evidence directory only. Do not start PostgreSQL.

A later run needs a new evidence directory and a new container name. `CLEANUP.txt` records removal of `oo-a302-eef831aa-cargo`, `oo-a302-eef831aa-target`, and `oo-a302-eef831aa-net`.
