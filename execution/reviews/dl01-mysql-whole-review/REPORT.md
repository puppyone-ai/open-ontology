# DL01-W1-54 MySQL whole-product review

Verdict: `approve_child`.

The experimental MySQL source/child boundary is green on the original exits. Source acceptance stays 0/10. Parent SP03-W02, whole DL01, release, and any verified or certified mark stay unclaimed.

Reviewer grok-4.7/xhigh, session `f04f4f43-73c8-4ac7-8f3a-7de2b7d27673`. Author session `b5ac4bc8-1872-4fe4-a56a-04a70f7103a2`. Frozen content `e4f9131c6c3634e30f5f73b6b3db19ab7aa06f7d`. Evidence seal `0abaca9acaf9337690e5888e5c644bf2969dcb4a`. Base archive `577160c22de63adb9b38e0147f3bc2da893c4d9d`. Packet `evidence/b5ac4bc8-1872-4fe4-a56a-04a70f7103a2/DL01/mysql`.

The original exit is the MySQL preview chain on `execution/batches/dl01-wave1-20261009/DL01-W1-06-mysql.md`, the later 100k and whole-product cards through `DL01-W1-06-model-restored-whole-product.md`, the DL01 MySQL row in `execution/PRODUCT-LOOPS.md`, and Plan SP03-W02. A least-privilege TLS read lands in a sealed physical Postgres import. One real builder pack stays unpublished. A trusted operator reads it, refuses a hidden column and a widened policy, and cancels an in-flight statement. The source path that run treats as disconnected stays unreachable, and the target still answers. Identities are one fixed content SHA.

## What this review ran

`native-loop/SHA256SUMS` has 266 entries. `sha256sum -c` exited 0, with 0 mismatches. The inventory file's sha256 is `64bcb349a51a7421d08f59059c1e655b971840e5f4f8e4170a5ae45c31566ad9`.

Read-only SQL on retained `oo-dl01w1-06-mysql` (8.4.10) and `oo-dl01w1-06-pg` (PostgreSQL 17.10, database `previewdb`). No model call. No source DML. No gold write. The retained `operator-preview` binary was executed twice into a review directory, not into the author evidence tree. An `ACCESS EXCLUSIVE` lock on the sealed shipments table covered only that cancel. Afterwards the count was 8000 and no exclusive lock remained.

Binaries match the author receipt: `pg-builder` `d5fd5fd13a121092fbb87d4be4b84bf9bcb36955685dba32e308264e28488991`, `operator-preview` `1a2b0e57c904e8d2c62ca18854d6d458177676261439337eaa139a270312dc31`, `oo-pipeline` `50fd4f991842d093327be0ade3d42e86548f03508457089e9b51da57c9d768c6`. `7e4e42f14438f6d1bdeb84ffd936ed5b7192fd7f`, `131279c54572f612d10945ccb5a17a3a9c638f9e`, `39b2c1e17202a089897b3076ee0d71e194f767e7`, `b1ffb079d7beec4297dbd3deec97219584c6775d`, and `f2357a026033fbac5beb20b4bf8555d808881fe7` are ancestors of `e4f9131c6`. The owned-tree diff from `39b2c1e1` to `e4f9131c` is `tools/pg-builder/src/sealed.rs`, 35 insertions and 1 deletion.

## Sealed import

Live run `b512537d-ea3c-4eda-8bba-3eef0afe52ec`, source `mysqlscale`, source generation 1, target generation 4, state `awaiting_publish`, sealed, not retired. Config `bc299cb7d389f447b3ac1afbbb2cac0a1026f1abdf3061179f9c75e126264bfe`. Schema `3488e2839c8b3481e55ff42de1c7d8b4da8b7a5fa860d0c0b45474e51b35ea3e`. Type `e096918bfc0ab782ad06f9177b784974ce217d88b62366ddd8b9da5dc87d28be`. Input `118fea950b34929ad212b21fcc19c593b7f0e0f7e4f1cde0e4723a1c085f13d3`.

