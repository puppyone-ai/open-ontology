# CDC transaction independent review

Verdict: **WITHHELD**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a284`. Reviewed assembler commit `2bdb3faa6329caa1503dceb52686c18c0be007dd`, tree `8368b5947f4119f258f0dab00ceae3eba1bcea04`. Fixed archive `c551afea55d5a9164760878a5d6a76835b869875`, tree `e03c77fde3be687ace03984569e128407618017a`. `tools/cdc-transaction` is byte-identical between those commits. The proof built a `git archive` of the fixed commit. No product file, root lock, spec, or ledger was edited.

`linux/proof-3` is the decisive run. The actual CLI completed unavailable ordinals 32768 and 65535. The contract range is `0..=32767`. `FaultCode::OrdinalRange` is declared in `src/model.rs` and is not constructed anywhere in the sealed sources. Author session `eef831aa-dba4-4af2-a78b-541bc9e0a280` FINDINGS says: "A `u16` ordinal cannot exceed 32767, which is the contract maximum." A `u16` holds `0..=65535`. That sentence is false. The ordinal clause stays unaccepted.

This record leaves CDC-D01 open. It leaves SP04 open. It leaves Debezium, ACK, P2, P11, a durable spool, and a release unaccepted. The author `linux/review-1` is not this review.

## Decisive run

`linux/proof-3`, container `oo-a284-proof3`, id `b8669d262ef8802da4783534cef1491d15cc0402dd9823f130520b02f655a3cb`. HostConfig: ExitCode 101, OOMKilled false, Privileged false, NetworkMode `oo-a284-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`, ReadonlyRootfs false. `capacity.txt` recorded `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0`. `docker-run.exit` is 101. `test.exit` is 101. `clippy.exit` is 0.

`linux/proof-3/console.log`: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, `cargo 1.94.0 (85eff7c80 2026-01-15)`, Debian 13, `version.exit` 0. `repo-head.txt` is `d738a621c296e97458e2b9dd30947dd03bc1689b`. The image is `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64. PostgreSQL was not started.

Exits on that container, in order: `blob-cmp.exit` 0, `lock-subset.exit` 0, `lock-compare.exit` 0, `fetch-offline.exit` 0, `fetch.exit` 0, `overlay-absent.exit` 0, `smoke-test.exit` 0, `smoke-count.txt` 27, `smoke-names.exit` 0, `position.exit` 0, `position-count.txt` 3, `position-names.exit` 0, `overlay-present.exit` 0, `test.exit` 101, `clippy-component.exit` 0, `clippy.exit` 0, `product-unchanged.exit` 0. `cargo-update-invoked.txt` is `no`. `clippy.stdout` is empty. Clippy stderr ends `Finished dev profile [unoptimized] target(s) in 0.71s`. `python-present.flag` is 1. Python was not invoked inside the container.

## Author smoke, before the overlay

`cargo test --locked --offline -- --test-threads=1` on the sealed archive, with `tests/a284_independent.rs` absent. `linux/proof-3/smoke-test.stdout`:

- lib: 5 passed, 0.00s
- bin: 0 passed
- `cli`: 3 passed, 0.01s
- `handcomputed`: 19 passed, 23.72s
- doctests: 0

The passed fields sum to 27. `linux/proof` and `linux/proof-2` recorded the same 27 before their overlay failures.

Position overlay, the byte copy of the a280 oracle, compiled against `oo-core` from the fixed archive: `frozen_positions_fit_oo_core_position`, `max_epoch_and_lsn_fit_and_wider_values_do_not`, `sample_timestamps_keep_the_stored_spelling`. `linux/proof-3/position.stdout`: `ok. 3 passed; 0 failed; finished in 0.00s`.

## Independent CLI

`cargo test --locked --offline --test a284_independent -- --test-threads=1`. `linux/proof-3/test.stdout`: `FAILED. 14 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 25.95s`.

The failed test is `ordinals_32768_and_65535_are_outside_the_contract`, panic at `tests/a284_independent.rs:558:5`:

```text
contract ordinals are 0..=32767 and FaultCode::OrdinalRange is the refusal; ordinal 32768 code 0 fault null completes 1; ordinal 65535 code 0 fault null completes 1
```

Both feeds are one `unavailable` cell plus `unavailable_ordinals`, transaction `ord`, commit LSN `0/5`. The CLI exited 0, stored one `complete_transaction`, and wrote `fault` null. The assertion still requires exit 2, fault `ordinal_range`, and zero completes. It was not weakened.

Passed on that same run:

