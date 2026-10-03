# Findings

Verdict: APPROVE LIMITED for the mapping primitive at `b5415544c9eb8d608eafd411884627dda9ee7c84`.

`_oo.register_physical_mapping` recomputes SHA-256 in SQL and refuses a different digest with `OO022`. Python `hashlib`, the Rust plan's golden constants exercised by the package tests, and `_oo.pm_sha256` agreed on the empty string, `abc`, 200 bytes of `a`, and the golden namespace payload `d528fe7ee77dde914147ad1edc2b2d6f2f42b25106ccfe6f39d5f8996f1fc398`. The independent identity `src_review` / `ef`*32 / generation 4 also agreed between `hashlib` and SQL, both through `pm_payload` and through the raw payload bytes. Lookup does not hash. On the probe database, `strpos(prosrc, 'pm_sha256')` for `lookup_physical_mapping` is 0. Register's source contains `pm_sha256` at offset 2586. Both functions are security definer, owner `oo_target_owner`, `search_path=pg_catalog, _oo`. `pm_sha256` is security invoker.

The stored identifier is 63 bytes: `pn_`, `pt_`, or `pc_`, plus 60 hex characters. The live long pair and the probe long pair each share a 63-byte logical prefix and received two different physical names. `UNIQUE (identity_digest)` and `UNIQUE (physical_name)` are present on `_oo.physical_mappings`, as is `octet_length(physical_name) = 63`. The probe stored row for the 82-byte logical name is that 63-byte physical name and the full 32-byte digest.

This module does not normalize. The live test passed finished `pgident/v1` strings. Unicode logical length 482 remained one row. Normalization, reserved words, and quoted identifiers stayed distinct in that test. A quoted `SELECT 1 AS "pn_..."` parsed on the probe database.

Replay of the probe identity returned `replay` and the same `registered_at`. The live test recorded the same for the golden row, plus rollback `OO024`, conflict `OO022` with the original row kept, and concurrent `inserted+replay` with one row. The caller owns the transaction. The probe's owner digest insert was rolled back and the `review_lie` count was 0.

`oo_a140_runtime` may execute lookup. `oo_a140_agent` may not, and neither role may select the table or execute `pm_sha256`. That is the six-value `linux/fixed/privileges.txt`: false, false, false, true, false, false. The probe adds register: `oo_a142_runtime` may execute register and lookup; `oo_a142_agent` may execute neither; runtime may not execute `pm_sha256`; public may not execute lookup. Agent select, lookup, and register, and runtime select, `pm_sha256`, `set_config`, and `SET` plus `INSERT`, all returned `42501` on the probe database.

The writer trigger closes `UPDATE`, `DELETE`, and `TRUNCATE`, and closes `INSERT` unless `_oo.physical_mapping_writer` is `register`. Those four owner statements returned `OO014`. The trigger does not compare `identity_digest` to `pm_sha256(pm_payload(...))`. That comparison is in `register_physical_mapping`. The table owner, with the GUC set, inserted a check-valid row whose digest was 32 bytes of `0x11`. The probe rolled that transaction back. Runtime has no insert privilege, so this is not a runtime registration path. A table constraint that calls the hash function is not part of this approval.

The receipt fence stayed closed. Unfenced `commit_apply_receipt` returned `OO011`. `apply_receipts` stayed 0. Public user tables stayed 0. The migration does not create a user table.

The probe applied `0001`, `0003`, and `0004` on a second database after the cluster-wide roles already existed, and recorded both migration markers. Reading the same `0004` checksum left `applied_at` and the table oid unchanged. A different checksum raised `OO001` and left the row. Updating `schema_migrations` returned `OO014`. The live test's own database installed `0000`, `0001`, and `0004` only. Neither path is a full import.

Restart proof is `docker restart` of this task's Postgres, then a new `psql` session. Lookup of the golden row and of the probe row matched the stored names and digests. `restart-compare.txt` says `match=1`. A crash kill was not run.

`cases.log` appends. The green sequence is the second block and ends with `live=ok`. Attempt 1 consoles are `linux/fixed/attempt-1` (package compile 101: the archive omitted `contracts/pipeline/target-fence-receipts`). Attempt 2 consoles are `linux/fixed/attempt-2` (probe exit 1 on `BEGIN`/`COMMIT` command tags). The author evidence tree was not hash-inventoried.

`sqlx-postgres` emitted two unfulfilled lint expectations. Clippy exited 0. Those sources were not edited. `0000` through `0003` were not edited.
