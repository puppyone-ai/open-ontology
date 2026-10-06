# Handoff

Verdict: limited review of the repaired normalized assembler at fixed archive `cc98bab7eba2c58c127435db4d2ae777797259de`. Reviewed repaired `assemble.rs` SHA-256 `9b9940554767b88b7d5eef81047cf7590c884e8094a498416a27cfa097621840`. CDC-D01 is unaccepted. SP04 is unaccepted. The parent goal is unaccepted. A release is unaccepted.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a298`. Repair commit `6f8385e87ae350d1ce4bf31fca8dc8b423aca29d`, tree `832024e6e0a5052db72be37e7b85bd10468ad12b`. Assembler commit `2bdb3faa6329caa1503dceb52686c18c0be007dd`, tree `8368b5947f4119f258f0dab00ceae3eba1bcea04`. Fixed archive tree `a9369aab2c53afdf94616abee66b5099adc46d3e`. Repo HEAD at the proof launch was `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`. The proof used `git archive` of the fixed commit, copied with `cp -R`. No product file was edited.

a284 session `eef831aa-dba4-4af2-a78b-541bc9e0a284` remains **WITHHELD**. Its files were not edited. The author sentence "A `u16` ordinal cannot exceed 32767, which is the contract maximum." stays in that REPORT and stays false. A `u16` holds `0..=65535`. a284 `linux/proof-3` remains 14 passed and 1 failed, test `ordinals_32768_and_65535_are_outside_the_contract`. a290 `linux/preedit` remains 4 passed and 6 failed. a290's green self-review is not this independent result.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/tools/cdc-transaction/` for the fixed archive bytes
- `source/numstat.txt` and `source/blob-identity.txt` for the 29-line `assemble.rs` change and the new author test
- `overlay/a298_ordinal.rs`, SHA-256 `719dfc83e2c0c4ca35a05954e9b79a71449e936267ba99ade706f8d7d6bee0bc`
- `linux/proof/` for the author 37, Clippy, the unchanged a284 15, and the fresh 9
- `oracle/registry-triples.txt`, `tools/lock_subset.rs`, `tools/sha256_manifest.rs`, `tools/ORIGIN.txt`
- `recipe-origin.txt`, `CLEANUP.txt`, `MANIFEST.sha256`

## Product bytes under review

The check is `ordinal_above_contract` in `reject_bad_data`, constant 32767, fault `FaultCode::OrdinalRange`, before `validate_key`, `validate_payload`, replay, `charge_data`, and `store_data`. A refusal stores `self.fault`. The same assembler keeps returning that fault. A valid retry is a new assembler. The fresh overlay pins that, and pins hash `bcce7305687a58a07e5ea9416487eb2c15b12d2658dffd7dcd81546c33059440` for txn `r298`, text `pin`.

Repaired `assemble.rs` SHA-256 `9b9940554767b88b7d5eef81047cf7590c884e8094a498416a27cfa097621840`, git blob `f0b493a6ffdca317de753d1a45ca89a93498888e`. Author test SHA-256 `d3503329dbff0802e4739557761c3c0c62cb1b85a7e2134c1c02deba47c42b9f`, git blob `3dd2e0244c87b5c709c2d6c9dfd8c4b3016983c2`. Parent `assemble.rs` blob `f47af50c41320b5a7aa9ee7e9d5ddedb60b37952`. Child lock SHA-256 `4f2abdc20f88225bba1b91c7b7dbc1af26102e08144e45dc6f16e04664b1f26a`. Root lock SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`, git blob `2990f901ea98a7b465f462cce8742bc5c51c13aa`.

a207 remains the sole writer of the connector manifest and the root lock. Do not run `cargo update` or `cargo generate-lockfile` in a rerun of this review. Do not copy the child lock back onto the product tree.

## Decisive exits

`linux/proof` container `2b412e395cb55ac6f0fb9095a1c74731f0aa46d266dc98c6e210750350b0b153`, removed as `oo-a298-proof`. `docker-run.exit` 0. `version.exit` 0. `sealed.exit` 0. `fetch-offline.exit` 101, then `fetch.exit` 0. `smoke-test.exit` 0, `smoke-count.txt` 37, `smoke-names.exit` 0, handcomputed 19 finished in 23.68s, author ordinal file 10 finished in 0.01s. `clippy-before.exit` 0 in 1.62s. `a284.exit` 0, `a284-count.txt` 15, finished in 25.60s, including `ordinals_32768_and_65535_are_outside_the_contract`, the 200000-event cap, and the value-byte cap. `a298.exit` 0, `a298-count.txt` 9, finished in 0.03s. `clippy.exit` 0 in 0.20s. `product-scope.exit` 0. `lock-unchanged.exit` 0. `lock-subset.exit` 0. `lock-compare.exit` 0. Summary `registry_triples=11 missing=0 mismatch=0 path_packages=1 direct_pins_present=true`. `console.log` ends `worst_phase_exit=0`.

Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a298-eef831aa-net`, ReadonlyRootfs false. `capacity.txt` recorded `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0`. PostgreSQL was not started. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One proof builder. `python-present.flag` is 1. Python was not invoked. `cargo-update-invoked.txt` is `no`.

## Re-run

Pinned image above. `RUSTUP_TOOLCHAIN=1.94.0`. `HOME=/tmp`. One builder, 6 GiB, 1 CPU, `--cap-drop ALL`, private bridge, `cp -R`, `--locked --offline` after the locked fetch. `git archive cc98bab7eba2c58c127435db4d2ae777797259de`. The archive already contains the repair. Do not overlay product files onto it.

```text
cargo test --locked --offline -- --test-threads=1
cargo clippy --locked --offline --all-targets -- -D warnings
```

Run those before copying review tests. The author count on this archive is 37. Then copy `a284_independent.rs` only when its SHA-256 is `19c1faeaa5c7503e09e5f522f757a43a8f2602e57dd3644b79f1409b7bc24a20`, and expect 15 passed. Then copy `a298_ordinal.rs` only when its SHA-256 is `719dfc83e2c0c4ca35a05954e9b79a71449e936267ba99ade706f8d7d6bee0bc`, and expect 9 passed. Clippy again after the copies. Changing either overlay so a completed 65535 counts as success would hide a regression.

`linux/proof/console.log` is the single proof launch. Leave it in place.

The evidence inventory is `MANIFEST.sha256`, written by `linux/manifest/launch.sh`. The first container is `oo-a298-manifest`. The refresh container is `oo-a298-manifest-refresh`. The inventory rewrite, after `CLEANUP.txt` records the manifest-outside removal, is `oo-a298-manifest-inventory`. That last container's hostconfig is outside this directory.

Flags that read false in the overlay are an observation of the assembler output. They are not a Debezium, target-applied, ACK, epoch, router, cross-table, durability, or full-parent proof.
