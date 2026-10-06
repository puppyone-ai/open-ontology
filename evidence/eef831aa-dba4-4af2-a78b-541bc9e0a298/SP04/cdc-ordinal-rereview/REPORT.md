# CDC-D01 ordinal rereview

Verdict: **APPROVE_LIMITED** for the frozen CDC-D01 normalized assembler child, `tools/cdc-transaction`, at fixed archive `cc98bab7eba2c58c127435db4d2ae777797259de`. Reviewed repaired `assemble.rs` SHA-256 `9b9940554767b88b7d5eef81047cf7590c884e8094a498416a27cfa097621840`. No frozen requirement of that child remains unresolved in `linux/proof`. Parent SP04, Debezium, target applied, ACK, and a release stay unaccepted. This file does not edit the parent ledger and does not record coordinator acceptance.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a298`. This session edited no product file. The proof built a `git archive` of the fixed commit and copied it with `cp -R`. Repo HEAD recorded at launch in `linux/proof/repo-head.txt` is `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`. The archive is the fixed commit, so later host commits are outside this run.

Repair commit `6f8385e87ae350d1ce4bf31fca8dc8b423aca29d`, tree `832024e6e0a5052db72be37e7b85bd10468ad12b`. Assembler commit `2bdb3faa6329caa1503dceb52686c18c0be007dd`, tree `8368b5947f4119f258f0dab00ceae3eba1bcea04`. Fixed archive tree `a9369aab2c53afdf94616abee66b5099adc46d3e`. The fixed archive already contains the repair. This review did not copy product files onto the archive.

a284 session `eef831aa-dba4-4af2-a78b-541bc9e0a284` stays **WITHHELD**. Its REPORT, FINDINGS, and HANDOFF were not edited. Its REPORT quotes the author sentence "A `u16` ordinal cannot exceed 32767, which is the contract maximum." That sentence stays historical and false. A `u16` holds `0..=65535`. The contract range remains `0..=32767`. a284 `linux/proof-3` records `FAILED. 14 passed; 1 failed` on the unchanged overlay. The failed test is `ordinals_32768_and_65535_are_outside_the_contract`.

a290 session `eef831aa-dba4-4af2-a78b-541bc9e0a290` records the repair. Its pre-edit red stays historical: `FAILED. 4 passed; 6 failed`. Its later green run is the author's self-review. This rereview does not accept that self-review as the independent result. a290 files were not edited.

## What was reviewed

Scope is the ordinal check already present in the fixed archive of `tools/cdc-transaction`. It is not a new unit and not a new component. `source/numstat.txt` for `6f8385e8` is 29 insertions in `src/assemble.rs` and 630 insertions in `tests/ordinal_range.rs`. `source/blob-identity.txt` records every other CDC file, both locks, and `crates/oo-connectors/Cargo.toml` unchanged from the repair parent through the fixed archive. `assemble.rs` git blob moves from `f47af50c41320b5a7aa9ee7e9d5ddedb60b37952` to `f0b493a6ffdca317de753d1a45ca89a93498888e`. `tests/ordinal_range.rs` is absent on the parent and blob `3dd2e0244c87b5c709c2d6c9dfd8c4b3016983c2` on the fixed archive. Root `Cargo.lock` git blob stays `2990f901ea98a7b465f462cce8742bc5c51c13aa`.

In the archived `assemble.rs`, `stage_data` calls `reject_bad_data` before `replay_data`, `charge_data`, and `store_data`. Inside `reject_bad_data`, `ordinal_above_contract` runs after `validate_table` and before `validate_key` and `validate_payload`. The helper is true when any key cell, any payload cell, or any `unavailable_ordinals` entry is greater than `CONTRACT_MAX_ORDINAL` (`32_767`). The cell match covers `Null`, `Unavailable`, `Bool`, `Int`, `Decimal`, `Text`, and `Bytes`. `fail` sets `resume_required`, stores `self.fault`, and marks the named open transaction `Refused`. `NeedsSpool` is the only code that sets `needs_spool`. `apply_line` counts the raw line, then returns the stored fault when `self.fault` is set. The same assembler does not clear that fault.

`model.rs`, `CONTRACT.md`, the child lock, the root lock, and the existing tests are the bytes in `linux/proof/source-sha256.txt`. Child lock SHA-256 `4f2abdc20f88225bba1b91c7b7dbc1af26102e08144e45dc6f16e04664b1f26a`. Root lock SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Author `ordinal_range.rs` SHA-256 `d3503329dbff0802e4739557761c3c0c62cb1b85a7e2134c1c02deba47c42b9f`.

## Linux proof

`linux/proof`, container `oo-a298-proof`, id `2b412e395cb55ac6f0fb9095a1c74731f0aa46d266dc98c6e210750350b0b153`, removed by its launch script. `docker-run.exit` is 0. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a298-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`, ReadonlyRootfs false. `capacity.txt` recorded `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux arch=arm64`. `console.log` records `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `toolchain-aside.txt` is `no-root-toolchain`. PostgreSQL was not started. `cargo-update-invoked.txt` is `no`. `python-present.flag` is 1. Python was not invoked. This is the only proof launch. `console.log` was not overwritten.

`version.exit` 0, `sealed.exit` 0, `overlay-absent.exit` 0 before the overlays were copied. `lock-compile.exit` 0, `lock-subset.exit` 0, `lock-compare.exit` 0, `lock-summary.exit` 0. `fetch-offline.exit` 101 because the private cargo volume had no `serde_json` yet. Locked online `fetch.exit` 0. That offline miss is the empty-volume fallback.

Author smoke ran before either overlay was copied. `smoke-test.exit` 0. `smoke-count.txt` is 37. `smoke-names.exit` 0. `smoke-test.stdout` result lines: lib 5 in 0.00s, bin 0, cli 3 in 0.01s, handcomputed 19 in 23.68s, `ordinal_range` 10 in 0.01s, doctests 0. The 19 include `txn_of_200000_completes_and_the_next_needs_spool` and `payload_budget_is_exact_and_the_next_event_is_not_stored`. The 10 ordinal names each end `ok`.

`clippy-component.exit` 0. `clippy-before.exit` 0. Clippy before the overlays finished in 1.62s. `clippy-before.stdout` is empty.

The a284 overlay was copied only inside the container, and only after its SHA-256 matched `19c1faeaa5c7503e09e5f522f757a43a8f2602e57dd3644b79f1409b7bc24a20`. The file was not edited. `a284.exit` 0. `a284-count.txt` is 15. `a284-names.exit` 0. `a284.stdout`: `test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 25.60s`. The line `test ordinals_32768_and_65535_are_outside_the_contract ... ok` is present. The same run has `test event_cap_200000_completes_with_zero_value_bytes ... ok` and `test value_byte_budget_is_separate_from_line_length_and_the_event_cap ... ok`. OOMKilled is false.

The fresh overlay is `overlay/a298_ordinal.rs`, SHA-256 `719dfc83e2c0c4ca35a05954e9b79a71449e936267ba99ade706f8d7d6bee0bc`, copied only inside the container after the same pin check. `a298.exit` 0. `a298-count.txt` is 9. `a298-names.exit` 0. `a298.stdout`: `test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s`. `a298.stderr` is the compile line, `Finished test profile [unoptimized] target(s) in 0.25s`, and the running line. The nine names are `fips_and_pinned_preimage`, `api_32767_selected_unselected_key_and_cells`, `api_32768_and_65535_refuse_before_seen_or_buffer`, `api_range_before_key_order_and_unavailable_mismatch`, `api_65536_is_malformed_and_stores_nothing`, `api_stored_event_remains_and_fresh_retry_matches_hash`, `api_existing_malformed_fault_stays`, `cli_32767_positive_and_high_ordinals_do_not_complete`, and `cli_65536_malformed_and_stored_in_range_remains`.

Clippy after both overlays: `clippy.exit` 0, finished in 0.20s, stdout empty. `overlay-present.exit` 0. `product-scope.exit` 0. `src-after.txt` adds only `tests/a284_independent.rs` and `tests/a298_ordinal.rs` to the archive file list. `lock-unchanged.exit` 0. `console.log` ends `worst_phase_exit=0`.

## Independent pin

The fresh overlay pins this preimage and digest. The author pin `ac9bbf8c7334f138906f25c32c151aef7c63e488263fcc683969b9d9f75e3592` is a different transaction and is not this oracle.

```text
{"data_collection_sequence":1,"global_txn_sequence":1,"key":[{"ordinal":0,"type":"int","value":"4"}],"op":"upsert","payload":[{"ordinal":32767,"type":"text","value":"pin"}],"table":"public.orders","txn_id":"r298","unavailable_ordinals":[]}
bcce7305687a58a07e5ea9416487eb2c15b12d2658dffd7dcd81546c33059440
```

Empty SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` and `abc` SHA-256 `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad` are the FIPS vectors in the same test. Value-byte charge for key int `"4"` plus text `"pin"` is 4. The overlay is `#![forbid(unsafe_code)]` and uses the crate hash function. No Python oracle ran.

