# Oracle immutable-buffer consumption: strict shared intake

Coordinator decision, 2026-10-27. Recipient: root / shared owner
`01a17985-acd8-71e2-af0a-0d6137423cb1`.
This replaces the assumption that the source must provide a vendor offset per
transaction. It does NOT relax or relabel the existing vendor-offset protocol.

## Runtime capacity integration (2026-10-30)

The reviewed macOS validator digest
`063c97f05f939548c6fd2991a796fd6f53b08f07d59dc72d76855cc5a3e41565`
is explicitly approved by the current Loader. It removes the arbitrary
million-ordinal check while retaining canonical integer, lineage, source
identity, complete scope and vendor-offset separation checks. Existing approved
validators remain valid; a manifest or `trusted` field cannot approve a program.

Run the chosen Loader's `check-native-runtime` with exact driver and validator
path/SHA references before registering it. This command checks that consumer's
actual executable policy without opening a source or target. A validator's old
`registration-recipe` snapshot is not evidence of what a newer Loader trusts.
Normal imports still validate all sealed report/config/receipt/Arrow/registry
digests and execute the approved validator; successful preflight does not waive
those checks or claim a target commit.

The exact new validator has revalidated both retained real 101k-transaction
table receipts byte-for-byte through `native_binding::resolve` and the normal
Loader `trusted_capture` boundary. Changed Arrow, identity/digests, registry pins,
unknown programs and caller trust bits refuse. The owned stopped binding now
selects the new Loader/validator through normal worker APIs, but remains disabled:
prepare the next genuine source rollover before firing it. Historical
occurrences, source instance, confirmed offset and data are unchanged.
This is not a new source transaction, a Linux validator release or a live
million-transaction acceptance.

## Two independent progress authorities

1. **Source capture:** Debezium LogMiner writes real source records to the durable
   raw spool. HTTP 200 is issued only after that SQLite transaction commits.
   Debezium owns its opaque `FileOffsetBackingStore` and schema history. Its final
   actual files are retained byte-for-byte. Source reconnect uses those files,
   source/config identity and log availability; never SCN arithmetic.
2. **Target consumption:** the target holds a real jobs claim/source epoch and
   consumes an immutable captured bundle. Its cursor is
   `(context SHA, bundle SHA, transaction ordinal, transaction ID)`.
   Ordinal is a BUFFER index, not SCN/GTID/vendor position. Target data + receipt +
   buffer progress commit atomically; the worker's real ACK verifies that commit.
   It may not modify Debezium offset/history or advance the source subscription.

If target ACK is lost, replay the exact same buffered transaction. If logs needed
by a later real reconnect are missing, retain the failure and require the normal
recovery/rebaseline path. A buffer cursor cannot reconnect a source.

## Concrete source-owner inputs

Directory:
`evidence/convergence-20261027/oracle-integration/mainline-01/buffer/`

- `context.json`: `oo.c1.oracle-buffer-context/v1`, exact Oracle source identity,
  source_id, actual mainline producer bundle ref, raw spool ref, typed-schema
  digest, source snapshot SCN as evidence, initial cursor and separately retained
  real final offset/history refs. `target_ack_updates_vendor_offset=false`.
- `index.json`: `oo.oracle.buffer-index/v1`, hash-pins the context and every
  `transaction-N.json`. No payload is permitted to redefine the context.
- Each transaction input: `oo.oracle.buffer-consumer-input/v1`, engine/source_id,
  context ref, existing `oo.c1.typed-batch/v3`, and this distinct source position:

```json
{
  "protocol": "oo.c1.oracle-buffer-position/v1",
  "engine": "oracle",
  "checkpoint_kind": "buffer_consumption",
  "context_sha256": "<exact context bytes>",
  "bundle_sha256": "<immutable complete producer bundle>",
  "spool_sha256": "<immutable raw capture>",
  "cursor_before": {"transaction_ordinal": "0", "transaction_id": null},
  "cursor_after": {"transaction_ordinal": "1", "transaction_id": "<real tx>"},
  "transaction_id": "<real tx>",
  "commit_scn": "<real commit SCN; evidence, not ordinal>",
  "event_count": 10000,
  "end_event_count": 10000,
  "event_order_first": 1,
  "event_order_last": 10000,
  "events_sha256": "<ordered exact stored event bytes + newline>",
  "captured_event_ids": {"path": "<absolute>", "sha256": "<hash>", "bytes": 123},
  "end_evidence": {"path": "<absolute>", "sha256": "<hash>", "bytes": 123},
  "typed_batch_digest": "<shared Batch::digest()>",
  "target_apply_ordinal": "1",
  "source_offset_write": false
}
```

