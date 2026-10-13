# Findings

## Whole source return

1. The sealed builder binary is not a product of the frozen commits. Content `7788327ef17a3fb8d731d34a1c0f056141e7e885` and seal `403cb41336610edc6a28a6a1ce4f9301c588451c` both have `tools/pg-builder/src/sealed.rs` sha256 `5aae6114f986f25dfebdfe3f4e151198cb81671943f8434bac279db48ce15f07`. The file compiled in `linux/product-compile-4` is sha256 `2d87535223c4577ce5cf5c5d30fb8f12449fc181c3fd759cad375f3d1eb79272`. That hash is absent from the frozen commits. `e4f9131c6c3634e30f5f73b6b3db19ab7aa06f7d` records the same file and test `columns_only_completion_reaches_unknown_cost_and_a_wrong_protocol_does_not`. This review did not rebuild `pg-builder` from that commit. Binary `38c8777bd92e3ff3e1b4ca0e01b28d2e95bb82f9e9f0941e3106910368c5e341` matches `EXCLUDED.txt` and is the compile-mount binary. Compile `repo-head.txt` `29c8379c41e645c7099747523193043ad1ac7262` contains the `5aae6114` file. The 04 commits do not include `tools/pg-builder`.

2. Committed `bind_model` rejects the recorded unknown-cost completion before it can return `cost_unknown`. `out-cost/model-response.bin` sha256 `a3e15764232f9aeebf356343aaaf88eb4669d0a56faa8f502c2ef181cdc35102` is a 671-byte object whose only key is `columns`, with seven objects. The preserved structural-protocol attempt against binary `d524c13f5ea82966bb390e1d737ed6bf499410eade9a6ef86dbfc3c4f4cb06f4` is `structural_draft`, reason `protocol`, pack `not_produced`. The predicate recorded in `e4f9131c6` accepts that object and rejects a different protocol. The exact demand is `raw/producer-demand.txt`.

3. The preview rows are an authored insert. `load_rows` in `tools/oracle-preview/product.py` opens `_oo.full_import_run` and inserts `catalog.GOLD`. Live `previewdb` still has that run at `state=open`, `sealed_at` null, five rows in `pt_9588b8a13b3b457e9eb5939d77f2b86bb2ba83a94c2d3274a2fe95d86ead`, alias `batch.t_batch`, engine label `oracle`, `source_id=oraclebatch`. `input_digest` `a6236974c0d71bfc372587e84e39d4dcf93aa994005700157854294abed47aa3` is sha256 of the text `oracle-asof-scn-2175138-rows-5`. The native batch fingerprint is `cd9f0d7d7ce31bc2a6ddc2ce3b7451f7517f4b452510647e83e12e584f4ddd1d`. `DATE` is `text`. `RAW` is `text`. `N1` is `int2`.

4. `confirm_oracle` did not read the PDB. `oracle-asof.stdout` is `ORA-00942` for `OO04_FIX.T_BATCH`. `oracle-asof.exit` is 0. The session is `sqlplus / as sysdba` without `SET CONTAINER=FREEPDB1`, and the orchestrator does not fail on that error.

5. `import-seal` outcome `draft_compiled` has `ready` false and `model` `not_called`. Native batch receipt keeps `admits_import` false, `pg_run_complete` false, `published` false, `rows` 5, SCN `2175138`. The open import run is the same pair of flags at the target. Committed `run_import` accepts only `oo.mysql-preview.manifest/v1`.

6. Fresh/cache identity `993c6db60995ecba3826e3cf7a09c026e97b5a020cc14ce7afaf6f96dbfe0508` and pack `93ee129412a7862600401eb9ad96eb164e29dc1d5acc854b7baa9f112d50a4fb` match, and the second call leaves the socket closed. Unknown price, HTTP 401, and the 1 ms timeout produce no pack. Preview and detached compares say `match`. Cancel is sqlstate `57014` with `rollback_confirmed=True`. The fresh model input, sha256 `6b2bd7a117b7f06315018baec98e35eaae259094cb4c07c5e245a05562de96e7`, does not contain the gold tokens. These results belong to the seeded table and binary `38c8777b`.

7. Five rows are the executed Oracle batch. They do not change source acceptance 0/10.

## Native P2, separate from the source exit

8. The batch child ran as `65534:65534` on network `oo-dl01w1-04-p2-src` only, `CapDrop=ALL`, `Privileged=false`, read-only root, 512 MiB, 0.5 CPU, pids 64, image `sha256:95c37d8742e63f562704c783123a6114caba09a7ea9a9405bfcce4aa6c2f6ccd`. The socket is on the launcher command in `run.py` (`--network none`, `/var/run/docker.sock`). The child inspect does not mount it. A passing-run launcher inspect file is not in the packet.

9. Grants are `CREATE SESSION`, object `SELECT` and `FLASHBACK` on the three proof tables, `EXECUTE` on `SYS.DBMS_FLASHBACK`, and `INHERIT PRIVILEGES`. `any_or_logmining` is false. Batch SQL is catalog, SCN, count, and `AS OF SCN 2175138` `FETCH FIRST 5 ROWS ONLY`. Decode schema is `ID:Int16`, `N16:Decimal128(16,6)`, `N1:Int8`, `D:Timestamp(Microsecond,None)`, `C6:Utf8`, `V2:Utf8`, `R:Binary`, `driver: none`.

10. Caller cancel is process shutdown. `cancel-before.out` is `status|ACTIVE` for sid 53 serial 61251 and marker `oo-dl01w1-04-p2-cancel`. `cancel-after.out` is `status|ABSENT`. The SQL has no `KILL`. `cancel_api` is `absent`. Deadline is exit 3, `CallTimeoutExceeded`, `ready` false, `rows_committed` 0, then `active|0`. Product-network deny is DNS `198.18.2.45` and a timeout to bridge `192.168.208.2`. Fence `import_receipt` is false. DML is `ORA-41900`. Lossy `NUMBER`, `TIMESTAMP(9)`, and time zone stay uncommitted. `T_SECRET` denial is `ORA-00942` because the object is absent.

11. This review did not start Oracle and did not re-decode `batch.arrow`. The P2 raw record stands as the component evidence. It is not the admitted import, the role-pack journey, or a ten-source acceptance.

## Inventory

12. Author `SHA256SUMS` matches the seal tree on 2462 of 2462 files. All 45 excluded ELF and rootfs hashes match the files on disk. Failed attempts remain in the inventory. Password files are outside the git tree, under `/Users/redux/.oo-dl01w1-04-p2/` and `/tmp/oo-customer-demo-20261007/llm-token`.
