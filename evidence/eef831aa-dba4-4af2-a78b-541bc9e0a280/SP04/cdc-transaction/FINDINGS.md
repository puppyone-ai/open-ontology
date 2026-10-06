# Findings

Scope is the frozen CDC-D01 / SP04-W01A assembler only. No new product unit was opened.

## Budget correction

The first contract text charged canonical preimage bytes toward the 32 MiB cap and also said a transaction of exactly 200000 events can complete. A minimal preimage for this schema is about 180 bytes. `200000 * 180` is greater than 33554432, so the event cap could not be reached. `linux/impl-3` showed that: `txn_of_200000_completes_and_the_next_needs_spool` returned `needs_spool` inside the first 200000 events.

The contract now counts stored key and payload value bytes. `null`, `unavailable`, and `bool` contribute none. The canonical preimage remains the SHA-256 input and is not the budget. The two caps are independent. The hand-computed tests cover exactly 200000 events and exactly 33554432 value bytes. Both passed on `linux/impl-7` and `linux/review-1`.

The same impl-3 run also showed `count_overflow_and_bad_lsn_do_not_wrap` stopping at `bind_invalid` because the selected-table list was not sorted. The test now passes `public.lines` before `public.orders`. The assembler behavior was already the sorted-unique check required by the contract.

## Position proof

`oo-core` uses workspace inheritance, so this standalone workspace cannot path-depend on it without a root manifest edit. Root `Cargo.toml` and `Cargo.lock` were not edited. The Position check is an evidence overlay compiled inside a private extract of commit `b859600c095e158a3af3017eb6561962eb780f9a`.

`cargo fetch` in Rust 1.94 does not accept `-p`. `linux/impl-6` exited 1 with `unexpected argument '-p'`. Later runs compile and fetch through `cargo test --locked -p oo-core --test cdc_d01_position --no-run`, then rerun that test `--offline`.

## Clippy

Rust 1.94 Clippy with `-D warnings` rejected five library sites and two binary sites. They were rewritten with `?`, let-chains, and `usize::is_multiple_of`. No `allow` was added for those lints. The shared test fixture allows `dead_code` because each integration-test crate uses a subset of the helpers.

## Assembler limits that stay in force

- Attestations are constants false. `applied` is null. Seen is not applied.
- Commit LSN comes only from `end`. A `row_lsn` field is rejected by `deny_unknown_fields`.
- Identical sequence and payload is idempotent. A conflicting ordinal is `duplicate_conflict` and does not replace the stored event.
- Rollback drops the pending transaction and does not complete it. Rollback of a completed id is `rollback_after_complete`.
- EOF with an open transaction keeps it, sets `resume_required`, and still reports transactions that already completed.
- Open set above 16 is `open_set_exceeded`. Event, value-byte, line, and rolled-back-id caps return `needs_spool` or `line_too_long` before the excess is stored.
- `FaultCode::OrdinalRange` is public and currently unconstructed. A `u16` ordinal cannot exceed 32767, which is the contract maximum.
- `sequence_gap` remains as a completion check. Insert-time uniqueness makes it unlikely once the other count checks have passed.

## Out of scope, still required for a CDC feed

An actual normalizer, Debezium or another decoder, P2, P11, a fenced target writer, offset acknowledgement, and a durable spool for a transaction above these caps are later units. This crate does not open a socket, a database, a source write, a heartbeat, a Valkey ACK, or a publish path.
