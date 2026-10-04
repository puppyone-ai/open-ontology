# Handoff — a146 R4

Verdict: **REQUEST_CHANGES**. The full-mode reader at `f5f0a73b0678e8b1611f1dc4a40e1dbf1d67b10e` is not signed. R4 does not close SP03-W03. Other databases, incremental mode, CDC, the registry, and release stay unsigned. Do not mark the ledger. Do not sign converted types, full type support, or supply or license approval.

## Read these

1. `FINDINGS.md` — `SnapshotFinished` versus `StreamItem::Exhausted`, the four live early finishes, and the `max_value` path that stays an error.
2. `linux/probe-4/r4-outcome.txt` — generations 3, 4, 5, and 6 return `SnapshotFinished` with `exhausted_seen=0`. Generation 6 leaves `r4e.extra` unread. Generation 1 matches the pre-writer cut and is exhausted. Generation 2 matches the post-writer cut by key (`fresh_item_scan=2:oak,1:pine`). Portal finish is `Budget` `max_value`.
3. `linux/probe/finish-outcome.txt` — a144's drained EOF and the bound-portal error, rerun on this archive. `snapshot_finished=0` on `max_value`. `reader_rollback_lines=0`, `reader_reset_lines=1`.
4. `linux/probe/stream-live-outcome.txt`, `stream-bounds-outcome.txt`, `keys-outcome.txt`, `cancel-outcome.txt`, `drop-outcome.txt`, and `linux/probe-2/stream-acl-outcome.txt` — composition on the current blobs. Exits are 0.
5. `linux/probe/pg_stream_acl_live.exit`, `linux/probe/pg_reader_r4_probe.exit`, `linux/probe-2/pg_reader_r4_probe.exit`, `linux/probe-3/pg_reader_r4_probe.exit` — harness 101s. Leave them. They are not product failures.
6. `linux/smoke/lib.exit` and `clippy.exit` — both 0, before any probe. Lib result `63 passed`.

## Next task

One writer task. No fix was applied in this review.

Change `SnapshotOwner::finish` so a loader cannot admit a round unless every selected table reached `TableEof`. Add a public admission value, suggested name `RunComplete`, constructible only after `StreamItem::Exhausted` and the current release checks. Return `protocol` fact `not_exhausted` for finish with no stream, finish before the first poll, finish after one batch while rows remain, and finish when a later selected table was never scanned. Keep `SnapshotFinished` as the close marker, or stop returning it for those four cases. A clean `ROLLBACK` stays release evidence. It is not the admission proof.

Add those four cases as production tests next to `pg_reader_finish_live`. The overlay `overlay/pg_reader_r4_probe.rs` is the shape, including the key-ordered oracle and the writer `SELECT` grant. Update lifecycle lines 13 and 42 so mid-stream finish is described as a close without admission.

Leave `connector_capabilities` and `type_capabilities` `bounded_reader` false in that change. Advertise `bounded_read` only in a later change, after `RunComplete` is the admission value. Leave `type_support.converted` false.

Input blobs to keep unless the finish change edits them: snapshot `f1a71fa73064dfb3b83d7d0183f43c6e1a9751a7`, session `7e191eacbf61439680cbd92a7109ded6c8a05596`, stream `ba9b215a059440a4681578200e7d719cf22407e2`.

## Left as prior stamps

a139 keys remain APPROVE. a143 remains APPROVE LIMITED for cancel and the unpublished run. This session reran those production tests on `f5f0a73b` and they passed. Those stamps do not become a full-mode signature.

## Cleanup

After the evidence commit, remove only `oo-a146-*` containers, the four `oo-a146-eef831aa-*` volumes, network `oo-a146-eef831aa-net`, and `/tmp/oo-a146-eef831aa-dba4-4af2-a78b-541bc9e0a146`. Commands are in `commands.md`. Do not prune. Do not remove `oo-a064-*` or any `oo-a145-*` resource.
