# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a315`. Implementation `ceb4be5ebe12cf581f511cc8a676050d02465c55`. Contract `c548c8b18c4de5d75e6f8527c71569a2c311eb11`, ancestor of the implementation, changes only `tools/description-pack/CONTRACT.md`. Fixed archive `99ac19af42059a308ba9c418815619e5a081f407`, commit tree `6c4f5e42462a2b7b4cd270dd3216d7bfce880740`. `tools/description-pack` tree `2b747801bdfa0de16f67c193c779f3427660198f` is the same at the implementation and at the fixed commit. Repo HEAD recorded at the proof-3 launch is `080c07ac71f38bb83def05bd211769a454e22f1b`.

This directory is the independent review of the occupied-review amendment. The author session `a309` is not this review. The parent ledger was not edited. This file does not accept S5-D05 for the parent, does not accept publication, does not close SP07 or SP08, does not raise a denominator, and does not certify a release.

The approval covers the tool behavior exercised by the raw exits below. An explicit flag plus the exact canonical prior-review hash reseals approved label, description, and grain through the compiler, reviewed-effective, and two role packs. Existing compiler review objects stay. Reviewer provenance is bridge output. Flag absent keeps `review.dataset_decision_occupied`. Wrong, missing, or extra CAS refuses `review.occupied_cas`. A hash without the flag refuses `schema.invalid`. Dataset accept is not rewritten. A stale override fails closed. Reject and defer do not reseal. One mixed fresh-and-occupied approval appends exactly one override review and keeps every prior review object.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/` and `source/fixed-commit.txt`
- `overlay/a315_sealed.rs` and the mounted a302 file recorded by `linux/proof-3/a302-overlay.sha256`
- `tools/lock_subset.rs` and `linux/proof-3/lock-subset.stdout`
- `linux/proof-1/` and `linux/proof-2/` for the kept failures
- `linux/proof-3/` for the 14-test smoke, both Clippy runs, the a302 seven, and the a315 five
- `CLEANUP.txt`, `MANIFEST.sha256`, `tools/sha256_manifest.rs`, `origin/recipes.txt`

## Decisive exits

`linux/proof-3` container `d158fb4e4010e4896f57a3b82ea212cdecd45ff7805f7e084c2755816d537d12`. `docker-run.exit` 0. `smoke-test.exit` 0, `smoke-count.txt` 14, `clippy-pre.exit` 0, `a302-test.exit` 0 with 7 passed, `a315-test.exit` 0 with 5 passed, `clippy.exit` 0, `lock-subset.exit` 0, `lock-summary.exit` 0, `product-unchanged.exit` 0. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a315-eef831aa-net`.

`linux/proof-1/fetch-offline.exit` is 101 and `fetch.exit` is 0. That was the cold volume. Proof-3 reused it, so its offline fetch is 0.

`linux/proof-1/capacity.txt` and `linux/proof-3/capacity.txt` each recorded `own=0 global=0 host_cargo=0` before start. PostgreSQL was not started.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0`. `HOME=/tmp`. One builder, 6 GiB, 1 CPU, `--cap-drop ALL`, `--pull never`. `git archive 99ac19af42059a308ba9c418815619e5a081f407` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `tools/description-review`, and `tools/description-pack`. Mount the archive read-only. Move `rust-toolchain.toml` aside. Keep both committed locks. `cargo fetch --locked`, then the author `cargo test --test pack` and Clippy, then copy the unchanged `a302_sealed.rs` and `overlay/a315_sealed.rs` and run those two test binaries. Do not run `cargo update` or `cargo generate-lockfile`. Copy the child lock into the evidence directory only. Do not start PostgreSQL. Do not replace the crate from a worktree.

A later run needs a new evidence directory and a new container name. `CLEANUP.txt` records removal of `oo-a315-eef831aa-cargo`, `oo-a315-eef831aa-target`, and `oo-a315-eef831aa-net`.
