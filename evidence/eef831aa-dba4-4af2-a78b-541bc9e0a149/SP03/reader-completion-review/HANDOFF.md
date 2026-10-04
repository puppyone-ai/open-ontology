# Handoff — a149 reader completion review

Verdict: **APPROVE_LIMITED**. Production API commit `16ca2969cd449653a64d98d9831d3c8b8976942a`, tree `0466e01e2794716f1d6bde65ba4ea82d39b88d1b`, parent `25a844ff54bd07e2b7c5e85423acbeedf2b5b5a7`.

This review signs the PostgreSQL full-mode reader production API: `finish_complete` returns `RunComplete` only for a covering `StreamItem::Exhausted` plus the existing release checks, and `finish` returns non-admitting `SnapshotFinished`. A later target-apply decision can cite that signature as the reader prerequisite. Do not check SP03-W03. Do not edit the ledger. Leave `scan`, `bounded_read`, and `bounded_reader` false until a separate activation. Do not sign full types, converted types, CDC, incremental mode, other databases, the registry, release, or supply approval.

Author evidence `3df15045` has 112 `SHA256SUMS` lines and tree `cd2131bd11ce594be2b50ff50e2ac36d19d4359c`. Its live tests exited 0. That run is not this approval. The independent outcome is `linux/probe/review-outcome.txt`, which ends `r4_review_ok`.

## Read these

1. `REPORT.md` — requirement map, blob table, containment, and both probe exits.
2. `FINDINGS.md` — the four early `not_exhausted` cases, the bound token, the subset cases that emit `Exhausted` and still do not mint, and the cleanup fact `database`.
3. `commands.md` — launch lines and the cleanup that runs after the evidence commit.
4. `linux/probe/review-outcome.txt` — fresh oracle. Generation 1 digest `f34ebaca9313b6951ad5ecefe19137a0044a07e5076841d154bd19d2bf7955fd`. Schema hash `b44ae80c153942684999963bca7017de7d009524917200bbb2dba836e27e5dac`.
5. `linux/probe/console.log` — clippy exit 0, review test 1 passed in 12.83s, `private_construction_rejected=1 cargo_exit=101`, `production_files_unchanged_after_probe=1`, `docker-run.exit` 0.
6. `linux/probe/private-construct.stderr` — E0639, E0308, E0599, E0603, E0423.
7. `linux/probe-1/console.log` — overlay compile exit 101. Leave it. Do not treat it as a product failure.
8. `linux/smoke/console.log` — archive of `16ca2969`, lib `63 passed`, clippy exit 0, elapsed 40s.
9. `linux/probe/postgresql.log` — 459328 bytes. Secret-file scan reported zero hits. Do not treat the log as a second oracle.

a146 evidence to reuse, not rerun: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a146/SP03/reader-r4-review/`. Its REQUEST_CHANGES was the missing admission value. The unchanged blobs listed below are the ones that review already executed.

## API to hold

`SnapshotOwner::finish` returns `SnapshotFinished`. That is lifecycle release. Generation 4 in the outcome delivered `Exhausted` and `finish` still returned `SnapshotFinished` with `admission=0`.

`SnapshotOwner::finish_complete` returns `RunComplete` only when this owner delivered `StreamItem::Exhausted` for every selected table. The bit is stored when the consumer takes `Exhausted` from a covering scan. `TableEof` does not store it. A key range and a chunk can deliver an `Exhausted` item with `covers_admission=0` and then `finish_complete` returns `protocol` fact `not_exhausted`. Early shapes return the same fact after both connections close.

`RunComplete` is `non_exhaustive` and holds a private seal. External code cannot construct it. The token binds source id, generation, config identity, type-policy identity, `PgTypePolicy`, input digest, schema hash, the open catalog, and the selected tables.

Cancel returns `cancelled`. A portal past `max_value` returns `Budget` `max_value` ahead of cleanup. A passed deadline returns `Budget` `deadline`. ACL loss returns `Privilege` `select_not_granted`. Catalog drift returns `Catalog` `schema_drift`. Terminated backends in this run returned `Protocol` `database`. None of those minted `RunComplete`. The string `cleanup_unconfirmed` was not observed.

## Terminology the coordinator should not treat as a product defect

`contracts/source/pgsql/bounded-stream-v1.md` line 9, blob `c0f01b52c4a408a7838b607e29322f318f16372e`, still calls `finish` the run finish. `crates/oo-connectors/src/key_plan.rs` lines 67–68, blob `8f152763844330532df58efdc77568e1f36e3d40`, still say the loader treats anything other than `SnapshotFinished` as incomplete. `contracts/source/pgsql/reader-lifecycle-v1.md`, blob `bdae1239de6feaf06f73818f54b784dd0c4e1dbb`, is the admission contract: only `RunComplete` may admit an eventual loader. Aligning the two older sentences is a documentation follow-up. This review did not edit them.

Capability flags remaining false are the same kind of follow-up. The lifecycle contract still says they stay false. Activate them in a later change after this acceptance. Do not reopen R4 because they are false.

## Blobs

Changed at `16ca2969` relative to a146 `f5f0a73b`:

- snapshot `10176c16a64f45409063a1236c0651065da8a05d`
- stream `5deb30e5da09e58904d04cce22760c5ffc284f96`
- lib `9f233d68a474f26c81201e1e6e025c8a950e3a06`
- lifecycle `bdae1239de6feaf06f73818f54b784dd0c4e1dbb`

Unchanged, and the reason the a146 matrix was reused:

- session `7e191eacbf61439680cbd92a7109ded6c8a05596`
- key_plan `8f152763844330532df58efdc77568e1f36e3d40`
- model `0d7af7c1b9b7ffc30bbecdc75b934c965c8dbefa`
- types/mod.rs `9920752ceee306796ff5ea4113f5d6109b659555`
- sql `6badaf187e33346c1657c632e6d5c43fe90f1ea2`
- connector manifest `e8e3254da87a29ce90c485e8ad6566dbd4654f43`
- `Cargo.lock` `15ae1071c7236a956547e3f1d37d627b2cbebfce`
- bounded-stream `c0f01b52c4a408a7838b607e29322f318f16372e`
- reader-keys `346833019418700f39e3cc3061d74dda4dbcabd8`

## Cleanup

After the evidence commit, remove only `oo-a149-smoke`, `oo-a149-probe`, `oo-a149-pg`, the four `oo-a149-eef831aa-*` volumes, network `oo-a149-eef831aa-net`, and `/tmp/oo-a149-eef831aa-dba4-4af2-a78b-541bc9e0a149`. Commands are in `commands.md`. Do not prune. Do not remove `oo-a064-*`, `oo-a145-*`, `oo-a146-*`, `oo-a147-*`, or `oo-a150-*`. The probe-1 container was already removed so the green probe could take the name `oo-a149-probe`. Its logs remain under `linux/probe-1/`.
