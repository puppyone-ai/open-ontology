# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a283`. Reviewed commit and fixed archive are `ec8b3953cdcb63035d64fbeca3310214aab002ef`, tree `00560beb14aba36ba62d944dff409c9fcd75af4d`. Repo HEAD recorded at the proof launch is `c551afea55d5a9164760878a5d6a76835b869875`.

This directory is the independent review of the business-calendar helper and CLI. The author session `a269` `linux/review-1` is not this review. The parent ledger was not edited. This file does not accept SEM-D01 for the parent, does not accept an `oo_dict` calendar view, does not accept authenticated finance close, does not close SP08, and does not certify a release.

The approval covers the helper behavior exercised by the raw exits below. Anchors are the configured core set. Quarter boundaries are `business_fiscal_quarter`. `week_start` is metadata. `finance_approval`, `authenticated`, `database_used`, and `clock_used` stay false. A caller closure hash is not a ledger proof. Missing closure does not fall back to today. Zone gaps and overlaps are not shifted.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/` and `source/commit.txt`
- `overlay/a283_handcomputed.rs`
- `oracle/registry-triples.txt` and `tools/lock_subset.rs`
- `linux/proof/` for the missing `calendar.yaml` compile failure
- `linux/proof-2/` for the overlay indent failure, and `linux/proof-2/failed-a283_handcomputed.rs`
- `linux/proof-3/` for the 21-test smoke, the 9-test overlay, Clippy, and the 115-triple subset
- `CLEANUP.txt`, `MANIFEST.sha256`, `tools/sha256_manifest.rs`, `recipe-origin.txt`

## Decisive exits

`linux/proof-3` container `ab6307b5f59e9d6943c82937bb508d0e65310fd49ef130ada251d7c9fae93641`. `docker-run.exit` 0. `smoke-test.exit` 0, `smoke-count.txt` 21, `test.exit` 0 with 9 passed and 0 failed, `clippy.exit` 0, `lock-subset.exit` 0, `lock-compare.exit` 0, `product-unchanged.exit` 0. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a283-eef831aa-net`.

`linux/proof` container `c1cec285c0fcdadb29f91ef2ce7b223a76270a4328031d6e119805b824bdf1b0`. `docker-run.exit` 101. `smoke-test.exit` 101, `smoke-count.txt` 0, `lock-subset.exit` 0, `fetch-offline.exit` 101, `fetch.exit` 0. Clippy `not_run`.

`linux/proof-2` container `c488c0602694a27b1549145dd8372d5f1831897ed4fe3850f0b7bca6e9957c8a`. `docker-run.exit` 101. `smoke-test.exit` 0, `smoke-count.txt` 21, `fetch-offline.exit` 0, `test.exit` 101, `clippy.exit` 101, `product-unchanged.exit` 0.

Each `capacity.txt` recorded `own=0 global=0 host_cargo=0` before start. PostgreSQL was not started.

## Preserved attempts

`linux/proof` did not compile the author suite because `examples/schema/calendar.yaml` was outside the archive. That is a harness miss. The crate was not edited.

`linux/proof-2` compiled the author suite. The overlay YAML lost the indent of its first phrase through a Rust string continuation, and Clippy rejected a needless lifetime in the overlay. `linux/proof-2/test.stdout` names `dsl_supported_or_refused_and_week_is_not_an_anchor`. Do not reread that 101 as an author-suite failure. The corrected overlay is `linux/proof-3`.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0`. `HOME=/tmp`. One builder, 6 GiB, 1 CPU. `git archive ec8b3953cdcb63035d64fbeca3310214aab002ef` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `tools/business-calendar`, and `examples/schema/calendar.yaml`. Copy with `cp -R` onto a writable work directory, including that example file. Mount the archive read-only. Keep the crate `Cargo.lock`. Do not copy that lock back onto the product tree.

```text
cargo test --manifest-path tools/business-calendar/Cargo.toml --locked --offline -- --test-threads=1
cargo test --manifest-path tools/business-calendar/Cargo.toml --locked --offline --test a283_handcomputed -- --test-threads=1
cargo clippy --manifest-path tools/business-calendar/Cargo.toml --locked --offline --all-targets -- -D warnings
```

The manifest pair is `linux/manifest/launch.sh`. It hashes this directory after the reports. Its refresh stdout stays outside the review directory.
