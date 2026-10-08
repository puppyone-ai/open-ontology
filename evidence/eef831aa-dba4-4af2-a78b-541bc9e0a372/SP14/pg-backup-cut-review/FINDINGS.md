# Findings

Reviewer: this session, sole coder, tester, and reviewer. No subagent. Product path is the fixed tree `20f065096fa9e20414eed8951d830333160e4af6`.

Verdict on original E51: **WITHHOLD**.

## Missing clause

A logical cut that publishes `cut_proven`, `replay_ready`, and an offset must cover the mutable relations that are actually in the dump, or it must refuse the certificate when those relations change.

On this run the fixture views `oo_dict.calendar` and `oo_dict.enums` were already present. They are `relkind` `v`. The SHARE list is `relkind` `r` and `p` only, captured once at `Barrier::acquire`. `still_held` compares that original list. A comment on each view committed while `shop.entry` and `oo_key_fixture.cut_marker` still showed ShareLock count 1. View and sequence ShareLock counts were 0.

In that same window both databases accepted:

- `CREATE SCHEMA oo_a372_probe`
- `CREATE TABLE oo_a372_probe.added` and `INSERT` of `7`
- `CREATE SEQUENCE oo_a372_probe.seq` and `nextval`
- `CREATE VIEW oo_a372_probe.v`

The fresh restore of the certified dumps has the comments, the row, `last_value` 1, and view `n` 7 on both restore databases. `live/restore-readiness.json` has `cut_proven` true, `replay_ready` true, `history_insufficient` false, and `offset_published` `100000`. The manifest writer blocks name the pre-existing tables only. `oo_a372_probe` is absent from that list. Sealed digests do not name the views or the new table.

`Report::seal` does not inspect extra tables, views, or sequences. `replay_ready` follows the sealed checks plus `cut_proven`. The contract text requires SHARE on user tables and stable sealed digests. It does not refuse a dump that also contains a view comment or a table created after the lock list was captured. The measured tool therefore accepts other state. Calling the result exact fixture-only support would be false. Calling it general PostgreSQL support would be false.

The open SP14-W04 PITR and pgBackRest parent does not close this clause. The certificate under review is the logical slice's own `replay_ready` bit. a368 recorded the uncovered sequences and views as a reason to leave the parent open. That note does not make this certificate true.

## What the sealed writer lock holds

a361 withheld a cut proved only by marker and `count(*)` around two sequential dumps, because an amount update can leave both samples unchanged. The repaired tool blocks that update. This run saw both SHARE locks, got `55P03` on `UPDATE shop.entry SET amount = 1.0001 WHERE entry_id = 3`, kept the amount at `1234567890.1234` until release, then committed `1.0001` on the source. The restore has the original amount. Marker `cut-a356-1|100000` and count `90000` stayed put, which withholds nothing and establishes nothing by itself.

Shop gold hashes equal the a361 oracle. `gold.rs` blob `2564210b226be3d6980265a1dc429cfadf2a613f` is unchanged from that oracle's product blob. The 101-check campaign was not rerun and is not this proof.

The corrupt dump was refused with exit 4 before `CREATE DATABASE`. `replay_ready` false, offset null, `reason` `dump_hash_mismatch`.

`wal_moved` is false. The two dump hashes differ. The manifest does not establish one cross-database snapshot.

## Failed attempts

1. `cut-check` clippy exit 101, `collapsible_if` in `oracle/src/check.rs`. Product test and clippy had already exited 0. The checker was rebuilt. `failed-attempts/attempt-1.txt`.
2. First live harness. Locks were visible, then separate DDL round trips ran before the value update. The update missed the window: `rejected.exit` 0, no `55P03`, source amount became `1.0001`. The restore still contained the boundary objects with `cut_proven` true. Catalog `relkind` concatenation failed with exit 3, so those two catalog lines are empty. That run is `failed-attempts/live-2/` and is not the cut measurement. The owned PostgreSQL container and data volume were removed before the retry. Cargo and target volumes stayed until final cleanup.

The retry is the measurement in `REPORT.md`. `run-live.exit` is 0. `cut-check.exit` is 0. The checker does not score the boundary, so a passing checker and a withhold stand together.

## Non-claims

`development_unit` is false. `parent_acceptance` is false. A restore of the sealed shop rows is not acceptance of E51, not AT-OPS-06, and not a production SLO. Enum and calendar product files stay with a365.
