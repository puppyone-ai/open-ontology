# Minimal loop acceptance card

External runner. It does not accept the parent goal, production runsc, arbitrary databases, or the full plan. `http_release_acceptance` stays false.

The only acceptance entry is:

```text
minimal-loop-acceptance card --repo <worktree> --archive <git-archive> --out <dir> --pinned-commit <sha>
```

Add `--binary`, `--manifest`, `--config`, and `--execute` only for a binary whose manifest says `built_from` is `git-archive` and whose sha256 matches that manifest. Optional `--pi-commit`, `--pi-executable`, and `--pi-runtime` count as the published Pi pin only when the commit is `7c10bd4337495ee613f2224843ecdf349b80d1df` and both file hashes equal the config. Optional `--inventory` is the selected product file inventory. The required trees are `tools/minimal-product-loop`, `deploy/minimal-product-loop`, and `tools/pi-harness`. The card compares that inventory to the immutable archive. A file present under a required tree must be listed. A dirty live worktree does not fail the archive. `moving_source_attested` stays false. `tools/agent-entry` is optional packing. Listing it and matching the archive bytes keeps the inventory match, and `a397_committed` records whether that tree is non-empty. Absence of the tree does not fail the archive. `agent_entry_runtime` fails the archive. An HTTP release claim fails the archive. A missing binary stays unapproved.

`MP-01` passes only when `MP-01a`, `MP-01b`, `MP-01c`, and `MP-01d` all pass from measurements. Row counts alone do not pass. The whole `mp01_signed` bit on one import is not the copy-capability review and is not a prerequisite for measuring `MP-01d` on a selected campaign. `--copy-review` counts when the file is `oo.acceptance.local-copy-verdict/v1` with `mp01abc_signed` true, `customer_complete` false, `consistency_claim` `single_snapshot`, `business_peak` 1, `producer_source_socket_fact` `repeatable_read_read_only`, `runner_source_sockets` 0, cards `MP-01a` through `MP-01c` with `checks_pass` true, and typed rows `batch_a` / `batch_b` / `batch_c` at 2000 / 8000 / 90000 with `hash_equal` and `key_coverage`. The sibling `case.json` is `oo.acceptance.local-copy/v1`, `provenance` `live`, `customer_complete` false, and a producer record whose `binary_sha256` is 64 hex digits, with `binary_present`, `invoked`, `exit_code` 0, `source_socket_peak` 1, `source_socket_fact` `repeatable_read_read_only`, and `fixed_single_connection` true. A verdict `provenance` other than `live` rejects the review. The review's import hash is not compared with the campaign. `mp01_signed` may stay false. `copy_review_signed` stays false.

`--fixture-probe` records an operator fixture and does not set `copy_performed`. `--sealed-manifest` with `--import-manifest` recognizes a secret-free `oo.pipeline.local-pg-mirror-target/v1` file whose bytes match the manifest `target_config_sha256`, plus a complete single-snapshot bulk manifest for `public.batch_a` / `batch_b` / `batch_c` at 2000 / 8000 / 90000. `customer_complete` must be false. That pair is the selected campaign seal and its provenance. It does not set `copy_performed`. The campaign import hash is the hash in that manifest. A fresh campaign keeps its own hash. `--product-note` records `kit_consumed=true`, a 40-hex commit, and that same import hash when a manifest is present. It is a measurement, not a predecessor that blocks the campaign. `--binding-probe` records a reader measurement of the selected target. When it matches the manifest generation, snapshot, import hash, row counts, types, samples, partial denial `42501`, and `runner_source_connects=0`, `MP-01d` can see the typed binding. The probe does not set `typed_reconciliation` or `copy_performed`, and it does not pass `MP-01` by itself.

`copy_performed` is true only when `run` stdout or `state.json` contains `phases` or `copy_phases` with a discover begin and a copy begin, both `from` `source_connection`, before a later copy complete whose `failure` is `none`, `mirror_state` is `complete`, `customer_complete` is false, `snapshot_id` is non-empty, and `import_hash` is 64 hex digits. After `run`, that hash and snapshot must equal the sealed manifest re-read for the same campaign, and the hash must differ from the manifest that was already complete before `run`. An event whose `failure` or `event` is `replay_complete` is skipped. A producer line that only contains `replay_complete` does not count. A complete event without the two begins, without a snapshot, or whose snapshot does not match the re-read manifest does not count. A producer-direct copy is not this user-CLI phase pair. When the manifest was already complete and the run does not perform a new copy, `target_was_preloaded` becomes true.

`MP-01d`, once ask and close have run, also requires the capability review, the sealed campaign manifest, campaign provenance, the typed binding, a 64-hex campaign import hash, source unreachable after seal, the source secret withheld, the local chain on the copy, source access after seal 0, query audits bound to one import generation and naming the local target, and close reporting that the source, the persisted copy, and the database were kept. A command that did not run stays `pending` while the capability review is absent. With the review present, the pending reason is that the selected campaign was not executed. Exit 0 only when `archive`, `MP-01a` through `MP-01d`, and `MP-01` through `MP-06` are all `pass`.

`fixture` writes the public 2000/8000/90000 COPY files. `kit` hashes the mounted a406 runtime. Neither command is an MP pass.

Exit codes:

| Code | Meaning |
| --- | --- |
| 0 | Every measured check passed on the fixed archive |
| 1 | A measured check failed |
| 2 | Usage |
| 4 | A command was pending, not run, or a stream broke |

`parent_acceptance`, `goal_acceptance`, `runsc_certified`, `arbitrary_database`, and `full_plan` stay false.

Frozen questions are the sealed bulk import: `public.batch_c` count, id overlap of `public.batch_a` and `public.batch_c` with no foreign key, conditional amount sum, null amount count, one exact numeric, and the two int8 boundaries. Prompts must not contain SQL or those gold numbers. `MP-04` passes a question only when the final answer, from the top-level `answer` or the latest `turn_finished.answer`, contains the gold token and the query id of a query the answer cites. That cited query must have audit HTTP 200 and a database name. Its cells must equal the gold cells. For `id-overlap`, a cited join may instead carry the count 2000 in a wider row or list 2000 rows. Status `matched`, HTTP 200 alone, and an uncited first query do not pass. Gold is not sent to Pi. The shop fixture questions stay in `shop_questions` and are not this set. An operator approval file is not a Pi tool. Write, bad-model, and timeout results must be non-zero failures. `draft_ready` on a fault is still a false success.

`MP-02` accepts `generation_outcome` `validated`, or a `run` stdout whose `status` is `draft_ready` and whose `self_build` is `generated_unreviewed`. A later close status of `released` does not clear that run measurement. Close must still revoke the mapping and keep the database.

The development acknowledgement is `local-process-not-a-sandbox-certificate`.
