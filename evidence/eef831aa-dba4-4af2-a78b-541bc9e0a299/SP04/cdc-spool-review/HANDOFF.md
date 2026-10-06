# Handoff

Independent review of the CDC-D02 spool for session `eef831aa-dba4-4af2-a78b-541bc9e0a299`. Evidence is this directory. The product tree `tools/cdc-spool` was not modified. The fixed commit is `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`, tree `672f73fddb93878ae858aa74999061e8d824cc65`.

Limited verdict: the durable spool passed the preoverlay author suite and `linux/overlay-3`. Parent acceptance is false. SP04 stays open. This review does not release the crate and does not mark the parent goal done.

Green runs:

- `linux/preoverlay`: author suite, 34 passed, 0 failed, Clippy 0, registry 11 triples with 0 missing.
- `linux/overlay-3`: independent boundary file, 13 passed, 0 failed, Clippy 0, same registry result.

Preserved failures:

- Author `linux/impl-1` through `linux/impl-6` stay under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a291/SP04/cdc-spool`. Do not delete them. impl-3's `EndTooEarly` expectation was wrong for an end that named a collection before any row; the product returns `CollectionUnknown` in that case. The fixed tests and this review's overlay both treat those codes as different.
- Review `linux/overlay-1` and `linux/overlay-2` stay here. They failed Clippy on the review file. `linux/overlay-3` is the corrected boundary proof.

Callers must keep `ack_allowed` false. `target_applied`, `cross_table_consistent`, `debezium_normalizer`, and the other attestation flags stay false. `applied` stays null. `commit_position_provenance` is `caller_declared`. Seal-time progress is durable bytes and event counts. It is not target apply, not a source ACK, and not cross-table consistency.

`27754496` is the author impl-7 seal-time `VmHWM` only. This review measured seal-time `34004992` on the author suite and `30089216` on overlay-3, then `47861760` after that overlay's replay. A later run may differ. The seal reading is taken inside `finish`, before replay.

The disk cap is the internal counter `charge`. It is not an OS quota.

The crate lock has 11 registry triples, all present in the root lock at the fixed commit. Do not `cargo update` it. Root `Cargo.toml`, root `Cargo.lock`, and the connector manifest belong to a207.

Still required before a consumer treats a spooled transaction as applied:

- a fenced segment writer
- durable quarantine
- a control offset whose ACK is separate from decoded and durable counts
- spool TTL integration
- a real normalizer into the `cdc-transaction-v1` feed

Rebuild from a fresh evidence directory so existing `console.log` files are not overwritten. Use the pinned linux/arm64 Rust 1.94 image, one builder, 6 GiB, one CPU, and `git archive` of the fixed commit. Do not start PostgreSQL for this crate. Put the 500000-event fixture on a normal filesystem under `TMPDIR`, outside the 256 MiB noexec `/tmp` tmpfs. The boundary source is `overlay/a299_boundary.rs`.
