# Findings

Scope is the CDC-D01 ordinal check in `tools/cdc-transaction`. The sealed behavior is the a284 WITHHELD result. This session adds the missing `FaultCode::OrdinalRange` construction and a new test. It does not widen the contract range, the `u16` type, the locks, the manifests, the existing tests, or `model.rs`.

## What the sealed assembler did

`CONTRACT.md` says cell ordinals are `0..=32767`, strictly increasing, and `unavailable_ordinals` are exactly the unavailable payload ordinals. `Cell` stores `ordinal: u16`. `u16::MAX` is 65535, so 32768 and 65535 decode. 65536 does not decode and `jsonline.rs` maps that to `FaultCode::Malformed`. `FaultCode::OrdinalRange` was declared in `src/model.rs` and had no constructor. a284 `linux/proof-3` completed 32768 and 65535 on the compiled CLI with exit 0, `fault` null, and one complete.

`linux/preedit` reproduced that on archive `87c927b218cb2b6ca9b1759854a4c0fd778e8ffc` before this repair was copied in. The author suite passed: 27 tests, exit 0. The new test then failed 6 of 10. CLI shapes null, unavailable, bool, text, and key at 32768 and at 65535 each exited 0 with `fault` null, one complete, and `seen.data` 1. An unselected `public.notes` event at 32768 also completed, with `selected_row_count` 0. A payload whose cells were in range and whose `unavailable_ordinals` entry was 32768 returned `OrdinalMismatch`. A stored in-range event followed by ordinal 32768 returned `ApplyOut { effect: Accepted, completed: None }`. Ordinal 32767 completed. Ordinal 65536 stayed `malformed`.

## What the repair checks

`reject_bad_data` calls `ordinal_above_contract` before key validation, payload validation, the unknown-transaction branch, replay, `charge_data`, and `store_data`. The predicate is true when any key cell, any payload cell, or any `unavailable_ordinals` entry is greater than 32767. The cell match covers `Null`, `Unavailable`, `Bool`, `Int`, `Decimal`, `Text`, and `Bytes`. `Cell::ordinal` is private, so the helper reads the public fields. `model.rs` is unchanged.

`fail(OrdinalRange, Some(txn_id))` sets `resume_required`, stores `self.fault`, and marks the named open transaction `Refused`. It does not set `needs_spool`. A later `apply_line` still counts the raw line in `read` and then returns the stored fault. Completion stays on `apply_end`. The same assembler does not clear the fault, so a later valid event is not a retry on that assembler. A new assembler of the valid sequence is the retry the tests record.

Because the range check is first among the cell checks, an ordinal above 32767 is `ordinal_range` when an order, mismatch, or key fault would also apply. The tests cover a later high cell after an in-range cell, a high cell followed by a lower cell, and an `unavailable_ordinals` entry that does not match the cells. They drive an open transaction. They do not add a separate unknown-transaction fixture.

## Green behavior the tests recorded

`linux/repair/test.stdout` passed all 10 new tests and the author 27. `linux/repair/a284.stdout` passed `ordinals_32768_and_65535_are_outside_the_contract` and the other 14 unchanged a284 tests.

In-range ordinal 32767 completes for payload null, unavailable, bool, and text, and for a key cell at 32767 with an empty payload. The text case pins this canonical preimage and event hash:

```text
{"data_collection_sequence":1,"global_txn_sequence":1,"key":[{"ordinal":0,"type":"int","value":"4"}],"op":"upsert","payload":[{"ordinal":32767,"type":"text","value":"ok"}],"table":"public.orders","txn_id":"ord","unavailable_ordinals":[]}
ac9bbf8c7334f138906f25c32c151aef7c63e488263fcc683969b9d9f75e3592
```

Attestations on that completion are false. `applied` is null. `seen.data` is 1. `commit_position_provenance` is `caller_declared`.

Ordinals 32768 and 65535, for the same shapes, selected and unselected, return `OrdinalRange` with `txn_id` `ord`. `seen.data` stays 0 when nothing was stored. `transactions` stays 0. Pending `data_events` and `payload_bytes` stay 0. Status is `Refused`. `resume_required` is true. `needs_spool` is false. The seven progress flags stay false. The CLI exits 2 with fault `ordinal_range` and zero completes.

An in-range text payload with `unavailable_ordinals` 32768 or 65535 is `OrdinalRange`. An unselected `public.notes` text at 32767 completes with `event_count` 1, `selected_row_count` 0, and empty `rows`. The same table at 32768 is `ordinal_range`. A stored selected text `"a"` at ordinal 0, followed by an unselected text at 32768, keeps `data_events` 1 and `payload_bytes` 2 and does not complete.

Ordinal 65536 is `Malformed`, `txn_id` is absent, `seen.data` stays 0, and the pending status stays `Open`. The CLI exits 2 with fault `malformed` and zero completes.

After a stored in-range event, a following ordinal 32768 returns `OrdinalRange` and leaves the stored counts in place. Repeating the valid ordinal 32767 on that same assembler still returns `OrdinalRange`. Two fresh assemblers of the valid text sequence both produce hash `ac9bbf8c7334f138906f25c32c151aef7c63e488263fcc683969b9d9f75e3592`.

## What stays open

`seen` is still not an applied position. `applied` stays null. The seven attestation flags stay false. The event cap and the value-byte cap are unchanged. Debezium, ACK, P2, P11, and a durable spool are outside this repair. CDC-D01 is not accepted by this record.
