# Catalog cut repair

Session `eef831aa-dba4-4af2-a78b-541bc9e0a377`. This packet continues the original E51 logical backup after the a372 withhold of `20f065096fa9e20414eed8951d830333160e4af6` and after packet `d86fea93c4e537b77fd8bd4a47ca91f3a4746785`, which recorded `LOCK TABLE` on sequences as `42809` and refused the certificate. That refusal is kept under `prior-unproven-cut/` and `linux/build-prior-unproven/`. It is not this delivery.

The cut still exports one REPEATABLE READ snapshot per database and passes that id to `pg_dump --snapshot`. Table `SHARE` locks stay. Sequence exclusion is `ALTER SEQUENCE ... OWNED BY` restated to the current owner, which takes `ShareRowExclusiveLock` and leaves `last_value`, `is_called`, `log_cnt`, and `relfilenode` unchanged. The cut role is `oo_a377_operator`, a `NOSUPERUSER` member of `oo_a377_owner`.

## What ran

Pre-edit smoke stayed the earlier Linux arm64 Rust 1.94.0 run, inside exit 0. One later builder, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, ran focused tests and clippy. Cargo was not updated. The root manifest was not edited. `gold.rs` blob `2564210b226be3d6980265a1dc429cfadf2a613f` is unchanged. The 101-check campaign was not rerun.

Focused results, all exit 0: product tests (5 lib, 14 oracle), product clippy, cut-check tests (4), cut-check clippy, release build. The builder container was removed by `run-build.sh`.

The live fixture is the existing 100k logical backup on `oo_a356_control` and `oo_a356_agent` only. PostgreSQL 17.10 from `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. During the held window the harness commented `oo_dict.enums` and `oo_dict.calendar`, created schema `oo_a377_probe`, table `added`, inserted 7, created sequence `seq`, called `nextval` on that new sequence, and created view `v`. Separate sessions called `nextval` and `setval` on the preexisting sequences `shop.cut_seq` and `oo_key_fixture.cut_seq`.

## Measured cut

| Check | Result |
| --- | --- |
| Backup | exit 0, `cut_mode` `writers_blocked` |
| Writer locks seen | agent 1, control 1, mode `SHARE` |
| Value `UPDATE` during the cut | exit 3, `55P03`, amount stayed `1234567890.1234` |
| Value `UPDATE` after release | exit 0, source amount `1.0001` |
| Marker and entry count | `cut-a356-1\|100000`, 90000 before and after |
| Preexisting `nextval` and `setval` | four probes exit 3, each stderr contains `55P03` |
| Sequence state | `41\|f` before, after, and on both restored databases |
| Snapshot ids | control `00000005-00000008-1`, agent `0000000B-00000008-1`, both `REPEATABLE READ` |
| `pg_dump` flags | `deferrable=absent`, `format=custom`, one id per database |
| Sequence fence | `held` true, `ShareRowExclusiveLock`, `alter_sequence_owned_by`, sqlstate `00000` |
| Catalog sequence rows | `41\|f\|0` |
| Dump sequence state | `setval(..., 41, false)` on both sequences |
| Restored views | `oo_dict.enums` and `oo_dict.calendar` present, during-cut comment absent, probe schema count 0 |
| Post-cut names in `pg_restore -f` text | absent |
| Shop gold | party `62230764…fa5b`, account `fef89f25…a958`, entry `a1136957…0450` |
| Corrupt dump restore | exit 4, reason `dump_hash_mismatch`, restore databases `none` |
| Good-copy restore | exit 0, amount `1234567890.1234` |
| Second restore of the sealed tree | exit 4 |
| `cut-check` | exit 0 |
| `replay_ready`, `cut_proven` | true |
| `pitr_ready`, `parent_acceptance`, `production_slo_certified`, `all_state_certified`, `production_ops`, `offset_min_used` | false |
| `offset_published` | `100000` |
| `w04_gate` | open |

Backup outer span was 4 seconds. Restore outer span was 1 second. Those are the tool fields `rpo_window_seconds` and `rto_seconds` for this run. `production_slo_certified` is false.

## Limits

E51, PITR, the docs/11 pgBackRest RPO/RTO numbers, production SLO, and full SP14 stay open. `wal_moved` is false and the control LSN stayed `0/2B5D358`. Readiness has no `development_unit` field. The same session wrote the code and the review.