Catalog stdout is 12614 bytes, sha256 `1412bfe3058081d937d42949e4739e97b47a966c4cb8468c8bd648da9e515ac3`. SHA-256 of the `identity_preimage` in `crates/oo-builder/src/sealed_import.rs`, filled from those live fields, is `e6c65a3c25b0eac2c5c8615e94ddec88e158adffdcbbed89fcb00ac23a270af6`.

There is no `logistics` schema. Exhausted physical tables for this run:

| logical | physical | rows |
| --- | --- | --- |
| hubs | `pt_a44f18aae9adde90061583c3f4d0dc3abb5a9fec1c32af7e9928e1ea35fd` | 2000 |
| shipments | `pt_772dd23f61865fe275d0377c4f21848d03f5c28393e4127c575e4a0ae82d` | 8000 |
| tracking_events | `pt_70c27a6c6a950f594988b9848cebcf1b9837ae1f52953e9457f057c8d3d2` | 90000 |
| value_probe | `pt_f7ca6848ff2d5079c5869ac5781d6a7fbafded936451027f2e5aff34cf01` | 6 |
| range_gap | `pt_b08e9e7c7fbd041b8b339b6ce7b19739b2992143ad9906a81ebaa0118281` | 3 |

MySQL `COUNT(*)` as `oo_scale` over TLS matches those five counts. `information_schema.table_rows` is the approximate set 2058, 7978, 89537, 6, 3 and was not used.

Both sides agree on delayed air 667, null cost 470, shipment 1 `9999999999999999999999999999999999.9999`, shipment 2 `-0.0001`, and shipment 7001 `9007199254740993.0001`. Probe row 2 stores unsigned `18446744073709551615` as `numeric(20,0)`. Exact amounts are `numeric(38,4)`. JSON stays `json`. The badge is `bytea`. Probe row 3 has a null note. The zero-date and the out-of-range time are null in the stored probe rows. `range_gap` has three `mysql_datetime_range` rows. Sealed prototype run `de8276fc` has `parcel` 5 and `empty_box` 0. The scale catalog for config `bc299cb7` records `loss_concealed` false, `manifest_reject_count` 2, and `stored_rejected_rows` 2.

Current import seal is `draft_compiled`, compiler `compile_sealed_import_catalog`, dependency `b1ffb079`, model `not_called`, pack `not_produced`. The mismatched stored-batch attempt is `compiler_refused`, code `quality.rejected`, message `a stored batch does not match the sealed table`.

## Builder record

Fresh: `packed`, model `called`, socket `opened`, cache `miss`, attempt 1, `ready` false, publication `not_published`, review `unreviewed`, pack `128aa157f5b36805f67c6c279abe3601ad2e46fb2c150a2a52592f77877d7369`, input `9f268878f84cf985481efa4d14a1cc9b1a51ffdbfb59df4e532b484993ccd02a`. Cache: cache `hit`, socket `not_opened`, attempt 0, same pack, same unpublished review state.

The live and unknown-cost model bodies are the same 4631 bytes, sha256 `62da624cd1bc63852c146089ff387117295892b413183c846e3430847ee50f29`. The only key is `columns`. There are 43 columns. `logistics.shipments.cost_amount` and `logistics.value_probe.exact_amount` are money with unit `XXX`. The other 41 units are null. `effective.json` contains `XXX` twice.

`config-live.json` sha256 `9c217c2fd652e10b1c1b3a8ce5f4978da897476829cd563952fd75b66dc87105` declares USD `0.000001` per total token, status `declared_non_billing`. `config-cost.json` sha256 `000c7d15ab55a44b1114ec862784637227e2227fc44e8e2fcebdb69196a9e3ad` declares price status `unknown`. The cost result is `cost_unknown` / `unknown_cost` / pack `not_produced`. HTTP 401 and the timeout result produce no pack. The input-hash miss result is HTTP 401, pack `not_produced`, recorded input `42d1739f1367e2b54b856359127682b6348e1310e8325bb9a39b532dd16eb3d7`. Its `model-input.json` matches the fresh prompt byte for byte, sha256 `52c3bee14168913493f0d2e015186957cbe94e8f2b3d934ca8f4d6a8a075aac7`. Preserved HTTP 402 and 503 results stay `provider_failure` / `http_status` / pack `not_produced`. This review did not call the model.

