# Findings

Verdict: **WITHHOLD**. The consumer at `a13b1cc80ed66c07199a41d47b31fd42530f2be9` does not retain every approved overlay in one combined role pack.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a357`. Decisive Linux evidence is `linux/proof-4`. The oracle is `overlay/a357_consumer.rs`, content SHA-256 `b29bbd5e85ed7006c02dab912bb65b7c6a092f71cfc412a16d0aa51cbb1512af`. Six tests passed in 8.73s. The test that records the gap is `separate_packs_drop_the_other_tables_approved_text`. The sealed-receipt twin is `stored_three_table_receipts_keep_hashes_and_split_the_pack`. E47 is not accepted. `parent_acceptance` is false. No count changes.

## Combined pack gap

`description-pack` reseals `command.artifact`, the sealed draft embedded in that generation, and applies that call's approvals. A curated overlay for another dataset is not compiled into the role file.

Independent fixture, three tables, one `generate` with three echoed calls. Separate packs from the original curated map:

- The shelf narrow role contains `UNREVIEWED: SHELF_ID_BODY_a357` and omits `BIN_ID_BODY_a357`.
- The bin narrow role contains `UNREVIEWED: BIN_ID_BODY_a357` and omits `SHELF_ID_BODY_a357`.
- The ledger pack applies no entry or memo prose. `field_candidates_applied` is false. The shelf pack has `field_candidates_applied` true, `model_publication` false, `authenticated` false, and `parent_auth` `required`.

`review_only` on the shelf generation records `SHELF_ID_BODY_a357` on `review.datasets[]` for `public.shelf`. Copying that document onto the curated map keeps the body in the map. `pack_pair` of the bin generation against that map still renders the bin body and omits the shelf body. `pack_pair` on the three-table aggregate returns three table rows. No narrow role text contains both bodies.

Sealed a328 ready pass-1 calls, re-reviewed and re-packed by this oracle against the sealed curated map, operator `local-operator`, `authenticated` false:

- Party rebind is `bound`, `model_bytes` `copied`, `stored_generation` `unchanged`, stored prompt `oo.builder.description-prompt/v1`, rebound prompt `oo.builder.description-prompt/v2`, rebound prompt SHA-256 equal to `prompt_sha256()`, producer `c23db28dc27f5cf20741dd36e8a8e88129ad1715`.
- Party narrow role contains `UNREVIEWED: Required text key, distinct on every row.` The account narrow role omits that sentence. The party narrow role omits `Unique numeric account identifier.`
- `national_id` and its stored description are absent from the party narrow role, the party restricted role, and the account narrow role.
- Restricted party text omits `Text display name, absent on some rows.` and `Required text in the phone column, distinct on every row.`
- Entry prose `Int8 entry identifier; statistics show one distinct value per row.` is absent from both entry roles. Entry `field_candidates_applied` is false.
- Party effective `party_key` description equals the stored sentence and `review` is null. Settings that list only `dataset_descriptions` omit the party-key sentence from the role file.
- Party pack statements: `model_publication` false, `field_candidates_applied` true, after-document `grants` `not_authorized`, `publication` `unavailable`.

There is no shipped combined final role pack. Closing E47 requires a product that keeps every approved overlay in that final product, or an amended original requirement. This review does not make that product change.

## Clauses the same oracle held

`rebind_copies_descriptive_bytes_and_replaces_prompt_echoes`. A stored v1 `complete_command` stays v1. `candidate_sha256` is unchanged. The rebound prompt is v2 and its hash is the compiler `prompt_sha256()`. Descriptive strings equal the fixture literals. `rebound_request_sha256` differs from the stored `table_outcome.request_sha256`. Columns are `copied` true and the stored card hash differs from the rebound card hash. Producer is `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. The rebound is `prepare_description_request` plus `complete_description_candidates` on a copy. It is not an LLM claim. The stored generation file, captured model body, and cache file stay in place. An unbound rebind does not call `apply_description_review`. Pack of an unbound or non-validated generation writes nothing.

