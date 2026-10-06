# Findings

Scope is the frozen CDC-D01 assembler in `tools/cdc-transaction` at commit `2bdb3faa6329caa1503dceb52686c18c0be007dd`, built from fixed archive `c551afea55d5a9164760878a5d6a76835b869875`. No product file was edited. No new product unit was opened. The red test in `linux/proof-3` is the finding. It was not changed to pass.

## Ordinal boundary

`CONTRACT.md` says cell ordinals are `0..=32767`, strictly increasing. `unavailable_ordinals` uses the same ordinals. The cell type stores the ordinal in a `u16`. `u16::MAX` is 65535. `FaultCode::OrdinalRange` exists on the public enum in `src/model.rs` and has no constructor in the sealed tree. A search of `source/blobs` finds the name once, on the enum variant.

`linux/proof-3` drove the compiled CLI, `CARGO_BIN_EXE`:

| ordinal | decoder | CLI exit | completes | fault |
|---|---|---|---|---|
| 32767 | accepts | 0 | 1 | null |
| 32768 | accepts | 0 | 1 | null |
| 65535 | accepts | 0 | 1 | null |
| 65536 | rejects | 2 | 0 | `malformed` |

32767 is inside the contract and completed, with the ordinal stored on the payload cell and on `unavailable_ordinals`. 32768 and 65535 are outside `0..=32767` and also completed. The progress object for those two runs has `fault` null. The refusal the contract names, `ordinal_range`, was not produced. 65536 fails in serde because it does not fit in a `u16`. That is a malformed line, and it is a different fault from `ordinal_range`.

Author FINDINGS, session `eef831aa-dba4-4af2-a78b-541bc9e0a280`, says a `u16` ordinal cannot exceed 32767. That confuses the contract maximum with the type maximum. The type maximum is 65535, and the CLI completes every value the type accepts in this check.

`linux/proof` recorded the 32768 completion and stopped the assertion there. `linux/proof-2` recorded both 32768 and 65535. `linux/proof-3` recorded both again after Clippy on the overlay was clean. The three raw `test.stdout` files stay in place.

## Checks that passed on the same CLI

These passed in `linux/proof-3/test.stdout` and do not close the ordinal clause.

- Begin, data, and end for `west`: event count 2, selected `public.tickets` emitted, unselected `public.notes` counted and omitted, commit LSN taken from `end` as `0/15`, position `18446744073709551637`, commit order 1.
- Interleaved ends: commit order follows end order. `east` at `0/2A` is commit order 1. `west` at `0/2B` is commit order 2. Each sequence stays `1`.
- Rollback of an open transaction completes nothing. A second rollback of the same id increments `duplicate_rollback` and still completes nothing. Rollback after a complete is `rollback_after_complete`.
- A second body at the same sequence is `duplicate_conflict` and leaves `data_events` at 1. The identical body is `duplicate_data` and the completed `event_count` stays 1.
- EOF after one complete and one open transaction exits 3 with `resume_required`, keeps the complete, and keeps the open transaction pending. `seen` counts both data events. `applied` is null.
- A barrier with an open transaction is `open_txn_at_barrier`. A lineage reset allows a lower commit LSN. The encoded position does not include lineage, so reusing epoch 1 and LSN `0/30` after the reset is `commit_position_conflict`.
- A collection count of `u64::MAX` plus another collection count of 1 is `count_overflow` and completes nothing. An end that arrives before the declared count is `end_too_early` and the pending transaction stays refused with its stored event.
- Inserting `row_lsn` `0/FF` on a data line is `malformed`. The earlier complete keeps commit LSN `0/11`. The row field is not used as a commit LSN.
- Empty SHA-256 and the `"abc"` vector match the FIPS 180-4 digests above.
- The 32 MiB figure 33554432 is `pending.payload_bytes`, the sum of stored key and payload value lengths. A bool cell charges 0. A text value of one byte charges 1 even though the JSON line is longer. Exactly 33554432 value bytes, on lines of at most 1048576 bytes and on fewer than 200000 events, stays open and then completes. One more value byte returns `needs_spool` and does not increase `payload_bytes`. Exactly 200000 events with `payload_bytes` 0 completes. Event 200001 returns `needs_spool` with `payload_bytes` still 0. The event cap and the value-byte cap are separate. No resident-set or heap measurement was used.
- On every progress object these tests read, `applied` is null, and `ack_allowed`, `target_applied`, `cross_table_consistent`, `upstream_commit_lsn_verified`, `debezium_normalizer`, `p2_verified`, and `p11_verified` are false. Completed transactions carry the same seven flags false and `commit_position_provenance` `caller_declared`.

## Lock and position smoke

All 11 registry triples in the child lock are present in the fixed-archive root lock with matching checksums. `path_packages=1`, the path package is `cdc-transaction`, and the direct serde and serde_json pins are present. The three `oo-core` Position tests passed before the overlay was copied. They are the author's oracle, byte for byte, and they are a smoke of the decimal encoding. They are not an independent CDC proof.

## What this review leaves in force

The assembler still has no constructor for `OrdinalRange`, so ordinals 32768 through 65535 take the completion path. Callers that trust a completed transaction to contain only ordinals `0..=32767` are ahead of this crate. Seen counters are not an applied position. The decimal position encodes the caller-declared epoch and the `end` commit LSN. A later unit still has to supply a normalizer, the P2 and P11 checks, a fenced target merge, an ACK that is separate from `seen`, and a spool above 200000 events or 33554432 value bytes. This review does not supply them and does not patch the crate.