## Registry triples

`linux/proof/lock-subset.stdout` ends `summary registry_triples=11 missing=0 mismatch=0 path_packages=1 direct_pins_present=true`. The only path package is `cdc-transaction 0.1.0`. Direct pins serde 1.0.229 checksum `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` and serde_json 1.0.151 checksum `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14` are in that list. Each registry source is `registry+https://github.com/rust-lang/crates.io-index`. `cargo update` and `cargo generate-lockfile` were not run. The child lock was not copied back onto the product tree. a207 remains the sole writer of the connector manifest and the root lock.

## Archive bytes the container hashed

`linux/proof/source-sha256.txt`:

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

`source/blobs/tools/cdc-transaction/` is the fixed-archive copy of that tree. `MANIFEST.sha256` is the evidence inventory written by `linux/manifest/launch.sh`. `CLEANUP.txt` records the owned removals.

## Limit of this verdict

The green suites show the repaired normalized assembler, at the SHA above, refuses ordinals 32768 and 65535 with `OrdinalRange` before `seen.data` and the event buffer change, completes ordinal 32767, treats 65536 as `Malformed`, keeps a stored in-range event, keeps an existing fault, and matches `bcce7305687a58a07e5ea9416487eb2c15b12d2658dffd7dcd81546c33059440` on a new assembler. The same runs keep the author 37 and the unchanged a284 boundary file green, including the 200000-event case and the value-byte cap, with OOMKilled false.

