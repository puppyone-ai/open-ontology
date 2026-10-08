# Findings

## Sequence exclusion that holds nextval

PostgreSQL 17.10 `lockcmds.c` allows `LOCK TABLE` only for relkind `r`, `p`, and `v`. Sequences return `42809`. Packet `d86fea93c4e537b77fd8bd4a47ca91f3a4746785` measured that and refused the certificate. That packet remains the ancestor of this work. A permanent refusal is not the close of this card.

`ALTER SEQUENCE ... OWNED BY` is the only sequence option that does not set `need_seq_rewrite` (`src/backend/commands/sequence.c`, `init_params`). It opens the sequence with `ShareRowExclusiveLock`. That mode conflicts with the `RowExclusiveLock` taken by `nextval` and `setval` inside `init_sequence`, and it does not conflict with the `AccessShareLock` used by `pg_dump` to read `last_value`. `CACHE` and `INCREMENT`, even when the parameter is unchanged, rewrite the relfilenode and zero `log_cnt`. They are not the fence. `RESTART`, `nextval`, and `setval` change values and are not the proof.

Owned-fixture measurements on this PostgreSQL 17.10, saved under `seq-lock-probe/`:

- `OWNED BY NONE` and a restated `OWNED BY shop.entry.id` left `last_value`, `is_called`, `log_cnt`, and `relfilenode` unchanged.
- Concurrent `nextval` and `setval` waited and then failed with lock timeout. Verbose psql reported `55P03`.
- A backend that had already received one cached value from `CACHE 10` issued a second `nextval` while another session held `OWNED BY NONE`. The second call returned `55P03` and the on-disk row stayed `10|t|32`.
- `pg_dump --snapshot` during the lock exited 0 and wrote the locked `setval` lines.
- `ALTER SEQUENCE ... CACHE` and `INCREMENT BY` with the current parameter changed the relfilenode. Those statements were rolled back.
- `USAGE` and `UPDATE` on the sequence, plus `MAINTAIN` on the table, let a stranger `LOCK TABLE` and did not let that stranger `ALTER SEQUENCE`. The error was `must be owner of sequence`.
- `SET ROLE oo_a377_operator` after the objects were owned by `oo_a377_owner` ran `LOCK TABLE` and `ALTER SEQUENCE ... OWNED BY NONE` with `rolsuper` false. `GRANT` of the superuser role to the operator was rejected by PostgreSQL and is not used.

The product assigns `oo_a377_owner` at fixture load, grants that role to `oo_a377_operator`, and in the cut transaction runs `SET ROLE oo_a377_operator`, table `SHARE`, then a restatement of `OWNED BY`. The cut compares `last_value|is_called|log_cnt` around that statement. A change, or any sqlstate other than `00000`, leaves the fence unheld. `ALTER SEQUENCE ... OWNER TO` is not inside the cut transaction, because that path took `AccessExclusiveLock` in the privilege probe and would block `pg_dump`.

## Same-run 100k restore

One private cluster, after the probe container and its data volume were removed, loaded the 100k fixture and ran backup, the corrupt-dump restore, and the good restore.

The manifest records `sequence_fence.held` true, lock mode `ShareRowExclusiveLock`, method `alter_sequence_owned_by`, and sqlstate `00000` for `shop.cut_seq` and `oo_key_fixture.cut_seq`. `cut_mode` is `writers_blocked`. `history_insufficient` is false. `quiescent` is true.

Harness probes, started once the table `SHARE` locks were visible:

- `nextval` on both preexisting sequences, exit 3, stderr `55P03`.
- `setval(..., 99, true)` on both, exit 3, stderr `55P03`.
- Amount update while `SHARE` was held, exit 3, `55P03`, amount `1234567890.1234`.
- Amount update after the cut sessions ended, exit 0, amount `1.0001`.

Sequence state stayed `41|f` on the source after the cut. `pg_restore -f` contains `setval('shop.cut_seq', 41, false)` and `setval('oo_key_fixture.cut_seq', 41, false)`. The catalog rows bound in the manifest are `41|f|0`. The two snapshot ids differ: control `00000005-00000008-1`, agent `0000000B-00000008-1`.

The good restore exited 0. Restored entry 3 is `1234567890.1234`. Restored sequences are `41|f`. Restored views `oo_dict.enums` and `oo_dict.calendar` are views and their comments are not `a377-during-cut`. Schema `oo_a377_probe` is absent from the restore. `post_cut_absent_on_restore` is true and `post_cut_present_on_source` is true. Shop file hashes match the sealed a361 values. A one-byte corruption of the agent dump was refused with `dump_hash_mismatch` before either restore database existed (`dbs-before-corrupt` and `dbs-after-corrupt` are `none`).

`replay_ready` and `cut_proven` are true for this logical restore. `parent_acceptance`, `pitr_ready`, `production_slo_certified`, `all_state_certified`, `production_ops`, and `offset_min_used` are false. `w04_gate` is `open`. `offset_published` is `100000` because the cut was proven and the stored watermark is `100000`. `rpo_window_seconds` is 4 and `rto_seconds` is 1 because those fields are the elapsed seconds of this run. They are not the docs/11 control or agent SLO.

The catalog sha256 equals the unproven packet's catalog sha256, `8819c6e48c6595d8a5e1cebac12baa2576d1502bb35dda9b43f291b6f7373bcc`. The inventory is the same frozen rows, including sequence `41|f|0`. The dump hashes differ because this dump's `setval` third argument is `false`.

The first password scan of the evidence tree matched only `runtime/secrets/admin.pass`, which is gitignored and exists because this host cannot bind-mount `/tmp`. The scan now skips directories named `secrets`. A second scan, and the private-key scan with the same exclusion, matched nothing. `live/password-leak-files.txt` and `live/private-key-files.txt` are `none`.

## Fixture limit

The loaded schemas and the two `cut_seq` sequences have no identity column. PostgreSQL rejects `ALTER SEQUENCE ... OWNED BY` on an identity sequence with `cannot change ownership of identity sequence`. A user sequence that returns a sqlstate other than `00000` keeps `sequence_fence.held` false, and restore then exits 4 with `cut_unproven` before `CREATE DATABASE`. This run's two sequences both returned `00000`.

## Review

Verdict: the logical cut and its restore are recorded. E51 stays open. PITR, production SLO, and full SP14 stay open.

The same run held table `SHARE` and sequence `ShareRowExclusiveLock`, dumped both databases with explicit snapshot ids, kept preexisting sequence state at `41|f`, excluded the during-cut objects, and restored sequences, views, catalog rows, and the 100k shop gold. Concurrent `nextval` and `setval` failed with `55P03`. The corrupt dump was refused before any restore database existed. The proof is the lock mode plus the snapshot ids plus the inventory plus the dump `setval` lines plus the restored rows. A before/after fingerprint alone is not the proof.

`gold.rs`, `Cargo.toml`, `Cargo.lock`, `protocol.rs`, and `decide.rs` were not edited. `pg-wrap.sh` remains the a274 file. No root manifest change, no cargo update, no 101-check rerun, no second builder, no enterprise connection, no push.
