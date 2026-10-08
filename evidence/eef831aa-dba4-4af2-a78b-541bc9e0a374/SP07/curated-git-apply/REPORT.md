# Curated Git apply

Isolated operator adapter. An approved description-review result, re-executed by the pinned compiler, plus an expected head and blob, becomes one curated commit on `refs/heads/curated` and one receipt. This slice reports `authenticated: false` and `parent_auth: required`. It does not authenticate, grant, or publish.

## Commits

- Contract: `9d33bfc065901db78e8f736e3a840b57630a31e4` `curated-git-apply: freeze the E57 operator commit contract`
- Implementation: `576e476f10eb6c9e3a06bdde7cc4f9e8b17bc9f6` `curated-git-apply: commit reviewed curated datasets by CAS`

## Pins

Reviewed commit `be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba`. Trees matched `HEAD` at the green run:

- `crates/oo-core` `769dd0e8f7ee77a29017fb3e0f9d90917b0a8194`
- `crates/oo-builder` `40f74d7e9aac9c13e95d1da3d61d689184773c77`
- `crates/oo-jobs` `ca228d7c648e554675cdf31ae828320e9d05cd9e`
- `tools/description-review` `a60b6d82e5b114430a66d82f91de4d3a7f3647c4`
- `tools/description-pack` `9d99fe90ecf87c09b4bb33e46e89bd078c10586b` (recorded, not linked)

## Builder

One container. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64. `RUSTUP_TOOLCHAIN=1.94.0` so the repo `rust-toolchain.toml` channel 1.98.1 was not installed.

- rustc 1.94.0 (4a4ef493e 2026-03-02)
- cargo 1.94.0 (85eff7c80 2026-01-15)
- git 2.47.3
- memory 6g, cpus 1, private `CARGO_HOME` and `CARGO_TARGET_DIR`
- Postgres was not started

Green run source `HEAD` before and after: `6ed4c9630c38cf0bd95b5879b3197b022997d867`. The test did not update a ref in the source checkout. Fixtures lived under the container temp dir.

## Result

`cargo test -- --test-threads=1 --nocapture` exit 0.

- library unit: 1 passed
- `tests/apply.rs`: 11 passed
- hostile-env re-exec of the accepted-review test: 1 passed

`cargo clippy --locked --all-targets -- -D warnings` exit 0.

Subcommand exits are in `raw/exits.txt`. They name the Git subcommand and the status only.

## What the adapter does

`apply_curated_git` re-runs `description_review::apply_description_review` on the original command bytes. It writes only compiler-approved dataset `label`, `description`, and `grain`, and v2 column `label` and `description`. Other semantic fields and unrelated paths stay byte-identical. The commit is built with a private `GIT_INDEX_FILE` (`read-tree`, `hash-object --no-filters`, `update-index --cacheinfo`, `write-tree`, `commit-tree`) and published with `update-ref --no-deref` old-SHA compare-and-swap. A mismatch leaves the ref in place. A lost reply is recovered by reading the ref and the commit message before another update. The service author is `oo-curated-git-apply <curated-git-apply@open-ontology.invalid>` at `1791417600 +0000`. The reviewer label stays declared provenance.

This is not parent SP02 or SP07 acceptance, and it is not goal acceptance.

## Hash supplement

The nine-file evidence commit had no `SHA256SUMS`. This supplement does not change product bytes and does not rerun the green Git tests. `linux/hash-oracle/main.rs` is the Safe Rust walker. `source/pin.txt` records the implementation commit, tool tree, and reviewed dependency trees. `source/SHA256SUMS` is that walker over the implementation tree of `tools/curated-git-apply`. `SHA256SUMS` is that walker over this evidence directory. `SHA256SUMS.sha256` is the sidecar. Raw hash exits are `linux/hash/`. Cleanup is `CLEANUP.txt`.
