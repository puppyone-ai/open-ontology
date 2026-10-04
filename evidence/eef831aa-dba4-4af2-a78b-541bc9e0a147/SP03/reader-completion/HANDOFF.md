# Handoff — a147 reader completion

Production commit `16ca2969cd449653a64d98d9831d3c8b8976942a`, tree `0466e01e2794716f1d6bde65ba4ea82d39b88d1b`, parent `25a844ff54bd07e2b7c5e85423acbeedf2b5b5a7`. Fixed input for the Linux archive was `d80472d8a99afd66d8d0e71420f7be2bb62e65c8`, tree `a3b10827acfac28c4abf986831cbcb26823477dc`. Connector files at the parent match that fixed input. This writer does not sign R4, whole-reader acceptance, converted types, or supply approval. Do not mark the ledger. Leave `scan`, `bounded_read`, and `bounded_reader` false until an independent R4 review.

## Read these

1. `REPORT.md` — path spelling, blob and content hashes, containment, and the three probe exits.
2. `FINDINGS.md` — the four early `not_exhausted` cases, the bound `RunComplete`, and the preserved harness failures.
3. `linux/probe-3/complete-outcome.txt` — ends `complete_live_ok`.
4. `linux/probe-3/finish-outcome.txt` — ends `finish_live_complete`. `SnapshotFinished` remains the close token. Cancel and `max_value` leave `snapshot_finished=0`.
5. `linux/probe-3/console.log` — Clippy exit 0, complete test 1 passed in 2.67s, finish test 1 passed in 1.23s, `docker-run.exit` 0.
6. `linux/probe/console.log` and `linux/probe-2/console.log` — exit 1 (Clippy missing) and exit 101 (test compile). Leave them.
7. `linux/smoke/console.log` — pre-edit archive, lib `63 passed`, Clippy exit 0, elapsed 39s.
8. `linux/probe-3/postgresql.log` — 354666 bytes. Secret-file scan reported zero hits. Do not treat the log as a second oracle.

## API the reviewer should hold fixed

`SnapshotOwner::finish` returns `SnapshotFinished` and releases the owner. That value is non-admission.

`SnapshotOwner::finish_complete` returns `RunComplete` only after this owner delivered `StreamItem::Exhausted` for every selected table. The bit is written at `StreamSlot::next_item` when `covers_selected` is true. `RunComplete` cannot be built outside `snapshot.rs`. Early finish returns `protocol` fact `not_exhausted` and still closes both connections. Cancel, budget, deadline, catalog or ACL failure, and failed cleanup do not mint the token.

`contracts/source/pgsql/bounded-stream-v1.md` was not edited. It still says `finish` is the run finish. The lifecycle contract is `contracts/source/pgsql/reader-lifecycle-v1.md`.

## Blobs

Current:

- snapshot `10176c16a64f45409063a1236c0651065da8a05d`
- stream `5deb30e5da09e58904d04cce22760c5ffc284f96`
- lib `9f233d68a474f26c81201e1e6e025c8a950e3a06`
- lifecycle `bdae1239de6feaf06f73818f54b784dd0c4e1dbb`
- test `26ace07d5c2cadcd2584b43d981372834fc29050`
- session, unchanged, `7e191eacbf61439680cbd92a7109ded6c8a05596`

Input at `d80472d8` and `f5f0a73b`: snapshot `f1a71fa73064dfb3b83d7d0183f43c6e1a9751a7`, stream `ba9b215a059440a4681578200e7d719cf22407e2`, lib `2f590204e95ddbe26f0ad52d7f58b81124bb1c82`.

## Cleanup

After the evidence commit, remove only `oo-a147-*` containers, the four `oo-a147-eef831aa-*` volumes, network `oo-a147-eef831aa-net`, and `/tmp/oo-a147-eef831aa-dba4-4af2-a78b-541bc9e0a147`. Commands are in `commands.md`. Do not prune. Do not remove `oo-a064-*`, `oo-a145-*`, or `oo-a146-*`.
