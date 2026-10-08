# Catalog cut repair

Session `eef831aa-dba4-4af2-a78b-541bc9e0a377`. This packet repairs the original E51 logical backup after the a372 withhold of `20f065096fa9e20414eed8951d830333160e4af6`. The cut now exports one REPEATABLE READ snapshot per database, passes that id to `pg_dump --snapshot`, and binds the catalog inventory to the manifest. PostgreSQL 17.10 still rejects `LOCK TABLE` on a sequence. The measured cut is therefore unproven, restore is refused, and E51 stays open.

## What ran

Pre-edit smoke and the focused build used Linux arm64 Rust 1.94.0 inside `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. The private server was PostgreSQL 17.10 from `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Cargo was not updated. The root manifest was not edited. `gold.rs` was not edited. The 101-check campaign was not rerun (`oracle-101-rerun.txt` is 0).

Focused results, all exit 0: product tests (5), product clippy, cut-check tests (4), cut-check clippy, release build. Oracle unit tests in the same build: 14 passed.

The live fixture is the existing 100k logical backup on `oo_a356_control` and `oo_a356_agent` only. During the held SHARE window the harness commented `oo_dict.enums` and `oo_dict.calendar`, created schema `oo_a377_probe`, table `added`, inserted 7, created sequence `seq`, called `nextval` on that new sequence, and created view `v`. It also called `nextval` on the preexisting sequences `shop.cut_seq` and `oo_key_fixture.cut_seq`.

## Measured cut

| Check | Result |
| --- | --- |
| Backup | exit 0, manifest sealed as `unproven` |
| Writer locks seen | agent 1, control 1 |
| Value `UPDATE` during the cut | exit 3, `55P03`, amount stayed `1234567890.1234` |
| Value `UPDATE` after release | exit 0, source amount `1.0001` |
| Marker and entry count | `cut-a356-1\|100000`, 90000 before and after |
| Preexisting `nextval` | exit 0, state `41\|f` then `41\|t` |
| Snapshot ids | control `00000011-00000006-1`, agent `00000004-00000008-1`, both `REPEATABLE READ` |
| `pg_dump` flags | `deferrable=absent`, `format=custom`, one id per database |
| Sequence fence | `held` false, both rows `42809` |
| Catalog sequence rows | `41\|f` |
| Dump sequence state | `setval(..., 41, true)` on both sequences |
| Post-cut names in `pg_restore -f` text | absent |
| Shop gold | party `62230764…fa5b`, account `fef89f25…a958`, entry `a1136957…0450` |
| Corrupt dump restore | exit 4, reason `dump_hash_mismatch`, restore databases `none` |
| Good-copy restore | exit 4, reason `cut_unproven`, restore databases `none` |
| Second restore | exit 4 |
| `cut-check` | exit 0 |
| `replay_ready`, `cut_proven`, `pitr_ready`, `parent_acceptance` | false |
| `offset_published` | null |
| `w04_gate` | open |

`cut-check` accepts this refused shape. It does not turn the refusal into a certificate.

## Limits

E51, PITR, the pgBackRest RPO/RTO numbers, production SLO, and full SP14 are not accepted. `rpo_window_seconds` and `rto_seconds` are null. `wal_moved` is false and the control LSN stayed `0/2B4B588`. Readiness has no `development_unit` field; this packet does not set one. The same session wrote the code and the review.
