# Findings

Verdict: API slice completed, with one compiler seam left open.

Fixed source `0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1`. Session `eef831aa-dba4-4af2-a78b-541bc9e0a203`. Oracle `live/oracle.json`. Outcome `live/outcome.json`. Raw phase exits are under `linux/phases/` and `linux/harness-phases/`.

## F1. Role pack has no accepted input on this snapshot

`compile_role_pack` was called three times. Each result has `compiled: false`.

Input `structure_draft_json` is the reviewed structure draft (`protocol` `oo.builder.structure-draft/v1`). The compiler returned `oo.builder.role-pack/v1/schema.rejected` at path `/`, message `the effective document does not match the role-pack contract`.

Input `compiler_datasets_and_relationships` and input `compiler_datasets_only` returned `oo.builder.role-pack/v1/classification.unknown_pii` at `/datasets/0/columns/0`, message `unknown PII stays hidden and is not classified by this compiler`.

The reviewed draft leaves every column `pii` null and `classification` null. Accepting structural facts does not fill those fields. There is no public compiler in this fixed snapshot from `oo.builder.structure-draft/v1` to `oo.builder.effective-oos/v1`. The harness recorded the three rejections and continued. It did not insert PII values and retry the pack compiler.

Owned path for a later task: a builder compiler that emits `oo.builder.effective-oos/v1` after a reviewed contract sets PII and classification. This task did not add that code.

## F2. Column ACL is an administrative bridge

`live/column-acl.json` records `bridge`: `trusted administrative classification; ontology compiler left pii and classification unset`.

`install_column_acl` returned `Installed` for `oo_r_party_read` and `oo_r_invoice_read`. The party grant lists four physical columns and omits `internal_note` (`pc_fdaa80edd422b1fb9a56247bef27675893dba342a4a3e2d1f5221405e23b`). The invoice grant lists five physical columns and omits `clerk_note` (`pc_3c0366d8d7cd26f7bec820839589a1d774d212fbee3772614d32059f1454`). Semantic types used by that bridge are the PostgreSQL type names `int8`, `text`, and `bool`. They are not fields from the ontology compiler.

`load_policy_catalog` accepted each compiled policy alone. Both views use policy id `3033ccada1eef250b5f550f8cdfde039b3e9897e7b3e0489779f3adec3490859`, project `a203ent`, `estimated_rows` 0, and `pending_unclassified` 2.

## F3. Guard refusal and database 42501 are different checks

The executor path for a hidden logical column returned `ExecError::Guard`. Outcome text for `SELECT internal_note FROM ent.party` is `column is not visible: ent.party.party_id, ent.party.display_name, ent.party.active, ent.party.credit_limit; 2 columns are pending classification`. The invoice hidden column uses the same form. Both `INSERT` statements returned `write statement is not allowed`. Those four results are class `guard_refusal`. They do not carry SQLSTATE 42501.

The harness also opened the source reader and the two target logins and required SQLSTATE 42501 from the database: reader `INSERT INTO ent.party`, then `SET ROLE oo_r_party_read` / `SET ROLE oo_r_invoice_read` followed by `SELECT` of the hidden physical column and `INSERT` into the physical table. `run.exit` 0 means each of those statements returned 42501 and the source party count stayed 3. The SQLSTATE string is not stored in `outcome.json`.

## F4. The import seal is awaiting_publish

`install_calls` is 1 and the effect is `bootstrapped`. `full_import_calls` is 1 and the effect is `installed`. Run `7dba3fda-8dba-4cbd-83a6-a7ac0382bda0` has state `awaiting_publish`. The harness read `count(*)` from `pg_catalog.pg_publication` and required `0`. `provenance.publication` on the reviewed draft is `unavailable`. `provenance.pg_catalog_integration` is `not_accepted`. `shadow_marked_published` is false. `parent_acceptance` is false.

## F5. One review item can take one action

Dataset curated labels applied: `交易方` / `企业主体名录` and `发票` / `发票单据名录`. `label_attempt` is `chinese_labels_accepted`.

The relationship item `relationship/ent.invoice/invoice_party_fk` was accepted as `candidate_relationship`. A curated override of that same item id was not submitted beside the accept. `review_note` records `one_review_per_item_id blocks override beside accept:relationship/ent.invoice/invoice_party_fk`. The queue still has one `curated_unbound` item in state `pending`. The relationship status on the effective draft is `draft`.

## F6. Captured comments are digests

`live/catalog.json` stores table and column comments as `byte_len` and `sha256`. Party table `76245388c7bd896c3d40c874157b2dd28802d70c904a224777cef089d5e4aace` (18 bytes). Invoice table `6e9da0a86dd9152be0fb6944efd6bce1ea8b4effb0e8ab250e1329e00de92a9b` (12 bytes). The strings `交易方主数据`, `可空中文名称`, and `不对主体角色公开` are absent from the catalog file. `catalog_contains_comment_body` is false. The Chinese text in `oracle.json` is source cell values, and the Chinese text in the draft is the curated dataset label and description.

## F7. The harness lock dropped unused registry packages

Resolve mode on the successful run is `standalone`. `linux/lock-registry-added.txt` is empty, so the harness lock added no registry name/version/checksum. `linux/lock-registry-removed.txt` has 56 triples that were in the copied frozen root lock and are absent after resolving the harness package. `lock-diff.exit` is 0. `cargo update` was not run. The root `Cargo.lock` in the product tree was not read or written by this task.

## F8. Query CLI stays outside this slice

`outcome.json` field `query_cli` is `not_exercised`. Permission queries used `oo_gateway::query_executor` (`DedicatedSession::execute_trusted_select`) against the fixed archive. The a198 binary was not read.
