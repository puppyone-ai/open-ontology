# CDC-D01 ordinal repair

Verdict: repair recorded. CDC-D01 stays unaccepted. An independent review is later. This session does not accept the parent goal, SP04, Debezium, ACK, P2, P11, a durable spool, or a release.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a290`. Assembler commit `2bdb3faa6329caa1503dceb52686c18c0be007dd`, tree `8368b5947f4119f258f0dab00ceae3eba1bcea04`. Fixed archive `87c927b218cb2b6ca9b1759854a4c0fd778e8ffc`, tree `37c128eebd35349061cc01e8f7adcf4ba1b6b1b8`. Both proofs built a `git archive` of that fixed commit and copied it with `cp -R`. Repo HEAD at both launches was `c8588faab992af1193ff58dbc489a0b117796aa0`.

a284 session `eef831aa-dba4-4af2-a78b-541bc9e0a284` stays **WITHHELD**. Its REPORT, FINDINGS, and HANDOFF were not edited. The author sentence that a `u16` ordinal cannot exceed 32767 stays historical and untouched. `u16` holds `0..=65535`. The contract range remains `0..=32767`.

## Product change

`tools/cdc-transaction/src/assemble.rs` gained the ordinal check inside `reject_bad_data`, after `validate_table` and before `validate_key` and `validate_payload`. `ordinal_above_contract` rejects a key cell, a payload cell, or an `unavailable_ordinals` entry above 32767 with the existing `FaultCode::OrdinalRange`. `fail` runs before replay, `charge_data`, and `store_data`, so the refusal does not write the event hash, the buffer, `seen.data`, or a completion. The open transaction, when one exists, is marked refused. `self.fault` stays set. A later line on that assembler returns the stored fault. No recovery path was added.

`tools/cdc-transaction/tests/ordinal_range.rs` is the new public-API and compiled-CLI test. No other product file, lock, manifest, model, existing test, spec, or ledger was edited.

Sealed `assemble.rs` SHA-256 `ac92b4300e76a33e8a1b9185269d4750b6355c9955136f7765b95c4715aafe8a`, git blob `f47af50c41320b5a7aa9ee7e9d5ddedb60b37952`. Repaired file SHA-256 `9b9940554767b88b7d5eef81047cf7590c884e8094a498416a27cfa097621840`, git blob `f0b493a6ffdca317de753d1a45ca89a93498888e`. New test SHA-256 `d3503329dbff0802e4739557761c3c0c62cb1b85a7e2134c1c02deba47c42b9f`, git blob `3dd2e0244c87b5c709c2d6c9dfd8c4b3016983c2`. Working tree diff against the fixed archive is 29 insertions in `assemble.rs` plus the new test. Child lock SHA-256 stayed `4f2abdc20f88225bba1b91c7b7dbc1af26102e08144e45dc6f16e04664b1f26a`.

## Pre-edit red

`linux/preedit`, container `oo-a290-preedit`, id `f432988d57dda5b8137b0a8323ba0d78294ed45f9472066a8433b39fc0f4ce91`, removed by its launch script. HostConfig: ExitCode 101, OOMKilled false, Privileged false, NetworkMode `oo-a290-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`, ReadonlyRootfs false. `capacity.txt` recorded `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. PostgreSQL was not started.

`docker-run.exit` 101 is the red test. `version.exit` 0, `sealed.exit` 0, `lock-subset.exit` 0, `lock-compare.exit` 0, `product-unchanged.exit` 0, `cargo-update-invoked.txt` `no`. `fetch-offline.exit` 101, then locked online `fetch.exit` 0. `smoke-test.exit` 0 and `smoke-count.txt` 27, before the overlay was copied: lib 5, bin 0, cli 3, handcomputed 19, doctests 0. Handcomputed finished in 23.29s. `python-present.flag` is 1. Python was not invoked.

`test.exit` 101. `red-summary.txt`: `FAILED. 4 passed; 6 failed; finished in 0.01s`. `red-marker.txt` is `expected_fail`. Passed on the unfixed assembler: `ordinal_32767_completes_for_each_in_range_shape`, `ordinal_65536_is_malformed_and_stores_nothing`, `cli_ordinal_32767_exits_zero`, `cli_ordinal_65536_is_malformed`. Failed because 32768 and 65535 completed: `ordinals_above_32767_refuse_without_storing`, `unselected_high_ordinal_refuses_and_in_range_is_omitted`, `cli_high_ordinals_exit_ordinal_range_with_no_complete`, `cli_unselected_and_shapes_match_the_bound`, `stored_event_survives_refusal_and_fresh_retry_keeps_the_hash`. `unavailable_list_above_32767_is_ordinal_range` returned `OrdinalMismatch` where the test requires `OrdinalRange`. The raw `test.stdout` is preserved.

## Green repair

