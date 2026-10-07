# Coordinated cut freeze

Session `eef831aa-dba4-4af2-a78b-541bc9e0a368`. Product base `1d88fe8ad9fa4a113c105e1df4d0903e517fc9a9`. This file is the cut rule for the repair. It was written before any edit under `tools/pg-backup-restore/`. The original E51 fixture names, dump flags, restore flags, and remaining-state list stay as they are. This file does not accept E51, SP14-W04, AT-OPS-06, PITR, cluster roles, pgBackRest, runsc, or a parent goal.

## What is not a cut

`history_insufficient` must not become false because the cut marker and `count(*)` of `shop.entry` were read twice around two sequential dumps and happened to match. A value update that leaves that marker and that count unchanged is invisible to those samples. `pg_current_wal_lsn()` on the control database does not observe an agent-only write. WAL movement still sets `wal_moved` only. It does not set `pitr_ready`. `pitr_ready` stays false.

`quiescent` is not a caller input. The operator file has no `quiescent` key. An operator file that adds one is refused. The manifest bit is the opposite of `history_insufficient` and is written only after the proof below. A manifest that says `quiescent` true without that proof is refused.

## Proof required before history is sufficient

`history_insufficient` may be false only when all of the following are true for one recorded interval that starts before the first gold `COPY` and ends after both `pg_dump` calls:

1. A session on `oo_a356_control` and a session on `oo_a356_agent` hold `SHARE` locks on every user table in that database. `SHARE` blocks `INSERT`, `UPDATE`, and `DELETE`, including from the table owner, and it does not conflict with the `ACCESS SHARE` locks taken by `pg_dump` and `COPY`. The sessions are read-write only so `LOCK` is allowed. They commit, or the process exits, before backup returns. A failed backup must not leave either database read-only.
2. While those locks are held, a writer statement on each database is rejected with `lock_not_available` (`55P03` / lock timeout). The product statements are `UPDATE shop.entry SET amount = amount WHERE entry_id = 3` and `UPDATE oo_key_fixture.negative_decision SET operator_label = operator_label WHERE left_key = 'k-0001' AND right_key = 'k-0002'`. They are probes. They are not the adversarial value change.
3. While those locks are held, a content digest of every sealed gold relation is read from the database before the dumps and again after the dumps. The two digests of each relation are equal. The digest is `md5` of the ordered `md5` of the gold `COPY` projection (`coalesce(column::text, '<NULL>')` joined by `|`). The gold file SHA-256 taken in that same interval is stored on that relation and must match the gold bytes named in the manifest.
4. The marker sample is still `cut-a356-1|100000` and the entry count is still `90000` on both reads. Those samples can withhold a seal. They cannot establish one.

The manifest records `cut_mode` `writers_blocked`, one `writer_blocks` object per database (`application_name`, `lock_mode` `SHARE`, `probe_rejected` true, and the locked relations), and one `relation_digests` object per sealed gold relation (`content_md5`, `database`, `gold_sha256`, `relation`). Dump file hashes stay in `dumps`. `quiescent` is true only in this case.

There is no cross-database PostgreSQL snapshot in this repair. A coordinated snapshot would be an equivalent only if both dumps and the gold files were bound to relation digests captured inside that snapshot. This repair does not claim that equivalent. It uses the writer block above. No caller flag selects either mode.

## When the proof is missing or ambiguous

If the locks are not acquired, or a probe is not rejected, backup exits without a manifest and without a dump, and it releases any session it started.

If dumps were written and the digests differ, the locks are no longer the same sessions, or the marker or entry count moved, the manifest is sealed with `cut_mode` `unproven`, empty `writer_blocks`, empty `relation_digests`, `history_insufficient` true, and `quiescent` false.

Restore of an unproven cut, a missing proof, a digest that does not match the gold file, a dump hash mismatch, or a manifest hash mismatch returns exit 4 before `CREATE DATABASE` and before `pg_restore`. The readiness report then has `replay_ready` false, `offset_published` null, and `history_insufficient` true. Unknown and ambiguous history must not be reported as `history_insufficient` false. No offset is published. Nothing is activated. Source rows are not rewritten by the backup.

`replay_ready` on a completed restore still requires the existing row, checkpoint, key, and post-cut checks. It also requires `cut_proven` true. `cut_proven` is true only for a manifest that passed the proof above. `pitr_ready`, `all_state_certified`, `production_ops`, `production_slo_certified`, `offset_min_used`, and `parent_acceptance` stay false. `w04_gate` stays `open`.

## Live check that is not the product flag

On the private PostgreSQL 17.10 fixture of 100000 typed rows, an independent session attempts `UPDATE shop.entry SET amount = 1.0001 WHERE entry_id = 3` only after both `SHARE` locks are visible, and retries only after backup releases them. That update preserves the marker and the entry count. The lock-window attempt must be rejected. The later attempt must commit. Restore must show the pre-update amount. A dump byte that does not match the manifest must be refused with no restore side effect.

Enum and calendar migrations stay with session a365. This repair does not edit them.
