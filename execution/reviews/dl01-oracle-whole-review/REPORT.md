# DL01-W1-53 Oracle whole-source review

Verdict: `return`.

The whole Oracle source exit is unmet. Source acceptance stays 0/10. The native P2 record is a separate component observation and does not close that exit.

Reviewer grok-4.7/xhigh, session `100ae8f8-205f-494e-9a43-d1f844cca67e`. Author session `992aa5d3-6848-4dd9-aff1-d4d31a099983`. Frozen content `7788327ef17a3fb8d731d34a1c0f056141e7e885`. Evidence seal `403cb41336610edc6a28a6a1ce4f9301c588451c`, parent of the seal is the content commit. Base archive `577160c22de63adb9b38e0147f3bc2da893c4d9d`. Packet `evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/driver`.

The original exit is the human whole-source section of `execution/batches/dl01-wave1-20261009/DL01-W1-04-oracle-whole-source.md`, read with `execution/PRODUCT-LOOPS.md` DL01, `execution/batches/dl01-wave1-20261009/acceptance-frontier.json`, Plan DBX-W09, and the P2 runtime section of the same card. A customer connection, least-privilege catalog, and typed Arrow batch go through an admitted physical import into an independent Postgres, then a real model and native compiler, a role pack, and a trusted operator query that allows one statement, denies another, and cancels an in-flight statement. After Oracle stops, the local target still answers. Source and binary identities are the fixed commits. Five representative rows can be the Oracle batch. They do not move the ten-source count.

## What this review checked

`git archive` of the seal. `SHA256SUMS` has 2462 entries, 0 missing, 0 mismatched. Its own sha256 is `542c19ba6ef7aeaceb36725a607c1eabce9f8dae9081965067ad9b8a87ae210b`, the same value as the author receipt. `receipt.json` is the seal commit and is outside that inventory. All 45 ELF and rootfs files named in `EXCLUDED.txt` are on disk and match the recorded sha256, including `pg-builder` `38c8777bd92e3ff3e1b4ca0e01b28d2e95bb82f9e9f0941e3106910368c5e341`.

One read-only `psql` on the running target `oo-dl01w1-04-p2-pg`, database `previewdb`. Oracle container `oo-dl01w1-04-p2-oracle` stayed exited. `product.py` was not run. No model call. No volume or secret was recreated. `tools/pg-builder` was not edited.

## Whole product

Fixed-commit reproduction of the sealed builder is unmet. `linux/product-compile-4/sealed-rs.sha256` is `2d87535223c4577ce5cf5c5d30fb8f12449fc181c3fd759cad375f3d1eb79272` for `/src/tools/pg-builder/src/sealed.rs`. The same path at the content commit and at the seal is `5aae6114f986f25dfebdfe3f4e151198cb81671943f8434bac279db48ce15f07`. The compile container's `repo-head.txt` is `29c8379c41e645c7099747523193043ad1ac7262`, and that commit's `sealed.rs` is the `5aae6114` file. The mounted file was outside the frozen commits. The 04 content commit does not touch `tools/pg-builder`. `e4f9131c6c3634e30f5f73b6b3db19ab7aa06f7d` is the later 06 commit of that file: 35 insertions and 1 deletion, the `bind_model` predicate, and test `columns_only_completion_reaches_unknown_cost_and_a_wrong_protocol_does_not`. This review did not compile `e4f9131c6`, so the checked-in binary is not a reproduction of that commit.

The recorded unknown-cost completion is that gap. `out-cost/model-response.bin` is 671 bytes, sha256 `a3e15764232f9aeebf356343aaaf88eb4669d0a56faa8f502c2ef181cdc35102`, and its only key is `columns`. Committed `bind_model` requires two keys and protocol `oo.pg-builder.sealed-decisions/v1`, so it returns `protocol` before the price check. The preserved `failed-attempts/structural-protocol` result from binary `d524c13f5ea82966bb390e1d737ed6bf499410eade9a6ef86dbfc3c4f4cb06f4` is `structural_draft` / `protocol` / pack `not_produced`. The `cost_unknown` result was produced by `38c8777b`. Fresh `out-fresh/model-response.bin` does include the protocol key. Both builder results come from binary `38c8777b`.

