# Handoff

Verdict: repair recorded. CDC-D01 is unaccepted. Independent review comes later.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a290`. Assembler commit `2bdb3faa6329caa1503dceb52686c18c0be007dd`, tree `8368b5947f4119f258f0dab00ceae3eba1bcea04`. Fixed archive `87c927b218cb2b6ca9b1759854a4c0fd778e8ffc`, tree `37c128eebd35349061cc01e8f7adcf4ba1b6b1b8`. Repo HEAD at the pre-edit and green launches was `c8588faab992af1193ff58dbc489a0b117796aa0`.

a284 session `eef831aa-dba4-4af2-a78b-541bc9e0a284` remains **WITHHELD**. Its files were not edited. The author's `u16` sentence was not edited. This file leaves CDC-D01 open, leaves SP04 open, and leaves Debezium, ACK, P2, P11, a durable spool, and a release unaccepted. No parent ledger was edited.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/preedit/` for the sealed archive, and `source/repair/` for the two files the green container compiled
- `overlay/ordinal_range.rs`, the byte source of `tools/cdc-transaction/tests/ordinal_range.rs`
- `linux/preedit/` for the author 27 and the red 4 passed / 6 failed
- `linux/repair/` for the 37 passed, Clippy exit 0, and the a284 overlay 15 passed
- `oracle/registry-triples.txt`, `tools/lock_subset.rs`, `tools/sha256_manifest.rs`
- `recipe-origin.txt`, `CLEANUP.txt`, `MANIFEST.sha256`

## Product files

Owned product paths are `tools/cdc-transaction/src/assemble.rs` and `tools/cdc-transaction/tests/ordinal_range.rs`. The check is `ordinal_above_contract` in `reject_bad_data`. The constant is 32767. The fault is the existing `FaultCode::OrdinalRange`. `model.rs`, `jsonline.rs`, `main.rs`, `CONTRACT.md`, `Cargo.toml`, and `Cargo.lock` are unchanged. Existing tests are unchanged.

Repaired `assemble.rs` SHA-256 `9b9940554767b88b7d5eef81047cf7590c884e8094a498416a27cfa097621840`, git blob `f0b493a6ffdca317de753d1a45ca89a93498888e`. Test SHA-256 `d3503329dbff0802e4739557761c3c0c62cb1b85a7e2134c1c02deba47c42b9f`, git blob `3dd2e0244c87b5c709c2d6c9dfd8c4b3016983c2`. Sealed `assemble.rs` SHA-256 `ac92b4300e76a33e8a1b9185269d4750b6355c9955136f7765b95c4715aafe8a`. Child lock SHA-256 `4f2abdc20f88225bba1b91c7b7dbc1af26102e08144e45dc6f16e04664b1f26a`.

A refusal stores `self.fault`. The same assembler keeps returning `OrdinalRange`. A valid retry is a new assembler. The tests pin that. Do not add an in-place clear of the fault in a later patch unless a new contract says so.

## Decisive exits

`linux/preedit` container `f432988d57dda5b8137b0a8323ba0d78294ed45f9472066a8433b39fc0f4ce91`, removed as `oo-a290-preedit`. `docker-run.exit` 101. `smoke-test.exit` 0, count 27. `test.exit` 101, 4 passed, 6 failed. `product-unchanged.exit` 0. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a290-eef831aa-net`.

`linux/repair` container `165917dd43d2cc8886c96df6ee4746c3c70895fc423bce7c73a6cacbb5a30635`, removed as `oo-a290-repair`. `docker-run.exit` 0. `test.exit` 0, `test-count.txt` 37, `test-names.exit` 0. `clippy.exit` 0. `a284.exit` 0, `a284-count.txt` 15, including `ordinals_32768_and_65535_are_outside_the_contract`. `product-scope.exit` 0. `lock-unchanged.exit` 0. `lock-compare.exit` 0. Same memory, CPU, cap-drop, and network. PostgreSQL was not started. One builder ran at a time.

`linux/preedit/capacity.txt` recorded `own=0 global=0 host_cargo=0`. `linux/repair/capacity.txt` recorded `own=0 global=1 host_cargo=0`. The global builder was another owner. The own cap stayed 0 before launch.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0`. `HOME=/tmp`. One builder, 6 GiB, 1 CPU. `git archive 87c927b218cb2b6ca9b1759854a4c0fd778e8ffc`. Copy with `cp -R`. Mount the archive read-only. Copy the two repair files onto the work tree. Keep the crate `Cargo.lock`. Do not run `cargo update` or `cargo generate-lockfile`. Do not copy the child lock back onto the product tree.

```text
cargo test --locked --offline -- --test-threads=1
cargo clippy --locked --offline --all-targets -- -D warnings
```

The author suite is 27 and the new file is 10. A green sum is 37. The a284 file `a284_independent.rs` SHA-256 `19c1faeaa5c7503e09e5f522f757a43a8f2602e57dd3644b79f1409b7bc24a20` is a boundary test, copied after those two commands. It is not a product file. Changing it so a completed 65535 counts as success would hide a regression.

a207 remains the sole writer of the connector manifest and the root lock.

The evidence inventory is `MANIFEST.sha256`, written by `linux/manifest/launch.sh`. The first container is `oo-a290-manifest`. The refresh container is `oo-a290-manifest-refresh`. The inventory rewrite, after `CLEANUP.txt` records the manifest-outside removal, is `oo-a290-manifest-inventory`. That last container's hostconfig is outside this directory.
