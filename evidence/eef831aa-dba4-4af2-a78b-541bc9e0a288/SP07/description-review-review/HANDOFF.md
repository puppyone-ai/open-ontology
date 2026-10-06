# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a288`. Implementation `8f4e792c4eca6eabb204de4c8bc09caa854165ec`. Fixed archive `216e6df82a61d2bc22548ce1a5075d5f4a045648`, commit tree `b7d5faef17d3e8f0170047cc7f82a996f46538ff`. `tools/description-review` tree `16a7de40a57a7db335b5a0a005b8239360aa678d` is the same at both commits. Repo HEAD recorded at the proof-4 launch is `2a6b6f0acd51c060577e1f336e0a686d54df2001`.

This directory is the independent review of the description-review command. The author session `a277` is not this review. The parent ledger was not edited. This file does not accept S5-D04 for the parent, does not accept publication, does not accept reviewer authentication, does not close SP07, and does not certify a release.

The approval covers the tool behavior exercised by the raw exits below. Approval can replace `label`, `description`, or `grain` on an existing core `DatasetFile` when the current field hash matches. Reject, defer, and a missing decision leave the current curated document. Stale current schema, current evidence, request, card, curated, and candidate anchors do not write the stale dataset. Unknown `classification` and `status` keys are refused and are not echoed in the review error or the review document. Exact bounds are written whole. A longer label and a forbidden control are not truncated into the document. Other columns, the curated caveat, and the PII fields on those columns stay. Compiler statements keep `curated=not_written`. The review statements are a separate object and record `database=not_used`, `authentication=not_claimed`, `files=not_written`, and `publication=unavailable`.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/` and `source/fixed-commit.txt`
- `overlay/a288_sealed.rs`
- `tools/lock_subset.rs` and `linux/proof-4/lock-subset.stdout`
- `linux/proof/` for the pipefail version-check failure
- `linux/proof-2/` for the `E0106` overlay compile failure
- `linux/proof-3/` for the overlay `dead_code` Clippy failure after 6 passing tests
- `linux/proof-4/` for the 16-test smoke, the 6-test overlay, Clippy, and the 117-triple subset
- `CLEANUP.txt`, `MANIFEST.sha256`, `tools/sha256_manifest.rs`, `origin/recipes.txt`

## Decisive exits

`linux/proof-4` container `ac9c102020e928e021eec8e1bed072dcb8fb30dec2c7b2f6761ba1ec3bfdd006`. `docker-run.exit` 0. `smoke-test.exit` 0, `smoke-count.txt` 16, `test.exit` 0 with 6 passed and 0 failed, `clippy.exit` 0, `lock-subset.exit` 0, `lock-summary.exit` 0, `fetch-offline.exit` 0, `fetch.exit` 0, `product-unchanged.exit` 0. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a288-eef831aa-net`.

`linux/proof` container `551d023b7fe8a290d866e1dd77d46c1870533daae4a4485f3f307f1ffd1bf887`. `docker-run.exit` 2. `version.exit` 2. Tests were not run.

`linux/proof-2` container `93f89ad00fe5951bb033e8da6c50488fff65e0a348cfc471cd5fcdf5f5627446`. `docker-run.exit` 101. `smoke-test.exit` 0, `smoke-count.txt` 16, `fetch-offline.exit` 101, `fetch.exit` 0, `test.exit` 101, `clippy.exit` 101, `product-unchanged.exit` 0.

`linux/proof-3` container `bce2af9f733ab3daf5427830d798543279ded8f92ba56ffdaa59311bd5aae4ff`. `docker-run.exit` 101. `smoke-test.exit` 0, `smoke-count.txt` 16, `fetch-offline.exit` 0, `test.exit` 0, `clippy.exit` 101, `product-unchanged.exit` 0.

`linux/proof-4/capacity.txt` recorded `own=0 global=1 host_cargo=0` before start. PostgreSQL was not started.

## Preserved attempts

`linux/proof` failed the toolchain check because `grep -q` under `pipefail` received `SIGPIPE` from `rustc -vV`. The release line was `1.94.0`. Do not reread that exit 2 as a product failure.

`linux/proof-2` passed the author suite and the locked fetch. The overlay helpers needed a named lifetime. Do not reread that 101 as an author-suite failure.

`linux/proof-3` passed the six overlay tests. Clippy rejected two unread overlay fields. The corrected overlay is the one `linux/proof-4` copied in after the author smoke.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0`. `HOME=/tmp`. One builder, 6 GiB, 1 CPU, `--cap-drop ALL`, `--pull never`. `git archive 216e6df82a61d2bc22548ce1a5075d5f4a045648` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, and `tools/description-review`. Copy with `cp -R` onto a writable work directory. Mount the archive read-only. Move `rust-toolchain.toml` aside. Keep both committed locks. `cargo fetch --locked`, then `cargo test` and `cargo clippy --locked --offline`. Do not run `cargo update` or `cargo generate-lockfile`. Copy the child lock into the evidence directory only. The overlay file is `overlay/a288_sealed.rs`, copied after the 16 author tests pass.
