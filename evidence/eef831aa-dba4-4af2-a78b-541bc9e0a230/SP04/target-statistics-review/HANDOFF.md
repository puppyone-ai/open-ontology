# HANDOFF a230 target statistics review

Product commit `613aa981dc2c57c0005cc6589e1a8ee7757f2a69`. Parent `5af5db7e9bc27a32bacb4d617bd2393c8edc2cb6`. Tree `b3ca121831767fc2b717836889ef25d1e1a719a2`. Author evidence `02b84f4bf731f6a24dfc6aece454d7e875d86938`.

Verdict: **APPROVE LIMITED**. This handoff does not mark a parent card, M1-D06A, M1-I01, M1-I02, or a release.

## Read first

- `REPORT.md` for the exits and the measured facts.
- `FINDINGS.md` F1 before treating attempt 1 as a product failure, and F2 before treating `Reusable` during an open caller transaction as a defect.
- `contracts/pipeline/target-statistics-v1.md` for the refusal order.
- `linux/overlay/live/facts.txt` and `linux/overlay/live/catalog-facts.txt`.

## What this review ran

A private TLS PostgreSQL 17.10 imported three typed rows through `import_unpublished`, observed mapped `reltuples` at -1, refused the closed-identity, mapping, role, owner, and relkind cases, refused one immediate `BEGIN` with no sleep, then `analyze_mapped_shadows` produced a known estimate with `published` false and `estimate_is_exact` false. The existing policy catalog, with `UnknownRows::Refuse`, selected those three cells. Source audit delta was 0. The sentinel stayed -1.

The proof files are `overlay/target_statistics_a230_review.rs` and `overlay/target_statistics_a230_catalog.rs`. They are not product source.

## Failed attempt

Keep `linux/overlay/attempt-1/`. The pipeline test passed. The catalog query matched the gold cells. `finish` failed because the harness searched for `nosleep_kind=OpenTransaction/Reusable` while the facts use two lines. Product source was not edited.

## Limits

`57014` was not provoked. `estimated_rows` is a rounded `reltuples` field. `ClassStatus::Confirmed` was a grant input. The import state stayed `awaiting_publish`.

## Cleanup

`cleanup/run.log` removes container `oo-a230-pg`, volumes `oo-a230-eef831aa-cargo`, `oo-a230-eef831aa-target`, and `oo-a230-eef831aa-pgdata`, network `oo-a230-eef831aa-net`, and `/tmp/oo-a230-eef831aa-dba4-4af2-a78b-541bc9e0a230`. Pinned Rust and PostgreSQL images stay. No prune. Other owners' containers stay.

## Next reader

A statement-timeout case would be a new run. The catalog file is evidence. It is not an `oo-gateway` change.
