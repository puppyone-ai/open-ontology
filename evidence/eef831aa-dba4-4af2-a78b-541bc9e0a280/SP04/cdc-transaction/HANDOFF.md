# Handoff

The assembler and JSONL CLI are in `tools/cdc-transaction`. The contract is `CONTRACT.md`. Evidence for session `eef831aa-dba4-4af2-a78b-541bc9e0a280` is this directory. Green runs are `linux/impl-7` and `linux/review-1`. Failed runs `linux/impl` through `linux/impl-6` stay in place.

The parent ledger row CDC-D01 / SP04-W01 is still open. This work does not check it off.

A later CDC unit still has to supply all of the following before any consumer treats a commit position as applied:

- a real normalizer from the decoder or Debezium envelope into this JSONL contract
- the P2 and P11 checks this crate currently leaves false
- a fenced target merge
- an offset store whose ACK is separate from `seen` and from `complete_decoded`
- a durable spool for a transaction that exceeds 200000 events or 33554432 value bytes

Until those exist, callers must keep `ack_allowed` false and must not advance a source position from this progress object. `commit_position_provenance` is `caller_declared`. The decimal position is an encoding of the declared epoch and commit LSN. It is not a verified PostgreSQL LSN.

The next editor of this crate should keep it a standalone workspace. Adding it to the root workspace, or path-depending on `oo-core`, requires a root manifest change that belongs to the connector lock owner. The Position overlay belongs in evidence, not under `crates/oo-core`.

Rebuild with the pinned linux/arm64 Rust 1.94 image, one builder, 6 GiB, one CPU, and the launch script in `linux/review-1`. Do not start PostgreSQL for this crate.
