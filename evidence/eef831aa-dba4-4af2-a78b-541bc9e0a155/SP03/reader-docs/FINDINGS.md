# Findings — a155 reader documentation alignment

The nine stale sentences named by a153 are aligned to the accepted reader API. The alignment is documentation. It does not add a reader, a loader, or a capability flag.

Fixed source `bedd6a2ced65edf6a44ac85ecd40551e1b79a45c`. a153 approval `b476c7561f7507b8f17128407a257907172b6f9a`. a149 reader API `16ca2969cd449653a64d98d9831d3c8b8976942a`. Docs commit `e6bd4c7aa8c3a43cc0516a21e325765c5eecb2f6`.

## What the sentences now say

`scan` and `bounded_read` are supported. Type capability `bounded_reader` is supported. The reason on all three is `PostgreSQL full-mode snapshot plus bounded pull and RunComplete admission`.

`SnapshotOwner::finish` returns `SnapshotFinished` as lifecycle close only. That value is not admission. `SnapshotOwner::finish_complete` returns `RunComplete` as full-selection admission only after this owner delivered `StreamItem::Exhausted` for every selected table and the existing release checks passed.

R1 remains the snapshot owner's open, probe, and lifecycle close. R2 remains the bounded Arrow batch stream and budgets (`bounded-stream-v1.md`). R3 remains key planning, cancellation, and whole-round failure (`reader-keys-v1.md`, `reader-lifecycle-v1.md`). R4 remains full-selection admission on that path. Those names have the limited acceptance above.

A key chunk is not full-run admission. A key-range read or a single-table chunk does not record `Exhausted` for every selected table. Loss of the snapshot does not append later chunks and does not reconnect under the same generation. The whole unpublished run restarts on a new generation, with no mixed cut. A later generation is a new cut and reads the committed rows again.

## What stays unsupported

`cdc`, `type_conversion`, and `registry_api` stay `not_in_this_slice`. `source_ddl` and `source_write` stay `forbidden`. Incremental mode stays unsupported. Resume cursors stay unsigned. Catalog `type_conversion` stays `unsupported`. Per-column `type_support.converted` stays false. A caller `converted=true` flag is still ignored. `numeric_parts` stays the unsigned inspection decoder. MySQL unsigned, PositionCodec, CDC, Loader reject persistence, and user-defined output stay unsupported on the type-capability list.

The decode entries that were already true stay true. They are not a whole-type certificate. This alignment does not certify full types, a customer proof, or a release. SP03-W01 stays incomplete in the catalog contract. SP03-W03 was not checked. The ledger was not edited.

## Loader

No production target loader admits `RunComplete`. The reader token is the prerequisite only. a154 is the moving full-import writer. This documentation does not describe that writer as implemented.

## Lines

| File | Was | Now |
| --- | --- | --- |
| `crates/oo-connectors/README.md` | `bounded_read` unsupported. R2, R3, and R4 unsigned. | Limited full-mode acceptance. `finish` is close. `finish_complete` is admission. |
| `crates/oo-connectors/README.md` | `scan` and `bounded_read` grouped with the unsupported capabilities. | `scan` and `bounded_read` supported. `cdc`, `type_conversion`, and `registry_api` stay unsupported. `bounded_reader` uses the same reason. |
| `contracts/source/pgsql/README.md` opening | This slice does not implement scan or bounded read. | The catalog protocol still does not implement CDC, type conversion, or a registry API. `scan` and `bounded_read` are the separate reader. |
| `contracts/source/pgsql/README.md` capabilities | `scan` and `bounded_read` are `not_in_this_slice`. | Those two are supported with the admission reason. The other three stay `not_in_this_slice`. |
| `reader-keys-v1.md` opening | `connector_capabilities` and `bounded_reader` stay unchanged. | Those flags are supported. A key chunk does not record admission. |
| `reader-keys-v1.md` close | `SnapshotFinished` is the only successful close. | Close versus admission, and a lost snapshot restarts the whole unpublished run with no mixed cut. |
| `type-conversion-v1.md` opening | `connector_capabilities` stays unsupported, including `type_conversion`. | `type_conversion` stays unsupported. `scan` and `bounded_read` are not a type certificate. |
| `type-conversion-v1.md` limits | The bounded reader stays unsupported. The slice does not claim the reader or the Loader. | `bounded_reader` is supported for the limited path. The protocol does not claim a production target loader. |
| `snapshot-owner-v1.md` opening | R4 is unsigned. `scan` and `bounded_read` stay unsupported. | R1 stays this owner. R2, R3, and R4 have the limited acceptance in the sibling contracts. |

`snapshot-owner-v1.md` also states the same close-versus-admission split in the finish paragraph, and the later-stream paragraph names `bounded-stream-v1.md` as the pull. The probe and `finish` still do not implement cancel. Protocol ids are unchanged: `oo.source.pgsql/v1`, `oo.source.pgsql.snapshot/v1`, `oo.source.pgsql.keys/v1`, `oo.source.pgsql.types/v1`.

## Link that was already absent

`crates/oo-connectors/README.md` still links `supply-chain/pg-source-dependency-delta.json`. That file is absent at `bedd6a2c` and at this commit. The sentence was not on the nine-line list. It was left as written. `checks/links.txt` records `missing_count 1` for that path. The links added in this change resolve.

## Smoke before the edit

`linux/smoke/fixed-blobs.txt` records the five documentation blobs at their pre-edit ids. `archive_hashes_match=1` was printed inside the container before `cargo test`. The docs commit came after that smoke exited 0.