No `offset_before`/`offset_after` fields exist in this buffer position.
The real vendor offset appears only in context.source_resume, explicitly final
and not mapped to intermediate transactions. Do not synthesize its bytes from
SCN or serialize a buffer cursor into an opaque-vendor-offset field.

Captured event evidence contains IDs actually read from the immutable spool:
transaction ID, total_order, db/schema/table, source SCN, commit SCN, rs_id and
ssn, plus the exact stored event-body hash. END bytes are copied from the
captured transaction boundary, not generated from the row count. Unknown raw
fields such as a missing redo-thread ID are not invented. These are
capture-scoped IDs, not a claim of a globally ordered Oracle redo position.

## Minimum shared adapter change

Keep `_oo.source_instance` Oracle registration and real jobs lease unchanged.
Only add a **separately tagged buffer checkpoint mode**:

1. `open` takes checkpoint_kind=buffer_consumption and pinned buffer_context;
   verifies its source ID/Oracle identity against the registered source epoch
   and typed-schema digest against typed_tables. Also require the externally
   admitted `buffer_index` ref (not merely a caller's self-declared context hash).
   Bind both context and index SHA immutably
   to the existing run. Do not call it an opaque offset or overload an unlabeled
   vendor checkpoint column. No second lease authority.
2. `apply` dispatches `oo.c1.oracle-buffer-position/v1` separately from
   `oo.c1.oracle-logminer-position/v1`. Existing COPY/key/tombstone semantics and
   target ordinal remain. Require exact registered context/bundle/spool, before
   cursor=stored cursor, after=next captured transaction, complete END/contiguous
   event order, payload hashes and shared typed batch digest. SCNs can jump or
   be equal without merging transactions.
   `buffer_transaction` must match the ordinal's exact entry in that pinned index;
   its source position must equal this apply input and its path-free typed batch
   digest must match. Root/admission must run the complete index/spool validation,
   not treat a transition-only unit helper as artifact authorization.
3. Persist typed rows, transaction receipt, immutable source evidence and new
   **buffer** cursor in the same target transaction. Reject another payload at
   an already-used ordinal. Exact replay returns the original receipt.
4. Worker ACK must check the committed receipt/CAS and return
   `checkpoint_kind=buffer_consumption`, context/bundle SHA, exact new cursor,
   source evidence digest and `source_offset_write=false`.
   Target-only receipts and unknown CAS outcomes are not consumption ACKs.
5. Native admission, mapped/full_import finalization, successful control CAS and
   publication remain separate gates. Buffer progress is not published progress.

The currently implemented shared vendor-offset route should reject these inputs
until that explicit dispatch/migration exists. Do not make it pass by changing
only `protocol` or supplying fabricated per-transaction offset files.

## Source-owner verification

`buffer_cursor.py build` reads only hash-pinned captured artifacts and produces
the context/index/payloads. `verify`:

- rehashes bundle/parts/journal/spool and retained real connector artifacts;
- independently derives every captured ID and event-body digest from raw spool;
- checks actual END ID/count and per-table counts, contiguous event order,
  transaction/batch association and buffer cursor chain;
- rejects context/source/bundle changes, skipped cursor, wrong END, mutated
  evidence, conflicting replay and vendor-protocol relabeling;
- supports same commit SCN for distinct transactions without conflating them.

The transition helper does NOT persist a checkpoint or produce a success ACK.
It returns a proposed next cursor only; the shared target transaction remains
the only authority to commit it. Unit/mock results are not a target acceptance.

The mainline producer argv now accepts a pinned adjacent `buffer_cursor.py` and
`buffer_index` in its config, verifies the full captured input, emits the
structured Oracle registry identity/context, and attaches these payloads to CDC
frames. It requires the actual `oo.c1.typed-ack/v3` receipt with fields
`checkpoint_kind:"buffer_consumption", context_sha256, bundle_sha256,
buffer_cursor:cursor_after, source_evidence_sha256:<full canonical input-position
digest>, source_offset_write:false`. These fields are directly in `receipt`,
NOT in the earlier proposed `buffer_checkpoint` wrapper. The committed
`source_position.buffer_position` contains the same position with artifact
paths removed, and `source_offset_advanced=false`. The shared worker generates
this only after real target transaction/CAS. The producer validates the retained
genuine ACK; a target receipt alone, wrong cursor, altered SCN or offset write
fails closed. See `FORMAL-TAIL-CONTRACT.md` for current code/config pins.

Use the mainline **C1** bundle's original 10,000-row Debezium baseline plus its
four committed transactions. Do not combine the separate final 10,001-row native
snapshot bundle with those earlier CDC events. The independent native snapshot
is the final-state oracle, not the C1 starting cut.

No new source transactions/connector restart were performed for this contract.
The attempted per-transaction vendor-offset experiment was never started and is
archived as UNRUN. All old freeze/bundle bytes are preserved.
