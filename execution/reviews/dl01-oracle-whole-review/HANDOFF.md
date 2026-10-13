# Handoff

Verdict `return`. Whole Oracle source exit unmet. Native P2 stays a component record. Source acceptance 0/10. This review did not edit `tools/pg-builder`, the 04 sources, ledgers, Plan, or the running databases.

## Sole 06 demand

Writer: DL01-W1-06. Path: `tools/pg-builder/src/sealed.rs`, function `bind_model`. The frozen commits contain sha256 `5aae6114f986f25dfebdfe3f4e151198cb81671943f8434bac279db48ce15f07`, which rejects the input below. Commit `e4f9131c6c3634e30f5f73b6b3db19ab7aa06f7d` already stores file sha256 `2d87535223c4577ce5cf5c5d30fb8f12449fc181c3fd759cad375f3d1eb79272` and test `columns_only_completion_reaches_unknown_cost_and_a_wrong_protocol_does_not`. That commit's parent is the Oracle seal. The diff against the seal is 35 insertions and 1 deletion in this file. No further producer edit is required for this return. This review did not compile `e4f9131c6`. Binary `38c8777bd92e3ff3e1b4ca0e01b28d2e95bb82f9e9f0941e3106910368c5e341` stays the compile-mount binary.

Input to accept: one JSON object, only key `columns`, value an array of objects with exactly `dataset`, `column`, `pii`, `semantic_type`, `unit`. Length and order match the draft columns. Recorded body, sha256 `a3e15764232f9aeebf356343aaaf88eb4669d0a56faa8f502c2ef181cdc35102`, 671 bytes:

```json
{"columns":[{"dataset":"batch.t_batch","column":"id","pii":"none","semantic_type":"identifier","unit":null},{"dataset":"batch.t_batch","column":"n16","pii":"none","semantic_type":"quantity","unit":null},{"dataset":"batch.t_batch","column":"n1","pii":"none","semantic_type":"quantity","unit":null},{"dataset":"batch.t_batch","column":"d","pii":"none","semantic_type":"free_text","unit":null},{"dataset":"batch.t_batch","column":"c6","pii":"none","semantic_type":"code","unit":null},{"dataset":"batch.t_batch","column":"v2","pii":"none","semantic_type":"free_text","unit":null},{"dataset":"batch.t_batch","column":"r","pii":"none","semantic_type":"free_text","unit":null}]}
```

With an unknown price, that body ends `cost_unknown` / `unknown_cost` / pack `not_produced` / `ready` false after one model call. If `protocol` is present it must be `oo.pg-builder.sealed-decisions/v1` and the object must have exactly those two keys. Protocol `oo.other/v1` stays `structural_draft` / `protocol` / pack `not_produced`. Any extra key stays `protocol`.

`tools/pg-builder/src/import.rs` is unchanged by this demand. Its `run_import` still accepts only manifest protocol `oo.mysql-preview.manifest/v1`.

## Same owner, remaining phase

Owner stays DL01-W1-04, session `992aa5d3-6848-4dd9-aff1-d4d31a099983`. The producer text is `e4f9131c6c3634e30f5f73b6b3db19ab7aa06f7d`. Rebuild `pg-builder` from that commit. Point the preview at that binary.

Replace the `catalog.GOLD` insert with an admitted physical import of the native Arrow batch: fingerprint `cd9f0d7d7ce31bc2a6ddc2ce3b7451f7517f4b452510647e83e12e584f4ddd1d`, SCN `2175138`, five rows, schema `ID:Int16`, `N16:Decimal128(16,6)`, `N1:Int8`, `D:Timestamp(Microsecond,None)`, `C6:Utf8`, `V2:Utf8`, `R:Binary`. The worker receipt keeps `admits_import` false and `pg_run_complete` false. The importer's own receipt is the one that has to seal `_oo.full_import_run`. The live run is `state=open`, `sealed_at` null, on `oo-dl01w1-04-p2-pg` database `previewdb`.

`confirm_oracle` has to fail the run when the PDB read returns `ORA-00942`. Use `FREEPDB1`. The current check is `sqlplus / as sysdba` in the root container and ignores the error.

`input_digest` has to be the Arrow batch. The current value `a6236974c0d71bfc372587e84e39d4dcf93aa994005700157854294abed47aa3` is sha256 of the literal `oracle-asof-scn-2175138-rows-5`. Logical alias `batch.t_batch` and `source_id=oraclebatch` are authored labels. The Oracle object is `OO04_FIX.T_BATCH`.

Do not call `pg-builder import` with the Oracle catalog. Do not run `product.py` main against this database. A clean rerun drops `previewdb` once `schema-applied.txt` is removed. Do not recreate volume `oo-dl01w1-04-p2-oradata` or the reader secret. Do not start a model call until the fixed builder and the admitted import are the objects under test. Re-prove fresh, cache, unknown cost, 401, timeout, allowed query, denied query, `57014` cancel, and the detached query on the imported rows. Five rows stay the Oracle batch. They do not move source acceptance.

## Resources left with the author

`oo-dl01w1-04-p2-oracle` exited. `oo-dl01w1-04-p2-pg` running. `oo-dl01w1-04-p2-product` exited. Networks `oo-dl01w1-04-net`, `oo-dl01w1-04-p2-deny`, `oo-dl01w1-04-p2-src`. Volumes `oo-dl01w1-04-p2-oradata`, `oo-dl01w1-04-p2-pgdata`, `oo-dl01w1-04-cargo`, `oo-dl01w1-04-target`. Secrets stay under `/Users/redux/.oo-dl01w1-04-p2/` and `/tmp/oo-customer-demo-20261007/llm-token`. This review started no container and removed nothing.

## Evidence

Author packet `evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/driver` at seal `403cb41336610edc6a28a6a1ce4f9301c588451c`. Review packet `evidence/100ae8f8-205f-494e-9a43-d1f844cca67e/DL01/oracle-whole-review`. Checks: `raw/sha-check.txt`, `raw/pg-identity.txt`, `raw/producer-demand.txt`.
