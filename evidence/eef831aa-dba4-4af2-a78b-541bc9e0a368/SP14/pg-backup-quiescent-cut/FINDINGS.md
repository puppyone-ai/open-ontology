# Findings

Reviewer: this session, after the live run. No subagent. Verdict on original E51: unaccepted. `parent_acceptance` remains false.

## What the repair shows

The a361 withhold was that marker, count, and LSN samples around two sequential dumps do not prove one quiescent cut. A value update that leaves `oo_key_fixture.cut_marker` and `count(*)` of `shop.entry` unchanged was invisible.

The repaired backup holds `SHARE` on every ordinary and partitioned user table in both databases for the whole gold `COPY` and both dumps. `SHARE` blocks `ROW EXCLUSIVE`, including for the table owner. It does not conflict with `ACCESS SHARE` from `COPY` or `pg_dump`. The live adversarial update is that case: `amount` on `shop.entry` `entry_id` 3 changed from `1234567890.1234` to `1.0001` only after the locks dropped. Marker and count stayed `cut-a356-1|100000` and `90000`. The dump restored the pre-update amount. The manifest records `cut_mode` `writers_blocked`, `probe_rejected` true on both sessions, and `shop.entry` `gold_sha256` `a11369572ab597c2211d47df03b92c2a1b2da88451bfb5439c1d7ecf2cb10450`, the same hash as `shop-entry.copy`.

`history_insufficient` false on the success report is tied to that proof. `cut_proven` is true only then. `Report::refused` sets `history_insufficient` true and `cut_proven` false. `seal` drops the offset when the cut is unproven or history is insufficient. The corrupt-dump refusal did that, and it did it before `CREATE DATABASE`.

`quiescent` is written by the manifest encoder as the opposite bit. The operator parser rejects a caller `quiescent` key (unit test, exit 2).

## What stays open

E51 is the full recovery goal, not this logical slice. The following stay open:

- SP14-W04 and AT-OPS-06
- PITR, WAL archiving, and pgBackRest. This manifest has `wal_moved` false and `pitr_ready` false
- cluster roles and runsc
- `enum_calendar_same_database`, still owned by a365. Those migration files were not edited
- any claim that two databases share one PostgreSQL snapshot. They do not. The proof is the pair of writer blocks plus the bound digests

`SHARE` covers user tables (`relkind` `r` and `p`). It does not cover sequences or views. A concurrent `nextval` can still change sequence state inside a dump. That is outside the frozen table-writer rule and is one reason this slice does not accept the parent goal.

The 101-check oracle from a361 was not rerun. Its withhold on the old marker rule is unchanged as a historical result. This repair replaces that rule. It does not reopen that campaign.

## Failed attempts

1. `cut-check` did not compile. A format string called a function inside braces. Product test and clippy had already exited 0.
2. `cut-check` tests failed. `content_md5` was checked as 64 hex digits. It is 32.
3. Clippy rejected unused `is_hex64` in the copied compression file.
4. Clippy rejected `PathBuf`, which only the tests use.
5. The live phases finished backup exit 0, corrupt-dump exit 4, restore exit 0, and restore-existing exit 4. Host `cp` of `saved/dumps/.` failed on macOS before `cut-check`. The checker was run once on that same container and exited 0. The rejected update's psql exit was 3, with `55P03` in stderr. The row did not change during the lock.

## Non-claims

`development_unit` is false. A passing restore is not an all-state certificate, not a production backup drill, and not acceptance of E51.
