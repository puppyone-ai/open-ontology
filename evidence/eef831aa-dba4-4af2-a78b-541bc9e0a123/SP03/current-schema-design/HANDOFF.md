# Handoff — a123 current catalog guard

Design is frozen. Implementation has not started. R1 is not accepted. The coordinator owns plan checkboxes. This session had no builder and did not launch PostgreSQL.

## Bound input

| | |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a123` |
| Fixed evidence input | `eb288ad96c9941369fc8afa98cb35612b70d23e9` / tree `8c64dcfa9831c1f08dab3d3e43394eea08657ae2` / parent `74fe4f0b8027b9a4b4e3c300998313a596634ae0` |
| Product blobs | Unchanged from `2b6dd57a0dc3d36e098268638c0364af247d07fb` for the five files in `REPORT.md` |
| Workspace tip at write | `3479e440f0e4858983e55014e5f01a4fe733b40e` |
| Stamp | `2026-10-03T08:51:01Z` |

The evidence commit that adds this directory cannot record its own SHA here.

## Files

| File | Role |
| --- | --- |
| `DESIGN.md` | State machine, SQL, admission, compare, contract paragraphs |
| `IMPLEMENTATION-CARD.md` | Coder order and stop conditions |
| `TEST-MATRIX.md` | C0–C15, fixture grant, attnum recording, fixed-SHA rereview |
| `FINDINGS.md` | Proof, hypothesis, rejected locks |
| `REPORT.md` | Decision |
| `HASHES.txt` | SHA-256 of the five files above. It does not hash itself. |

## Proposed product paths

```text
crates/oo-connectors/src/session.rs
crates/oo-connectors/src/sql.rs
crates/oo-connectors/src/snapshot.rs
contracts/source/pgsql/snapshot-owner-v1.md
crates/oo-connectors/README.md
crates/oo-connectors/tests/snapshot_live.rs
```

Slot A implements that list. First command is the Linux arm64 Rust 1.94 smoke in the card, on the frozen tree, before edits. Then one private PostgreSQL 17.10. The new fixture runs `GRANT SELECT, UPDATE ON TABLE lot.tick TO lot_writer` and records actual `pg_attribute.attnum` values. a121's red oracle is not rewritten.

## Coordinator item

One conditional incompatibility. The after-probe case (matrix C2) needs `ROLLBACK TO SAVEPOINT` to drop `ACCESS SHARE` so a committed `ADD COLUMN` can be observed as `schema_drift`. The read-only disallow list does not name `SAVEPOINT`, and the lock chapter says a lock taken after a savepoint is released on rollback to it. a121 did not execute that pair.

If the 17.10 run shows the pair failing, or shows `ALTER` blocked until the data transaction ends, the coder stops. There is then no way to observe committed drift on a relation this transaction has already scanned without ending the repeatable-read transaction. Do not substitute a lock that prevents the `ALTER` and then return `SnapshotFinished`.

No other budget exception is proposed. The owner takes 2 of 4 process-local slots for its lifetime. The cap number stays 4. The ledger is not a cluster lease, which is the same boundary as generation reserve. `REQ-PL-07`, R2, R3, and R4 stay unsigned.

## Cleanup

Nothing was started. No container, volume, or `/tmp` path belongs to this design session.
