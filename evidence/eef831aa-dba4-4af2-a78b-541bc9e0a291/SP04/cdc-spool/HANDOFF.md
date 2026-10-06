# Handoff

The spool and JSONL CLI are in `tools/cdc-spool`. The contract is `CONTRACT.md`. Evidence for session `eef831aa-dba4-4af2-a78b-541bc9e0a291` is this directory. The green run is `linux/impl-7`. Failed runs `linux/impl-1` through `linux/impl-6` stay in place, with `linux/precode` before them.

Parent acceptance is false. The ledger row CDC-D02 / SP04-W04A is still open. This work does not check SP04 off.

Callers must keep `ack_allowed` false until a later fenced target writer and a real Debezium normalizer exist. `commit_position_provenance` is `caller_declared`. The decimal position encodes the declared epoch and commit LSN. Seal-time progress is durable bytes and event counts. It is not target apply, not a source ACK, and not cross-table consistency.

Still required before a consumer treats a spooled transaction as applied:

- a fenced segment writer
- durable quarantine
- a control offset whose ACK is separate from decoded and durable counts
- spool TTL integration
- a real normalizer into the `cdc-transaction-v1` feed

The next editor should keep this crate a standalone workspace. Root `Cargo.toml`, root `Cargo.lock`, and the connector manifest belong to the connector lock owner. Do not `cargo update` this lock.

Rebuild with the pinned linux/arm64 Rust 1.94 image, one builder, 6 GiB, one CPU, and `linux/impl-7/launch.sh` pointed at a fresh evidence directory so its `console.log` is not overwritten. Do not start PostgreSQL for this crate. The 500000-event fixture is built by `tests/scale.rs` under `TMPDIR` on a normal filesystem, outside the 256 MiB noexec `/tmp` tmpfs.
