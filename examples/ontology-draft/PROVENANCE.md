# Ontology-draft fixtures

These files are synthetic catalog evidence. They were written by the oo-builder tests from in-memory tables. They are not a live PostgreSQL import, not a dump, and not an accepted `oo-connectors` result. The catalog shape follows the frozen `oo.source.pgsql/v1` contract at `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`. `pg_catalog_integration` stays `not_accepted`.

`oo-builder` tests load every file listed in `MANIFEST.sha256`, compile it, and compare the bytes. Set `OO_WRITE_DRAFT_GOLDENS=1` only to regenerate after an intentional compiler change. A normal test fails when a file drifts.

`shop-reordered.input.json` lists the same shop tables, columns, and foreign keys in reverse order. Its compiled bytes equal `goldens/shop.draft.json`. The reordered draft is not stored twice.

The document that is one byte over the 1048576-byte scanner limit is built in the test as spaces. It is not checked in. Depth 40 is `failures/depth.input.json` and fails `json.budget.depth` because the limit is 32.

## Independent hashes

The empty schema preimage `{"tables":[]}` has SHA-256 `cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc`. The one-column `public.customer.id` `int4` preimage used by the customer fixture has SHA-256 `60f7939195cc539644d11132ae4a604c3b025167f1fe657d121167feb8463499`. Both constants are asserted against the compiled drafts.

The customer comment is not stored. Its evidence record is `byte_len` 49 and SHA-256 `e240cabd1273526d2f85f1757c8b1970c94782c0e8963a2c6d14aaddb16ae18e`. The 49-byte sentence exists only in the oo-builder test source so the test can prove the draft contains the digest and does not contain the sentence. Policy SQL is the same kind of record: `byte_len` 46 and SHA-256 `4d9f0a5b01f7258ca3f1e2d30e49cc6e749c9c2ec4a6ca053011740af28c561e`. The shop input keeps a synthetic owner name, a synthetic grantee name, and the enum label `publish_all_rows` because those fields are part of the catalog document the hash covers. No draft file contains them.

`failures/unknown-field.input.json` adds a comment `text` field. The compiler returns `field.unknown` and the error text does not include that field.

## What each golden shows

| Fixture | Check |
|---|---|
| `empty` | Unpublished draft. Schema hash is the empty preimage. Queue is empty. |
| `customer` | One primary key, unknown PII, `data_type` `int4`, publication unavailable. |
| `shop` | Composite primary key order `tenant_id`, `id`. Quoted Unicode table `客户` with ident `q_e5aea2e688b7`. `bill_to_fkey` and `ship_to_fkey` stay separate, `declared_fk`, and `fk_ambiguous`. Wide `numeric(80,10)`, domain `zip`, and enum `mood` omit `data_type`. |
| `shop-reordered` | Same shop draft bytes. |
| `reject` | Fresh rejection drops the effective column and primary key, keeps the generated column, and keeps curated `pii` and `label`. |
| `stale-reject` | A changed schema hash returns the column to pending and records `stale_decision`. The rejected fact is not applied. |
| `accept` | The structural item is accepted. PII and semantic items stay pending. Publication stays unavailable. |
| `curated` | Override applies `Customers` and `Identifier`. `pii` and `entity` stay in the curated echo. |
| `comment-cleared` | Clearing a comment keeps `schema_hash` and changes `evidence_hash`. |
| `underscore-schema` | `_audit` stays physical. No OOS dataset. `identifier_not_in_oos`. |
| `fk-not-selected` | No relationship. `fk_target_not_selected`. |

Failure codes are the full `oo.builder.catalog-evidence/v1/...` strings in `failures/*.code`: `hash.schema`, `version.unsupported`, `json.duplicate_key`, `field.unknown`, `adapter.unsupported`, `review.not_in_scope`, `budget.name_bytes`, `budget.key_columns`, `json.budget.depth`, `selection.mismatch`, `ident.collision`, `duplicate.selection`.

## Draft file digests

| File | SHA-256 |
|---|---|
| `goldens/empty.draft.json` | `47e31be7ac19a1fa2cdd862be4dfa21de3520de23e7693ad779ccd175f69add1` |
| `goldens/customer.draft.json` | `486a22960ec2c759455bf26c1d970e44dc486a5e7225a38ec4d6c08b876cb106` |
| `goldens/shop.draft.json` | `a7afbcc681086049687571a0a46f4592c227e30fdf837ffd8177e5a87505fa72` |
| `goldens/reject.draft.json` | `b78970ba6760c7b9322db13836cd6968a23f22e6a4c958877917e8065dd22889` |
| `goldens/stale-reject.draft.json` | `165e8bbf51172424f31a4a2b0915fb247b1d5e0a12bdca96c725e3ceb9d0f207` |
| `goldens/accept.draft.json` | `847f05eebe459b37cd11e6357b538a7af387d4a9f88e2b42b4ae580dbdd97324` |
| `goldens/curated.draft.json` | `255407e0a8ce9af92315594615e174482f3296263d6e330a3240031e9427a4ea` |

The full file list, including inputs and failure codes, is `MANIFEST.sha256`.
