# Findings

Verdict: **APPROVE_LIMITED** for the frozen CDC-D01 normalized assembler child, `tools/cdc-transaction`, at `cc98bab7eba2c58c127435db4d2ae777797259de`. Reviewed repaired `assemble.rs` SHA-256 `9b9940554767b88b7d5eef81047cf7590c884e8094a498416a27cfa097621840`. No frozen requirement of that child remains unresolved in `linux/proof`.

Scope is the ordinal check in that fixed archive. This session did not edit the product, the contract range, the `u16` type, the locks, the manifests, `model.rs`, or the existing tests.

## Historical sealed result

a284 stays **WITHHELD**. Its files were not edited. Its REPORT quotes the author sentence "A `u16` ordinal cannot exceed 32767, which is the contract maximum." A `u16` holds `0..=65535`. The contract range is `0..=32767`. 32768 and 65535 decode. 65536 does not decode, and `jsonline.rs` maps that to `FaultCode::Malformed`. a284 `linux/proof-3/test.stdout` records `FAILED. 14 passed; 1 failed` in 25.95s. The failed test is `ordinals_32768_and_65535_are_outside_the_contract`. The actual CLI on that sealed assembler completed 32768 and 65535.

a290 `linux/preedit` remains the red repair attempt: author smoke 27 passed, then `FAILED. 4 passed; 6 failed`. The four passes were the in-range 32767 cases and the 65536 malformed cases. The six failures were completions of 32768 and 65535, plus an `unavailable_ordinals` value of 32768 returning `OrdinalMismatch` where that test required `OrdinalRange`. Those a290 files stay as written. They are not this review's result.

## Check order in the fixed source

`stage_data` calls `reject_bad_data` and returns on that error before the unknown-transaction branch, `replay_data`, `charge_data`, and `store_data`. `charge_data` is where the event hash is built. `store_data` is where the open transaction buffer grows and `SeenField::Data` is bumped. `reject_bad_data` calls `ordinal_above_contract` after txn id, sequence, and table checks, and before `validate_key` and `validate_payload`. The constant is 32767. The fault is the existing `FaultCode::OrdinalRange`.

`apply_line` bumps read lines and bytes first. If `self.fault` is already set, it returns that stored fault and does not parse the new line. `fail` sets `resume_required`, stores `self.fault`, and marks the named open transaction `Refused`. It sets `needs_spool` only for `FaultCode::NeedsSpool`. A later valid line on that assembler is not a retry. A new `Assembler` is the retry the fresh overlay records.

`source/numstat.txt` is 29 insertions in `assemble.rs` and 630 insertions in the author test. `source/blob-identity.txt` shows the root lock blob `2990f901ea98a7b465f462cce8742bc5c51c13aa` unchanged, and `model.rs` blob `d58efdc58b23ac43c8dcd0337a0481eaa2364ebf` unchanged. The container hash of `model.rs` is `47907b5890987c34377bd52b2bf9d2ef1a24c58555dbb8f580b1857970927a98`. `CONTRACT.md` container hash is `e6eb4d6e88c4c26b88dd10758d144eaf30d97cd4667e465e3687bc6625108bf7`.

## Behavior the fresh overlay recorded

`linux/proof/a298.stdout` passed 9 tests in 0.03s. Transaction id is `r298`. Selected table is `public.orders`. Unselected table is `public.notes`. The text pin uses key int `"4"` at ordinal 0 and payload text `"pin"` at ordinal 32767. Charge is 4. Two fresh assemblers of that sequence both produce hash `bcce7305687a58a07e5ea9416487eb2c15b12d2658dffd7dcd81546c33059440`. The same test checks FIPS empty and `abc`.

Ordinal 32767 completes for payload null, unavailable, bool, int, decimal, text, and bytes, and for a key cell at 32767. The unavailable completion stores `unavailable_ordinals` `[32767]`. The text completion stores the pinned hash. An unselected text at 32767 completes with `event_count` 1, `selected_row_count` 0, and empty `rows`. The CLI of the selected text exits 0, stderr empty, one complete, payload ordinal 32767, value `pin`, and the pinned hash. The CLI of the unselected text exits 0 with `selected_row_count` 0 and empty `rows`.

