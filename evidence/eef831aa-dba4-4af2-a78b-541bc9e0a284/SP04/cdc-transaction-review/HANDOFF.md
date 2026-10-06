# Handoff

Verdict: **WITHHELD**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a284`. Reviewed assembler commit `2bdb3faa6329caa1503dceb52686c18c0be007dd`, tree `8368b5947f4119f258f0dab00ceae3eba1bcea04`. Fixed archive `c551afea55d5a9164760878a5d6a76835b869875`, tree `e03c77fde3be687ace03984569e128407618017a`. Repo HEAD recorded at the `linux/proof-3` launch is `d738a621c296e97458e2b9dd30947dd03bc1689b`.

This directory is the independent review of the CDC transaction assembler and JSONL CLI. The author session `eef831aa-dba4-4af2-a78b-541bc9e0a280` and its `linux/review-1` are not this review. The parent ledger was not edited. This file leaves CDC-D01 open, leaves SP04 open, and leaves Debezium, ACK, P2, P11, a durable spool, and a release unaccepted.

The decisive defect is the ordinal clause. Contract ordinals are `0..=32767`. The CLI completed 32768 and 65535 with exit 0, `fault` null, and one complete. `u16` accepts both. `FaultCode::OrdinalRange` is never constructed. 65536 is `malformed`. The author sentence that a `u16` cannot exceed 32767 is false.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/` and `source/fixed-commit.txt`, `source/assembler-commit.txt`
- `overlay/a284_independent.rs`
- `oracle/registry-triples.txt` and `tools/lock_subset.rs`
- `linux/proof/` for the first ordinal completion and the `needless_lifetimes` overlay lint
- `linux/proof-2/` for both ordinal completions and the unused `stderr` field lint
- `linux/proof/failed-a284_independent.rs` for the overlay that `linux/proof` ran
- `linux/proof-3/` for the 27-test smoke, the 3 position tests, Clippy exit 0, and the same ordinal failure
- `CLEANUP.txt`, `MANIFEST.sha256`, `tools/sha256_manifest.rs`, `recipe-origin.txt`

## Decisive exits

`linux/proof-3` container `b8669d262ef8802da4783534cef1491d15cc0402dd9823f130520b02f655a3cb`, removed as `oo-a284-proof3`. `docker-run.exit` 101. `smoke-test.exit` 0, `smoke-count.txt` 27, `position.exit` 0, `position-count.txt` 3, `test.exit` 101 with 14 passed and 1 failed, `clippy.exit` 0, `lock-subset.exit` 0, `lock-compare.exit` 0, `product-unchanged.exit` 0. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a284-eef831aa-net`.

`linux/proof` container `b2098daa4932f44013a7793ef3279b611ba0e05fd67d554f7daecec5ef9334ae`. `docker-run.exit` 101. Smoke 27, position 3, overlay 101, clippy 101.

`linux/proof-2` container `c87ae9fa595cd109905322c1e7d3001b8fb5120ca58b32074fdc07dc17d82a38`. `docker-run.exit` 101. Smoke 27, position 3, overlay 101, clippy 101.

`linux/proof/capacity.txt` recorded `own=0 global=1 host_cargo=0`. `linux/proof-2` and `linux/proof-3` recorded `own=0 global=0 host_cargo=0`. PostgreSQL was not started. One builder ran at a time.

## Preserved attempts

Do not reread `docker-run.exit` 101 as an author-suite failure. The author commands in each container exited 0 before the overlay was copied. `linux/proof` shows ordinal 32768 completing. `linux/proof-2` and `linux/proof-3` show ordinals 32768 and 65535 completing. The Clippy failures in `linux/proof` and `linux/proof-2` are on the review overlay. `linux/proof-3` Clippy exited 0 with the contract assertion still in force.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0`. `HOME=/tmp`. One builder, 6 GiB, 1 CPU. `git archive c551afea55d5a9164760878a5d6a76835b869875`. Copy with `cp -R` onto a writable work directory. Mount the archive read-only. Keep the crate `Cargo.lock`. Do not copy that lock back onto the product tree. Move `rust-toolchain.toml` aside; it pins 1.98.1 and this image is 1.94.0. Set `CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`, and `CARGO_PROFILE_TEST_DEBUG=0`.

Run the author suite and the three position tests before copying `overlay/a284_independent.rs`. Then:

```text
cargo test --locked --offline --test a284_independent -- --test-threads=1
cargo clippy --locked --offline --all-targets -- -D warnings
```

A green ordinal test requires the product to refuse 32768 and 65535 with `ordinal_range`. Changing the test so that a completed 65535 counts as success would hide the defect. a207 remains the sole writer of the connector manifest and the root lock. This review does not edit either.