`linux/repair`, container `oo-a290-repair`, id `165917dd43d2cc8886c96df6ee4746c3c70895fc423bce7c73a6cacbb5a30635`, removed by its launch script. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a290-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`, ReadonlyRootfs false. `capacity.txt` recorded `own=0 global=1 host_cargo=0 cap_global=10 cap_own=1 postgres=0`. Same image, toolchain, and archive pin. `cargo-update-invoked.txt` is `no`. Offline `fetch.exit` 0. `python-present.flag` is 1. Python was not invoked.

`cargo test --locked --offline -- --test-threads=1` exited 0. `test-count.txt` is 37. `test-names.exit` is 0. The result lines are lib 5 in 0.00s, bin 0, cli 3 in 0.01s, handcomputed 19 in 23.62s, `ordinal_range` 10 in 0.01s, doctests 0. The author 27 names and the 10 new names are in `test.stdout`.

`cargo clippy --locked --offline --all-targets -- -D warnings` exited 0. `clippy.stdout` is empty. Clippy stderr ends `Finished dev profile [unoptimized] target(s) in 1.71s`. `clippy-component.exit` is 0.

The a284 overlay was copied only inside the container, after Clippy, from `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a284/SP04/cdc-transaction-review/overlay/a284_independent.rs`. Pinned SHA-256 `19c1faeaa5c7503e09e5f522f757a43a8f2602e57dd3644b79f1409b7bc24a20` matched. `cargo test --locked --offline --test a284_independent -- --test-threads=1` exited 0. `a284-count.txt` is 15, finished in 25.87s. The line `test ordinals_32768_and_65535_are_outside_the_contract ... ok` is present. The other 14 a284 tests passed on the same run, including the 200000-event cap and the 33554432 value-byte plan. `product-scope.exit` is 0. `lock-unchanged.exit` is 0. The overlay file was not added to the product tree.

## Registry triples

`linux/repair/lock-subset.stdout`: `summary registry_triples=11 missing=0 mismatch=0 path_packages=1 direct_pins_present=true`. `lock-compare.exit` is 0 against `oracle/registry-triples.txt`. The only path package is `cdc-transaction 0.1.0`. Direct pins serde 1.0.229 checksum `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` and serde_json 1.0.151 checksum `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14` are in that list. Source for each registry package is `registry+https://github.com/rust-lang/crates.io-index`. Root `Cargo.lock` SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. The child lock was not copied back onto the product tree. a207 remains the sole writer of the connector manifest and the root lock.

## Archive bytes after the two repair files were copied

`linux/repair/source-sha256.txt`:

| file | sha256 |
|---|---|
| `CONTRACT.md` | `e6eb4d6e88c4c26b88dd10758d144eaf30d97cd4667e465e3687bc6625108bf7` |
| `Cargo.lock` | `4f2abdc20f88225bba1b91c7b7dbc1af26102e08144e45dc6f16e04664b1f26a` |
| `Cargo.toml` | `421944a6eeacaa00cf1720d01b76787f0e08e3cb488336b5aa313e84afb8ee1d` |
| `src/assemble.rs` | `9b9940554767b88b7d5eef81047cf7590c884e8094a498416a27cfa097621840` |
| `src/jsonline.rs` | `63477fa357fbac7b828b0c0d2b050892e9d0dbef4164dc40d0040e75c9a9fcd7` |
| `src/lib.rs` | `21051ceb58244b1c733df919fcb6b313a64ac5245477cd375ee4e2268580c1a9` |
| `src/lsn.rs` | `442a7b05f2799c2f432c2c83d693bb3956e8bfd9a2d9e4b46e7b2a792e7d14b8` |
| `src/main.rs` | `42f85ab77a6737837cbf687b9631a5b1493041881c6ab3b4f6414ddb3b58821c` |
| `src/model.rs` | `47907b5890987c34377bd52b2bf9d2ef1a24c58555dbb8f580b1857970927a98` |
| `src/sha256.rs` | `eac61aa8d9d0eebe935be0d438b6877b40845656601c4c5a8180edd97963f037` |
| `tests/cli.rs` | `2f4e942459ad5ba68f363183fda01c6151d1c6bdb1afc1a1b6b735814e8b30ea` |
| `tests/common/mod.rs` | `38c7290213f50e11dd008b1a2115734d9b6ae16af77f4d54fecf817bae3d3fc0` |
| `tests/handcomputed.rs` | `8c538af3afa7ca657fefabcefaae505e1c15c0a7a473ce7b8eb74c62c72b84ba` |
| `tests/ordinal_range.rs` | `d3503329dbff0802e4739557761c3c0c62cb1b85a7e2134c1c02deba47c42b9f` |

`source/preedit/` is the sealed archive copy. `source/repair/assemble.rs` and `source/repair/ordinal_range.rs` are the two files the green container compiled. `MANIFEST.sha256` is the full evidence inventory. `CLEANUP.txt` records the owned removals.