- `sha256_known_vectors`: empty preimage `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`, `"abc"` preimage `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.
- `begin_data_end_count_filter_and_attestations`: transaction `west`, event count 2, selected row count 1, unselected `public.notes` omitted from `rows`, commit LSN `0/15`, position `18446744073709551637`, commit order 1, seven attestations false, `applied` null, `commit_position_provenance` `caller_declared`, `seen.data` 2.
- `interleaved_end_order_keeps_each_sequence`: `east` ends first at `0/2A` with commit order 1; `west` ends second at `0/2B` with commit order 2. Each row keeps `global_txn_sequence` 1.
- `rollback_does_not_complete_and_replay_is_idempotent`: rollback then the same rollback again completes nothing, `seen.rollback` 1, `seen.duplicate_rollback` 1. A later end of `back` completes once; a rollback after that complete exits 2 with `rollback_after_complete`.
- `duplicate_payload_conflicts_and_identical_does_not_double`: a changed body at the same sequence exits 2 with `duplicate_conflict` and `data_events` 1. The identical body completes with `event_count` 1 and `seen.duplicate_data` 1.
- `eof_pending_keeps_completed_and_open`: exit 3, `resume_required` true, fault null, completed `kept` stays, `open1` stays pending with `data_events` 1 and `payload_bytes` 1, `applied` null.
- `lineage_reset_clears_floor_and_keeps_position_identity`: a barrier while `alpha` is open is `open_txn_at_barrier`. After a reset, `beta` completes at the lower LSN `0/10`. `gamma` reuses the encoded position of `alpha` and exits 2 with `commit_position_conflict`.
- `count_overflow_does_not_wrap_and_early_end_keeps_pending`: `u64::MAX` plus 1 is `count_overflow` and completes nothing. An end that declares 2 events after 1 stored event is `end_too_early`, pending `data_events` 1, status `refused`.
- `row_lsn_is_not_a_commit_fallback`: `kept` completes at `0/11`. A following data line that inserts `row_lsn` `0/FF` exits 2 with `malformed`. The stored position is the end LSN, and it differs from `position(1, 0xff)`.
- `u16_decoder_accepts_32768_and_65535`: `serde_json` accepts both as `Cell::Unavailable`. 65536 does not decode.
- `ordinal_32767_completes_inside_the_contract`: exit 0, stored ordinal 32767, attestations false, `applied` null.
- `ordinal_65536_is_malformed_because_it_does_not_fit_u16`: exit 2, fault `malformed`, zero completes.
- `value_byte_budget_is_separate_from_line_length_and_the_event_cap`: a one-character text charges `pending.payload_bytes` 1. Thirty-two bool cells on a line longer than 32 bytes charge 0. A plan of exactly 33554432 value bytes, with each line at most 1048576 bytes and fewer than 200000 events, leaves the transaction open with `payload_bytes` 33554432 and `needs_spool` false, and the matching end exits 0. One extra value byte exits 2 with `needs_spool` while `payload_bytes` stays 33554432. The oracle reads `pending.payload_bytes`. It does not read RSS.
- `event_cap_200000_completes_with_zero_value_bytes`: 200000 bool-key delete events on unselected `public.audit` exit 0 with `event_count` 200000 and `selected_row_count` 0. Event 200001 exits 2 with `needs_spool`, `data_events` 200000, and `payload_bytes` 0.

`seen` is counted. `applied` stays null on every progress object those tests inspect. The seven attestation flags stay false.

## Registry triples

`linux/proof-3/lock-subset.stdout` lists 11 registry triples and `summary registry_triples=11 missing=0 mismatch=0 path_packages=1 direct_pins_present=true`. `lock-compare.exit` is 0 against `oracle/registry-triples.txt`. The only path package is `cdc-transaction 0.1.0`. Direct pins serde 1.0.229 checksum `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` and serde_json 1.0.151 checksum `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14` are in that list. Source for each registry package is `registry+https://github.com/rust-lang/crates.io-index`.

Child `Cargo.lock` SHA-256 `4f2abdc20f88225bba1b91c7b7dbc1af26102e08144e45dc6f16e04664b1f26a`. Root `Cargo.lock` SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. The proof copied the child lock into `linux/proof-3/Cargo.lock` and did not copy it back onto the product tree. `product-unchanged.exit` is 0.

## Archive blobs

`linux/proof-3/source-sha256.txt`:

