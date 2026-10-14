# OpenSearch connection, 2026-10-14

Goal sign-off is open. `recipe_completed` and registry `whole: true` are the shared minimum gate for run `4458418024444808b4bf3731b53b6cb7`. They do not certify a running C5 loop, publication, or a source commit clock.

## Minimum gate already on the run

Journal state is `recipe_completed`. Attempt 5 exited 0 and was observed running both `tools/source-onboarding/product_flow.py` and `tools/agent-product-verifier/dialogue.py`. `whole.json` SHA-256 `4a5eb3700ed5faf4b3f2d2c7689e0dd79e8b43d17bf895c048d8a4e194389f03` has `full_product_chain_complete` true, `published` false, and `source_dml_repeated` false. The opensearch registry row still points at that proof. Business publication stays unsigned.

Build `b_e9653e459c33efde8b5ddd8cacd44365`. Product receipt SHA-256 `5e18916db9a0408fe01db4a04ad66c864c6eaa31f16cc4ff3ea8127dcd669c95`. Protocol `oo.source-onboarding.product/v1`, fixture review true, source fresh, S0–S10 succeeded. Native maintenance revision `1f6c03ec479afa9646f8ff2bc70f0c118fe6279e6e6f8c9c3e180263657f26b9`. S5 on that build is the original validated file: mtime `1791924707.3198347`, inode `105451789`, SHA-256 `fff022a24ebb76d0be24b560ebbf4efd95c7dab1590dfd00d1e1a618f8d5ce87`, `cache_hit` false, `llm_calls` 1, model `gpt-6.1-sol`, evidence hash `27d0b4ecded49f3153c04de83a24f1b7718923ad307f13a392fa043e6a6f5a1d`. This resume did not create a model.

Connection capture was already on the run (`captures.json`). Arrow SHA-256 `bc7a18d90b521bd71d25ea05e853819e7a74af0e8f071b7c8af2d78ad8d7bed5` is the same for generation 5 and generation 6. Reader mutations on both captures are 0. Recovery verdict SHA-256 `e8d770ccec374aab2a6a4eaa542a2160bf4c86e75d134216f861af771141e106`: verified, `source_write_attempts` 0, watermark before 5, recovered generation 6. Lease read after the successful step: source `oo_dl01w1_44`, generation 6, `awaiting_publish`, applied position 0, codec `snapshot-zero/v1`. The generation-6 CAS was already committed and was not replayed.

Mutation audit classes read from the retained cycle files are `dba_index`, `dba_update`, and `dba_delete`.

Dialogue verdict SHA-256 `ab692f86053987e57913bca8db88c39276e5511d50474a11c42c886d3b09758a`: outcome verified, 3 turns, model `grok-4.7`, reasoning effort low, tools include query, freshness, describe, and knowledge_cat. Query ids `q_0gQUv4JyvYu6kF3JfbFM6oOTBe`, `q_KgVpCPAJZNJOlU7e5fAhHTpvEY`, `q_y5yjvdoLQAMG7dPW6LdD3D9toP`. Transcript SHA-256 `f58e4c04a8cab6c52360a1e54dfc51d4923407795a1f8533bd2c8ecf6e0ed7bd`. The successful step rechecked that transcript through `dialogue.py` and made no new model call. All three ids are present in the gateway WAL.

Live cancel verdict SHA-256 `11ef79ef081e1c4f473f6fa5f29f298d9eca127f09efcf4ed676a8a3c94a5fae`: outcome sent, sqlstate 57014, rollback true, query `q_JjCRmU3gOeVVrRxFkL39YtYryr`. That id is in the WAL. The step reused this verdict.

Public OO CLI select of the 14 approved columns returned 5 rows. Comparison with the independent generation-5 source decoder covered 70 cells and 0 differences. Generation 6 is bound. The verdict is `gold-verdict-g6.json` SHA-256 `237cf173ffdd767b518d3589482367d592829c8bb3fe7fc9be5030d08d313cb6`, query `q_TKWIuPfnqJVxr68i1lQIAtqRxj`. The generation-5 file `gold-verdict-v2.json` SHA-256 `67cce4eb508648d78d7104f713d2e0c046a6d8fa6632b12fcd87e017c270b031` is unchanged. The freshness warning remains `STALE_DATA` because the watermark is still `awaiting_publish`.