`cache_hit_opens_no_socket_and_stale_scopes_miss`. The second `generate` records `calls=0`, socket `not_opened`, the same candidate hash, and usage total 54 across three tables, with declared amount null. The reconstructed `oo.pg-builder.cache-key/v2` digest equals `input_sha256` and the cache filename. Mutations of prompt, model, origin, evidence, card, and request miss. The real key has no `field_seam`. A stale `evidence_hash` misses. Origin port 9 misses. The consumer prompt constant contains the data-not-instruction sentence. The shelf injection is in the posted comments payload and is absent from `PROMPT`.

`forged_echo_and_failure_leave_curated_and_cache_unchanged`. A wrong `prompt_sha256` is not stored. Review and pack return errors. Timeout and unavailable generations make `review_only` and `pack_pair` return errors, leave the curated canonical bytes unchanged, and create no `pg-builder` cache directory. Profile `most_common_vals` fails with a code containing `profile.values`. `authenticated: true` fails with a code containing `approval.authentication`. A bearer filename equal to the token bytes fails with `config.secret` and opens no call.

`unreviewed_mark_unknown_column_and_deferred_prose`. `confidence_effect` is `recorded_only`. `authenticated` is false. The queue has pending `badge` / `caveats`, pending `badge` / `open_questions`, and `national_mark` reason `deferred`. The caveat and question strings are on the badge decision record and are absent from the queue items. With `column_descriptions`, the new shelf body appears only as `UNREVIEWED:`. Without `column_descriptions`, the new body and the pre-seeded `CURATED_APPROVED_SHELF_a357` are absent from the role file. Effective `review` is null, so the pre-seeded approved curated object does not publish the new body as an approved description. `national_mark` and the injection string are absent from the role text. The restricted role omits `badge`. Listing `national_mark` without a classification is `classification.missing` before pack. Deferred ledger prose is absent and `field_candidates_applied` is false.

`stored_three_table_receipts_keep_hashes_and_split_the_pack` also re-reads the sealed timeout, unavailable, and pass-2 party documents. Timeout and unavailable reviews and packs error. Pass-2 party is `structural_draft`, table outcome `format_rejected`, refusal `evidence_identity`, `cache_store` `not_stored`, and review and pack error. Curated canonical bytes and the curated file hash stay `c7e75f0cdf6dc5c7c8634e1dd9bb34797479e035d6e31330623ecb85b8ad6a23`. The party generation bytes stay at their pinned SHA-256 after the refused review.

Author `tests/contract.rs` on the same archive, before the oracle file existed: 12 passed, including the cache, secret, unclassified-column, and field-text cases named in `linux/proof-4/test-builder.log`. pg-fixture: 2 passed.

## Live completion

One host call, `live-one/summary.json`. Protocol `oo.pg-builder.generation/v1`, outcome `validated`, dataset `shop.account`, model `grok-4.7`, origin `http://127.0.0.1:8317`, HTTP 200, socket `opened`, attempt 1, finish `stop`. Usage 2473 / 3054 / 5527. Declared cost status `unknown`, amount null, invoice false, billing_certificate false. `input_sha256`, `request_sha256`, and `request_input_sha256` match the sealed ready pass-1 account batch. `candidate_sha256` `1daee66e41275eb71d0787f678f03b4b523412a15cac622bc5c175050c93d141` is a new body. The cache object is stored at that input hash. This file was not submitted to review or pack. The packed bodies in this review are the sealed pass-1 receipts and the fixture literals.

Fetch, offline fetch, build, approve, and generate exits are 0. `generate.log` is empty. The bearer file stayed at 6 bytes and was not written into this directory.

## What remains open

The combined overlay pack. Full SP07 management RBAC. Public authentication. Semantic publication. Release. Parent acceptance. A fresh 100k database in this UUID. A cache-hit replay of the new live file. Review and pack of the new live candidate. Numeric oid 1700. Any product repair of the pack gap.