Ordinals 32768 and 65535, for those eight shapes on the selected table, and for text and key on the unselected table, return `OrdinalRange` with `txn_id` `r298`. `seen.data` stays 0. `seen.begin` stays 1. `read.lines` is 3. Pending `data_events` and `payload_bytes` stay 0. Status is `Refused`. `resume_required` is true. `needs_spool` is false. `applied` is null. The progress fields `ack_allowed`, `target_applied`, and `debezium_normalizer` read false. Completes stay 0. The CLI exits 2, stderr empty, fault `ordinal_range`, zero completes, `seen.data` 0, and status `refused`.

An `unavailable_ordinals` entry of 32768 or 65535, with in-range cells, is `OrdinalRange`. A null key cell at those ordinals is `OrdinalRange`, which is the range check before a key fault. A payload cell at 32768 followed by a cell at 0 is `OrdinalRange`, which is the range check before an order fault. An in-range cell followed by a cell at 32768 is `OrdinalRange`.

Raw ordinal 65536 is `Malformed`. `txn_id` is absent. `seen.data` stays 0. Pending status stays `Open`. `read.lines` is 3. Completes stay 0. The CLI exits 2, stderr empty, fault `malformed`, zero completes.

After a stored selected text at 32767, an unselected text at 32768 returns `OrdinalRange` and leaves `data_events` 1, `payload_bytes` 4, and `seen.data` 1. `read.lines` increases by 1. Completes stay 0. Status becomes `Refused`. A following in-range line on that same assembler still returns `OrdinalRange` with `txn_id` `r298`, and the stored counts stay. Two new assemblers of the valid text sequence both match the pinned hash.

An existing `Malformed` from 65536 stays `Malformed` when a later line carries ordinal 32768. `txn_id` stays absent. Pending status stays `Open`. `seen.data` stays 0. `read.lines` increases. Completes stay 0. The CLI of a stored in-range text followed by an unselected int at 65535 exits 2 with fault `ordinal_range`, `seen.data` 1, `data_events` 1, `payload_bytes` 4, status `refused`, and zero completes.

On the 32767 completions the overlay inspects, the seven attestation flags read false, `applied` is null, and `commit_position_provenance` is `caller_declared`. Those reads are the assembler output under test. They are not a Debezium proof, a target-applied proof, an ACK proof, a P2 proof, or a P11 proof.

## Unchanged boundary file

`linux/proof/a284.stdout` passed the unchanged 15-test overlay in 25.60s, including `ordinals_32768_and_65535_are_outside_the_contract`, `event_cap_200000_completes_with_zero_value_bytes`, and `value_byte_budget_is_separate_from_line_length_and_the_event_cap`. The author smoke passed `txn_of_200000_completes_and_the_next_needs_spool` and `payload_budget_is_exact_and_the_next_event_is_not_stored` in the 19-test file, finished in 23.68s. HostConfig OOMKilled is false. The a284 file also contains tests whose names mention an epoch barrier and two tables. Their passing lines are part of that unchanged file. They are not an epoch, router, cross-table, or durability proof.

## What stays outside this child

`seen` is still not an applied position. The event cap and the value-byte cap are unchanged separate checks. Parent SP04 stays unaccepted. A release stays unaccepted. This record does not prove Debezium, target applied, ACK, epoch, router, cross-table consistency, durability, or a full parent. Those items are later work in the frozen child text. They are not an unmet requirement of the normalized assembler.

## Clarification 2026-10-06

The first wording under "What stays open" said "CDC-D01 stays unaccepted" next to the parent. The proof had already passed with no recorded product defect: author 37, unchanged a284 overlay 15, fresh overlay 9, Clippy 0. The only child requirement a284 left open was the ordinal range. `CONTRACT.md` requires `0..=32767`. The unrepaired assembler completed 32768 and 65535. The repaired assembler refuses both with `OrdinalRange` before hash, buffer, and `seen.data` mutation, and the unchanged a284 test `ordinals_32768_and_65535_are_outside_the_contract` passed on `linux/proof`.

No frozen requirement of this child is unresolved on that proof. a284 stays **WITHHELD** on its own unrepaired evidence. Its files were not edited. The false sentence "A `u16` ordinal cannot exceed 32767, which is the contract maximum." stays in the a284 REPORT. a290's red result, 4 passed and 6 failed, stays historical. This clarification re-ran no test. The inventory retry is a hash only. The first hasher exited 126 on the noexec `/tmp`.