Pins on the completed recipe: `whole.py` `a50e5fa2d905cc571b95a5199ba7677042b4c902c78a67ea93ba3d3532fd2a42`, recipe `1e50d79629a113b7a94220879e033f3893d2e7f0ebc02ff26526722bf33c8615`, registry at resume `51308e7ae6a032d4085f35f1648d4fe056e3a67fb6723d6aeef43342b84f59f9`. HEAD `whole.py` before this commit was `db834d1aa2e7244ebc77ba4cf0ea5f29e89a2ad34d7f9e8642fa9f14a5e3487a`.

Linux `rustc 1.94.0` in container `oo-dl01w1-74-rust` ran `cargo test --locked` for this crate. Result: 6 library tests passed, 0 binary tests, 0 doc tests. Host `python3.14 -m unittest test_whole.py test_connect.py`: 6 tests passed. Fixed ELF SHA-256 values in use: API `025aceded6cc922c0a10cb2e9d535ccc433ad6f082e5c1c3b86f4797cafc50ae` (source `d34813ee44ec558460ee571ef9f37f1ecdd1e026`), gateway `e253d0c0ed6283288418cb9a1566ba7b9d1f7e8011c9b0a78a24727fa80cf5dc`, public CLI `fb8073fd6e01822b6d42ded74ae3673cf202e778b54bdca0ca044c909bb56703`, recovery loader `26855c12b869388b1a9a4590fe6f9521ae7f62685817428078bf51be40e67c18`. No new image and no new long-lived container.

## Ordinary six-field entry

`~/.cache/oo-opensearch-connection-whole/connection.json` mode 0600, SHA-256 `243c58fd8b4a2d57d802bf9c5554e5542205e525a509ec3440255af0198437f9`. Keys are only `type`, `database`, `host`, `port`, `user`, `secret_ref`. The file has no `recipe` and no `out`. Journal identity `input_sha256` equals that digest. Journal `engine_registry_sha256` at resume was `51308e7ae6a032d4085f35f1648d4fe056e3a67fb6723d6aeef43342b84f59f9`. The run pointer `connection.json.run.json` records `out` `.../runs/4458418024444808b4bf3731b53b6cb7` and the same input digest. The completing command this session was:

`/opt/homebrew/opt/python@3.14/bin/python3.14 tools/source-onboarding/onboard.py --input ~/.cache/oo-opensearch-connection-whole/connection.json --resume`

That resume is not repeated. The current `engines.json` digest is `358c6c7502923ee9769d006f9906a917b2f0fd8cab943eefca4fe74ea7d037f5` because later unrelated engine registration changed the global file. The opensearch row is still `whole: true` with proof `4a5eb370…`. Shared onboard resume drift from that global hash is owned by root (`d28ca4bd`). This source does not edit `onboard.py`.

## Live lease and source-after snapshot

Live `native-state` for binding `79fc4bb0-5676-41d7-8f53-b345cbb94e55`: generation 6, `load_state` `awaiting_publish`, `applied` true, `applied_pos` 0, `enabled` false, `period_ms` 1000, method `C5`, cycle `reconcile`, codec `snapshot-zero/v1`, lineage `opensearch.c5.cycle`, task `71f9b4f6-4f76-4e92-acb4-c60d155b24ea`. No owned worker process is running.

One new read-only `connect.py --cycles 1` wrote `/tmp/oo-opensearch-connection-whole/source-after-current-20261014`. Arrow SHA-256 matches generation 6 (`bc7a18d9…`). `reader_mutations` 0, live count 5, tombstones empty, `fullchain_executed` false. The local cycle report's generation field is 1 because that command does not call the loader. `source_write_attempts` 0 remains the recovery fact for a read-only capture. It does not show a worker still pulling.

Fixture schedule `testing/dl01-driver-preflight/opensearch/fixtures/c5-schedule.json` declares `capture_method` C5, `consistency` B, `delete_detection` reconcile. The binding period is 1000 ms. The schedule row is disabled, so the period is configured and not executing.

## Open exit

A new loader generation needs a new physical table. `pg_default_acl` has no table default SELECT for `oo_r_ledger`. This turn did not GRANT. The per-table ACL compiler is a root/PG fix; its output is not reused here. Publication stays `not_ready`. Gateway freshness on the generation-6 gold query is `STALE_DATA`. There is no source commit clock to promote that warning. OpenSearch deletion on this fixture is reconcile, consistency B, `snapshot_token` false. It is not log CDC. S5 was not called again. Generation-6 CAS, cancel, and dialogue were not replayed.