Completions inspected by the overlays leave `applied` null and leave the attestation flags false. Those fields read as false here. That observation is not a Debezium proof, a target-applied proof, an ACK proof, an epoch or router proof, a cross-table proof, or a durability proof. `seen` remains a normalized counter. This review does not prove a full parent goal.

## Clarification 2026-10-06

The first wording of this report said "limited review" and "CDC-D01 stays unaccepted" in the opening verdict. That mixed this child with the parent ledger. The proof on disk is unchanged and was not re-run. `linux/proof/docker-run.exit` is 0. Author smoke is 37 passed. Clippy before the overlays exited 0. The unchanged a284 overlay passed 15, including `ordinals_32768_and_65535_are_outside_the_contract`, the 200000-event cap, and the value-byte cap. The fresh overlay passed 9. Clippy after the overlays exited 0. `console.log` ends `worst_phase_exit=0`. No product defect is recorded in those exits.

The frozen child is the bounded Rust normalized PostgreSQL transaction assembler in `tools/cdc-transaction`: BEGIN, DATA, END, rollback, count, hash, caller-declared commit LSN, and read, seen, and complete kept distinct from applied. Ordinals in `CONTRACT.md` are `0..=32767`. a284 withheld the unrepaired assembler because 32768 and 65535 completed. That is the requirement the repair closes. On the repaired SHA above, those ordinals return `OrdinalRange` before the event hash, the buffer, and `seen.data` change. 32767 completes. 65536 stays `Malformed`. A stored in-range event remains. An existing fault stays. A new assembler retries to the pinned hash.

The frozen child text leaves Debezium, P2, P11, a fenced target writer, offset ACK, and a durable large spool as later work. Their absence is not an unmet requirement of this child. Parent SP04 and a release stay unaccepted. a284 files were not edited. a284 stays **WITHHELD** for the unrepaired assembler, including the false `u16` sentence and the historical 14 passed / 1 failed. a290's red 4 passed / 6 failed stays historical.

No tests were re-run for this clarification. `MANIFEST.sha256` is refreshed after this text by one private `rustc` of `tools/sha256_manifest.rs`. That run repeats no test. A first hasher, container `de1fd109b9e505f7450bd4a52736dd583c2c055c431590a8f7c29bd414cae818`, exited 126 because the binary was written on the noexec `/tmp`. The retry writes the binary under `/usr/local/bin` and does not repeat the tests.
