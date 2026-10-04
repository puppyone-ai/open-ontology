# Handoff — a145 COPY encoder

Coder result on fixed source `6b95d6f2fd2259edfcbffc27a6338518a64dd0fc`. Owned commit `d80472d8a99afd66d8d0e71420f7be2bb62e65c8`. Independent review of that commit is next. Full reader R4 and full apply stay gated. Do not mark the ledger. Do not check SP04-W02 from this evidence.

## Read these

1. `FINDINGS.md` — COPY order, NULL versus unavailable, budget poison, the types PostgreSQL decoded, and the types this codec refuses.
2. `contracts/pipeline/copy-encoder-v1.md` in `d80472d8` — the protocol text that matches the implementation.
3. `linux/focused-6/oracle.txt` — version `17.10`, `server_version_num=170010`, count 2, md5 `bfb64118b3d931be583473c4e5d942e9` equal to the local oracle, 4 chunks, and the two decoded rows.
4. `linux/focused-6/invocation.txt`, `overlay-notes.txt`, and the `*.exit` files — all phase exits 0, overlay of the four owned paths, mapping and Cargo and the type contract unchanged.
5. `linux/smoke/` — package smoke of the fixed archive before the encoder existed. All exits 0. `lib.rs` content sha256 `51ae51157e83e9f892e5588031c39bb9592884a198109aa8b996f268cae15ae8`.
6. Failed consoles, left as recorded: `linux/focused/console.log` (rustfmt of `physical_mapping.rs`, exit 1), `linux/focused-2/console.log` (`try_from` and `?` on `Option`, test exit 101), `linux/focused-3/console.log` (missing `Debug`, test exit 101), `linux/focused-4/console.log` (`matches!` lint, clippy exit 101), `linux/focused-5/console.log` (`true`/`false` versus `t`/`f`, live exit 101).

## What the proof showed

PostgreSQL 17.10 decoded generated binary COPY rows for bool, int8 including `i64::MIN`, text with quote, tab, and newline, SQL NULL, bytea, `numeric(8,2)`, date, and float8 including negative zero. A one-row chunk cap produced two row pieces. The transaction rolled back to ready status `I` and the fixture table's `to_regclass` was null. Local tests refused a zero budget, a value over the byte cap, a row over the row cap, a chunk that cannot hold the header, an unavailable cell, `jsonb`, and a negative numeric scale.

## Left for the reviewer

Confirm `d80472d8` contains only the four paths above and that its parent is `8e0d1da658d65878be96f6f2a7a4478dd3222b19`. Confirm the content sha256 of those blobs still matches `linux/focused-6/expect-*.sha256`, and that `physical_mapping.rs` blob content sha256 is still `981273955bb739c0da502b9ba4b985990b28e81d56a75f88bfd23a0859d056aa`. The live proof hashed the overlay bytes, which are the committed bytes, and did not rebuild after the commit object existed. Time, timestamp, float4, and negative-scale numeric were checked in-process. They were not columns in the live fixture. Domain checks were not re-executed. No production COPY sink was opened.

## Cleanup

Focused-6 already removed containers `oo-a145-focused-6` and `oo-a145-pg`, volumes `oo-a145-eef831aa-cargo`, `oo-a145-eef831aa-target`, and `oo-a145-eef831aa-pg6`, and network `oo-a145-eef831aa-net`. The names are in `linux/focused-6/cleanup-*.txt`. A check after that trap, repeated while this report was written, found no `oo-a145` containers, volumes, or networks. Do not prune. Do not remove another owner's containers.
