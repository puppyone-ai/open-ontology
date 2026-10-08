# Findings

## Sequence state left the snapshot

PostgreSQL 17.10 `LOCK TABLE` on a sequence fails with `42809` (`wrong_object_type`). The cut records that sqlstate and does not call `nextval` or `ALTER SEQUENCE`. Both user sequences in this fixture, `shop.cut_seq` and `oo_key_fixture.cut_seq`, came back `held: false` / `42809`. `sequence_fence.held` is false. `cut_mode` is `unproven`. `history_insufficient` is true. `quiescent` is false.

The catalog read inside the cut transaction says `41|f` for both sequences. The harness `nextval` during the window returned 41 and exited 0, and the source state after the backup is `41|t`. `pg_restore -f` of the sealed dumps contains:

```sql
SELECT pg_catalog.setval('shop.cut_seq', 41, true);
SELECT pg_catalog.setval('oo_key_fixture.cut_seq', 41, true);
```

`true` is `is_called`. The dump's sequence state is the later non-MVCC value, not the `41|f` row bound in `catalog.txt`. A snapshot id does not freeze `nextval`. The certificate is refused for that reason. Restore exited 4 with `reason=cut_unproven` before any `oo_a356_restore_*` database existed.

`proven()` is still a conjunction. A cut whose sequence fence is held can seal `writers_blocked`. This server did not grant that fence, so readiness stays off. The product does not hardcode `replay_ready` false, drop sequences from the dump, or pass `--table`.

## What the snapshot did hold

Each database exported its own snapshot after the table `SHARE` locks and before `pg_dump`. The ids differ: `00000011-00000006-1` on control and `00000004-00000008-1` on agent. The wrapper recorded `dbname`, `snapshot`, `deferrable=absent`, and `format=custom` for each. `--serializable-deferrable` was not passed.

`pg_restore -f` text of both dumps has neither `oo_a377_probe` nor `a377-during-cut`. The catalog inventory has the preexisting views and the party comment, and it has no probe objects. Shop gold hashes match the sealed a361 values. Entry 3 in the dump text is `1234567890.1234`.

The amount update issued while `SHARE` was held on `shop.entry` exited 3 with `55P03`. The row stayed `1234567890.1234` until the cut sessions were gone. The same update then committed `1.0001`. Marker `cut-a356-1|100000` and count 90000 did not move. A one-byte corruption of the agent dump was refused with `dump_hash_mismatch`, exit 4, and no restore database.

## Harness misses

Live run 1 started an autocommit `UPDATE` to `1.0001`. It exited 0 with empty stderr and committed. The product's own `amount = amount` probe in that same window was a lock timeout. The committed probe was not a `55P03` result. See `failed-attempts/live-1.txt`.

Live run 2 used an 8 second `lock_timeout`. The cut released in about 4 seconds, the waiting update was granted, and the probe rolled the change back. The product had already sealed an unproven manifest with the same `42809` rows and had already excluded the probe objects. The harness stopped before `cut-check`. See `failed-attempts/live-2.txt`.

The recorded run uses a 1 second `lock_timeout`, which expired while the cut still held `SHARE`. `rejected.err` contains `55P03`. The row did not change during the cut.

Build misses before the green build: `stderr` was not mutable; clippy `manual_is_multiple_of`; the checker dropped `#[test]` on one function; snapshot id constants were unused in the non-test binary. Those are `failed-attempts/build-1` through `build-4`. The following build exited 0.

## Review

Verdict: withhold. E51 stays open. PITR, production SLO, and full SP14 stay open.

The refusal is the measured 17.10 sequence lock result plus a dump that disagrees with the catalog's sequence row. It is not a scope waiver. Table `SHARE`, the two exported snapshots, and the post-cut exclusion were measured on this fixture. `parent_acceptance`, `pitr_ready`, `production_ops`, `production_slo_certified`, `all_state_certified`, and `offset_min_used` are false. `w04_gate` is `open`. `offset_published` is null.

This review is the same session that wrote the change. No second reviewer ran.