## Operator replay

The binary above ran on `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, network `oo-dl01w1-06-pg-net` only, read-only root, `CapDrop=ALL`, 0.5 CPU, 512 MiB, user `0:0`. User 0 was required to read the mode-0600 secret files. The retained volume was not chmodded.

| check | exit | result |
| --- | --- | --- |
| describe | 0 | described |
| count | 0 | 8000 |
| delayed air | 0 | 667 |
| null cost | 0 | 470 |
| shipment 1 | 0 | decimal, 38 nines, scale 4 |
| shipment 2 | 0 | decimal, coefficient 1, negative, scale 4 |
| shipment 7001 | 0 | coefficient `90071992547409930001`, scale 4 |
| hidden comment | 6 | guard |
| widened policy | 9 | pack_mismatch |
| second count | 0 | 8000 |
| late cancel | 10 | not_sent |
| in-flight cancel | 0 | sent, sqlstate `57014`, rollback confirmed |
| canceled query | 0 | canceled, sqlstate `57014`, 0 rows, rollback confirmed |

Successful results bind import `e6c65a3c25b0eac2c5c8615e94ddec88e158adffdcbbed89fcb00ac23a270af6` and snapshot `b512537d-ea3c-4eda-8bba-3eef0afe52ec`. Count and the canceled query both record `source_dialed` false. The count result's pack digest is `128aa157f5b36805f67c6c279abe3601ad2e46fb2c150a2a52592f77877d7369`. Publication stays `not_published`.

The in-flight execution is `abd1c0b3-6d54-4fd7-8b78-aae74fdcc5e2`. The lock was granted, the state file was `running`, and cancel followed about one second later. The author execution `9608392d-49a2-4428-b676-6c6cc2017921` remains the historical record.

## Source path and target login

`oo_scale`@`%` has `USAGE` on `*.*` and `SELECT` on `logistics.*`. `oo_read` and `oo_write` are absent. `require_secure_transport` is 1. This review's business session used `TLS_AES_128_GCM_SHA256` and TLSv1.3. The account `ssl_type` column is blank. For user `oo_scale`, `performance_schema` shows begin, checksum, commit, select, show_grants, show_status, and one `statement/sql/error`. It does not show insert, update, delete, replace, truncate, create_table, alter_table, or drop_table. `general_log` is on. This review did not read that log.

`oo_preview_login` is not superuser. Direct `SELECT` on the sealed shipments table is `permission denied`. After `SET LOCAL ROLE oo_r_reader`, with `is_superuser` off, the same session reads 8000 shipments and 7814 comment values. `has_table_privilege` for that role on that table stays false, and the table ACL names `oo_target_owner` plus `INSERT` for `oo_pipeline_runtime`. The operator audit records role `oo_r_reader`. The operator still refuses logical `comment` with exit 6.

From inside `oo-dl01w1-06-pg`, `127.0.0.1:5432` connects, bridge `192.168.160.2:3306` times out with exit 124, and DNS `198.18.2.127:3306` accepts then returns 0 bytes and no MySQL banner. The source container remains Up. Author `inside.log` keeps an earlier detach failure (`mysql_tcp=0`, `NATIVE_EXIT 1`) and then `mysql_tcp=124 pg_tcp=0 dns_tcp=0`, the consumer queries, the cancel, and `NATIVE_EXIT 0`.

## Left in place

`oo-dl01w1-06-mysql` and `oo-dl01w1-06-pg` are running. Volumes `oo-dl01w1-06-mysql-data`, `oo-dl01w1-06-pg-data`, `oo-dl01w1-06-cargo`, and `oo-dl01w1-06-target` stay. Review containers `oo-dl01w1-54-op` and `oo-dl01w1-54-cancel` were removed. No production path, ledger, Plan, matrix, or author evidence file was edited.