The physical import is unmet. `pg-builder import-seal` returned `draft_compiled`, `ready` false, `model` `not_called`, pack `not_produced`. `tools/oracle-preview/product.py` `load_rows` then calls `_oo.full_import_open_run` and `INSERT`s `catalog.GOLD`. The live run is `state=open`, `sealed_at` null, `source_id=oraclebatch`, `input_digest=a6236974c0d71bfc372587e84e39d4dcf93aa994005700157854294abed47aa3`. That digest is sha256 of the literal `oracle-asof-scn-2175138-rows-5`. The native Arrow fingerprint is `cd9f0d7d7ce31bc2a6ddc2ce3b7451f7517f4b452510647e83e12e584f4ddd1d`. The live table `pt_9588b8a13b3b457e9eb5939d77f2b86bb2ba83a94c2d3274a2fe95d86ead` has the five GOLD rows. `DATE` and `RAW` are `text`. `N1` is `int2`. The logical alias is `batch.t_batch`. The Oracle object in the native statements is `OO04_FIX.T_BATCH`.

`confirm_oracle` wrote `ORA-00942: table or view "OO04_FIX"."T_BATCH" does not exist` and `oracle-asof.exit` is 0. The SQL*Plus session is `/ as sysdba` with no `ALTER SESSION SET CONTAINER = FREEPDB1`, and the function does not read the error. The loop continued into the GOLD insert.

`admits_import=false` and `pg_run_complete=false` on the native batch receipt match a worker that has not imported. Nothing in the preview loop admits the import or seals the run. `pg-builder import` on the committed producer accepts only manifest protocol `oo.mysql-preview.manifest/v1`.

The fresh and cache results agree with each other: identity `993c6db60995ecba3826e3cf7a09c026e97b5a020cc14ce7afaf6f96dbfe0508`, pack `93ee129412a7862600401eb9ad96eb164e29dc1d5acc854b7baa9f112d50a4fb`, cache miss then hit, second socket `not_opened`. The fresh prompt does not contain the gold tokens. HTTP 401 and the 1 ms timeout produced no pack. Preview compare and detached compare both say `match`. Cancel proof is backend `active`, outcome `canceled`, sqlstate `57014`, `rollback_confirmed=True`. Those observations describe the seeded table and binary `38c8777b`. `oracle-running-after-stop.txt` is `false`.

## Native P2 component

The frozen P2 raw record is consistent with the P2 contract on the same card. It is not a new execution, and it is not source acceptance.

Batch child inspect: image `sha256:95c37d8742e63f562704c783123a6114caba09a7ea9a9405bfcce4aa6c2f6ccd`, user `65534:65534`, `CapDrop=ALL`, `Privileged=false`, read-only root, 512 MiB, 0.5 CPU, pids 64, network `oo-dl01w1-04-p2-src` only. Mounts are the audit directory and a read-only bind of the reader file at `/run/secrets/source-password`. The child inspect has no Docker socket. The launcher command in `linux/p2-native/run.py` is `--network none` with `/var/run/docker.sock` mounted on the launcher. The passing run did not save a launcher inspect document.

Reader grants are `CREATE SESSION`, `SELECT` and `FLASHBACK` on `T_BATCH`, `T_TS`, and `T_WIDE`, `EXECUTE` on `SYS.DBMS_FLASHBACK`, and `INHERIT PRIVILEGES`. `any_or_logmining` is false. Batch statements are the column catalog, `DBMS_FLASHBACK.GET_SYSTEM_CHANGE_NUMBER`, `COUNT(*)` `AS OF SCN 2175138`, and the five-column `FETCH FIRST 5 ROWS ONLY`. Stdout is `rows=5`, `admits_import=false`, `kernel_network=internal`, fingerprint `cd9f0d7d7ce31bc2a6ddc2ce3b7451f7517f4b452510647e83e12e584f4ddd1d`. Independent decode reports the same schema and `driver: none`. `decode.json` has no cell payload. This review did not decode `batch.arrow` again.

Deadline is a separate exit 3: `CallTimeoutExceeded`, `ready=false`, `rows_committed=0`, `cancel_api=absent`, afterwards `active|0`. Cancel observed `status|ACTIVE` for sid 53 serial 61251 on the marker select, then `status|ABSENT`. The observation SQL has no `KILL`. `cancel_api` stays `absent`. That is worker shutdown after the session was seen active. The product-loop `57014` is a Postgres cancel of the preview backend.

Fence receipt: allowed source connected, forbidden probes false, `import_receipt=false`. Product-container fence: DNS `198.18.2.45`, Oracle bridge `192.168.208.2`, connect timeout. DML is `ORA-41900` and `operational_path=false`. Wide `NUMBER`, `TIMESTAMP(9)`, and time zone stay uncommitted. Insert on a missing `T_SECRET` is `ORA-00942`.

## Left in place

`oo-dl01w1-04-p2-oracle` exited. `oo-dl01w1-04-p2-pg` running. `oo-dl01w1-04-p2-product` exited. Volumes and the reader directory were not removed. No review container was started. Counts, Plan, and the parent grade were not edited.
