# Findings

Limited approval covers candidate production in `tools/selfbuild-suggestions` at `d22187ea0fce8e420323660841f2da3b447d5de8`. Each clause below names the run that proved it.

## Deterministic graph, negative pairs, weak keys, no automerge

Independent test `graph_metric_and_partial_validation_keep_the_second_example_unrun` produced the same outcome twice. The review fixture (project `review`, phone column on `shop.party`) yielded one entity: primary `shop.party`, domain `commerce`, provenance `declared_fk`, confidence `graph`, `strong_key_status` weak, `weak_columns` `["shop.party.phone"]`, `auto_merge` false, candidate id `cand_3238c70833a4054c7c18dc716e7b54c3`.

Independent test `negative_weak_key_does_not_merge_certify_or_publish` supplied a validated `party_id` profile and the negative pair `shop.party` / `shop.account`. The party member is an isolated suggestion with `strong_key_status` weak. Weak status remains in force when a profile strong key is also present. `auto_merge` is false. The remaining example constraint is `entry_account_fk`. A stale-override queue item is present. `curated_written` is false.

Author test `graph_rules_keep_weak_keys_unmerged` passed in the same focused run. Author test `sealed_shop_catalog_names_party_as_primary_and_refuses_a_tamper` passed.

The live command has no phone column and no negative pair. Its one entity suggestion is primary `shop.party`, provenance `declared_fk`, `auto_merge` false, `strong_key_status` unknown, `weak_columns` empty, candidate id `cand_7cf18e19bea87e5f3599782605e88982`. Repeating produce was not required on the live path. The independent test already compared two produces of one command and required them to be equal.

## Metric template, anchor, owner, control total, no certification

On the review fixture, column `amount` (table has `posted_on`) produced candidate `cand_3cc4bb382160763a05d3d2a16fc20c8d` with `certified` false, `status` draft, `owner_needed` true, `control_total_needed` true, grain `month`, `time_dimension` `posted_on`, `time_anchor` `prev_month_start`, `time_anchor_closed` false, `time_anchor_needed` false, SQL `SELECT sum(amount) FROM shop.account`, `validation` not_applicable, `gateway_admitted` false.

Column `qty` has grain `row`, `time_anchor` null, `time_anchor_needed` true, `owner_needed` true, `control_total_needed` true, `certified` false, SQL `SELECT sum(qty) FROM shop.entry`.

The scripted runner's call list contains the probe and the `party_key` select. It does not contain `sum(`. The live command has no money or quantity column, so its metric array is empty. The sum templates were not sent to the private database.

`effects.metrics_certified` is false on the live outcome and on the closed-effect helper used by the independent probes.

## Feedback bounded and untrusted

`duplicate_relationship_and_feedback_collapse_once` collapsed one duplicate feedback item (`feedback_collapsed` 1, `scope_drops.feedback` 1) and kept one feedback candidate.

`negative_weak_key_does_not_merge_certify_or_publish` kept the text `untrusted-note-a364` on one feedback candidate with `untrusted` true and `trusted_metric` false. That text is absent from metric SQL, example SQL, and example questions. The live command has zero feedback items, so its feedback array is empty and `scope_drops.feedback` is 0.

## No grants, no curated write, no publish

Live outcome effects:

- `auto_merge` false
- `curated_written` false
- `grants_issued` false
- `metrics_certified` false
- `model_called` false
- `published` false
- `roles_changed` false
- `samples_retained` false
- `source_queried` false
- `knowledge_verified_count` 0

Author test `public_fixture_is_deterministic_and_does_not_publish` passed. The independent scope test `scope_refuses_other_model_scope_and_missing_authorization` emits no entity suggestions. `seal_mismatch_and_candidate_preimage_are_independent` emits no metric suggestions and checks the canonical hash `ecf9e98ec0641e23113ff3ce8bdc78d0ddd249886517fd4a7f68cc83d4e65667` for `{"a":1,"b":"x"}`.

The database role grants inside `pg/apply.sh` are the fixture's own shadow setup. They are not `effects.grants_issued`. That effect is false on the producer outcome.

## SQL candidates: real gateway versus not_run

Without a gateway config, the independent graph test reports `gateway.configured` false, `probe` not_run, `spawned` 0, and both examples `validation` not_run.

With a config whose probe does not exit 6, the same test refuses with reason `probe`, admits nothing, and leaves both examples `not_run`.

With a scripted probe exit 6 and a scripted select exit 0, the test admits the first example only. The scripted row value is `scripted-not-a-receipt`. That run is not a query-pg receipt.

The real path is `live/capture/`:

- Probe SQL `UPDATE shop.account SET label = label`, exit 6, empty stdout, stderr `query-pg: guard`, no audit file. Exit 6 writes no audit. That matches the captured absence of `request-1.audit.jsonl`.
- Select SQL is the full `LIMIT 1` statement. Exit 0. One text row. Summary and audit both succeeded with `returned_rows` 1.
- `original_sha256` `8918a40c1d36449a8fc5b4ee64581e584c35442ef2b09dcc7580c77d57cd7e4c` equals the checker hash of `gold/select.sql`.
- `executed_sha256` `d7f0d5ba3a0ba3c1feb5f1a8b0e33f0c2a7dba4cec2c5227e512260b640a0cb9` is the same value on the summary and on both audit lines. It is a different hash from `original_sha256`. The executed statement text is not in the receipt.
- Second example `SELECT account_id FROM shop.entry WHERE account_id IS NOT NULL LIMIT 1` stayed `not_run`. No third spawn.

`knowledge_verified` is false on the admitted example. The outcome string does not contain `"knowledge_verified":true`.

The shadow is one physical mapping, table `pt_f695fb869ab51c7d4e8f9f0acaa5f6059200adc25ce411094b5f06c471ce`, one inserted text value. Apply output records `mapped_rows=1` and `mapped_value=a364-mapped-row`. The 1000000 figure in the request is `large_table_row_threshold` on the catalog policy. It is not a row count. This run did not load 100k rows.

Author test `gateway_admission_follows_a_refused_probe_and_never_verifies` passed. The ignored producer test was not run. The live capture is the compiled-binary proof of the probe-then-one-select path.

## Observations that stay inside the limited approval

ProcessRunner clears the child environment, sets `PATH=/usr/bin:/bin`, and also sets `HOME=/tmp`. The contract text describes the cleared environment and PATH. The extra `HOME` assignment did not change the captured exits or the receipt.

The producer temp directory prefix on the request `audit_file` is `oo-a355-sbs-`. The producer deletes that directory on drop. The wrapper had already copied the audit JSONL to `live/capture/request-2.audit.jsonl`. The saved file is the binary's audit. The prefix is a private-path detail of this producer, and the copy is why the receipt survived.

Pins used by the producer crate, unchanged: `serde` `=1.0.229` with derive, `serde_json` `=1.0.151`, `sha2` `=0.10.9`. `build/Cargo.lock.frozen` matches `tools/selfbuild-suggestions/Cargo.lock` (SHA-256 `f29a2691f8e214a389c7221b2040c48a5558eef6d1f10e72bca21dd217864724`).

## Withheld

This packet does not accept the SP13 parent, an authentication design, certification, publication, or promotion of any suggestion into an accepted S6 entity. It does not accept the parent plan or the goal. `knowledge_verified_count` remains 0.