| file | sha256 |
|---|---|
| `CONTRACT.md` | `e6eb4d6e88c4c26b88dd10758d144eaf30d97cd4667e465e3687bc6625108bf7` |
| `Cargo.lock` | `4f2abdc20f88225bba1b91c7b7dbc1af26102e08144e45dc6f16e04664b1f26a` |
| `Cargo.toml` | `421944a6eeacaa00cf1720d01b76787f0e08e3cb488336b5aa313e84afb8ee1d` |
| `src/assemble.rs` | `ac92b4300e76a33e8a1b9185269d4750b6355c9955136f7765b95c4715aafe8a` |
| `src/jsonline.rs` | `63477fa357fbac7b828b0c0d2b050892e9d0dbef4164dc40d0040e75c9a9fcd7` |
| `src/lib.rs` | `21051ceb58244b1c733df919fcb6b313a64ac5245477cd375ee4e2268580c1a9` |
| `src/lsn.rs` | `442a7b05f2799c2f432c2c83d693bb3956e8bfd9a2d9e4b46e7b2a792e7d14b8` |
| `src/main.rs` | `42f85ab77a6737837cbf687b9631a5b1493041881c6ab3b4f6414ddb3b58821c` |
| `src/model.rs` | `47907b5890987c34377bd52b2bf9d2ef1a24c58555dbb8f580b1857970927a98` |
| `src/sha256.rs` | `eac61aa8d9d0eebe935be0d438b6877b40845656601c4c5a8180edd97963f037` |
| `tests/cli.rs` | `2f4e942459ad5ba68f363183fda01c6151d1c6bdb1afc1a1b6b735814e8b30ea` |
| `tests/common/mod.rs` | `38c7290213f50e11dd008b1a2115734d9b6ae16af77f4d54fecf817bae3d3fc0` |
| `tests/handcomputed.rs` | `8c538af3afa7ca657fefabcefaae505e1c15c0a7a473ce7b8eb74c62c72b84ba` |

## Preserved attempts

| run | container | id | docker exit | smoke | position | overlay | clippy |
|---|---|---|---|---|---|---|---|
| `linux/proof` | `oo-a284-proof` | `b2098daa4932f44013a7793ef3279b611ba0e05fd67d554f7daecec5ef9334ae` | 101 | 0, count 27 | 0, count 3 | 101 | 101 |
| `linux/proof-2` | `oo-a284-proof2` | `c87ae9fa595cd109905322c1e7d3001b8fb5120ca58b32074fdc07dc17d82a38` | 101 | 0, count 27 | 0, count 3 | 101 | 101 |
| `linux/proof-3` | `oo-a284-proof3` | `b8669d262ef8802da4783534cef1491d15cc0402dd9823f130520b02f655a3cb` | 101 | 0, count 27 | 0, count 3 | 101 | 0 |

All three: OOMKilled false, Privileged false, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000, NetworkMode `oo-a284-eef831aa-net`. `linux/proof/capacity.txt` recorded `own=0 global=1 host_cargo=0` and the container still started, because the own cap is 1 and the global cap is 10. `linux/proof/fetch-offline.exit` is 101 and `fetch.exit` is 0. `linux/proof-2` and `linux/proof-3` reused the private cargo volume and fetched offline with exit 0. `linux/proof/repo-head.txt` and `linux/proof-2/repo-head.txt` are `216e6df82a61d2bc22548ce1a5075d5f4a045648`.

`linux/proof` stopped inside the ordinal test at 32768: the CLI completed the transaction, and the loop did not reach 65535. Clippy then rejected `needless_lifetimes` on the overlay helper `last`. The overlay that ran is `linux/proof/failed-a284_independent.rs`.

`linux/proof-2` ran both ordinals and recorded the same completion pair that `linux/proof-3` records. Clippy then rejected `field stderr is never read` on the overlay `Out` struct. The contract assertion was unchanged.

`linux/proof-3` dropped that unused field, kept the contract assertion, and Clippy exited 0. The red test remains the product finding.

## Budget and inventory

Review builders: `linux/proof`, `linux/proof-2`, `linux/proof-3`, then the manifest pair in `linux/manifest/launch.sh`. One builder at a time, 6 GiB, 1 CPU, 0 PostgreSQL. The manifest uses network `none` and no cargo volume. `CLEANUP.txt` records removal of `oo-a284-eef831aa-net`, `oo-a284-eef831aa-cargo`, `oo-a284-eef831aa-target`, and `/tmp/oo-a284-eef831aa-dba4-4af2-a78b-541bc9e0a284`. The pinned image stays. `docker prune` was not run. Foreign owners were not touched.

`MANIFEST.sha256` is the second run of the byte-copied a199 `sha256_manifest.rs` (`d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`). The tool omits its output path. The refresh stdout, refresh hostconfig, and refresh exit stay under `/tmp/oo-a284-eef831aa-dba4-4af2-a78b-541bc9e0a284/manifest-outside` and are removed when that refresh exits 0. They are not rows.
