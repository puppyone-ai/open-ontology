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

That resume is not repeated. The `engines.json` digest is now `29ddf63d82d24b623683b980a722fa7cef8535769c8cfb920be4ea6274eaacd2`. It was `358c6c7502923ee9769d006f9906a917b2f0fd8cab943eefca4fe74ea7d037f5` at the previous receipt. The opensearch row is still `whole: true`, recipe `1e50d79629a113b7a94220879e033f3893d2e7f0ebc02ff26526722bf33c8615`, proof `4a5eb370…`. This source did not edit the registry or `onboard.py`. The old run was not resumed.

## Two owned I/U/D rounds

Binding `79fc4bb0-5676-41d7-8f53-b345cbb94e55` was enabled and then stepped only after fixture DBA changes. Each capture used the pinned reader and `native-fire` / `native-step` / `native-state`. `reader_mutations` was 0 on all six captures. Verdict: `/tmp/oo-opensearch-connection-whole/iud-two-20261014/verdict.json`.

| Label | Generation | Task | Live keys after the capture |
| --- | --- | --- | --- |
| r1 insert | 7 | `397e41dc-e895-41a3-8ab2-80b61beccbf8` | five stable keys plus `osconn14-644b5c6328c643fab933bc3b661ac463` |
| r1 update | 8 | `de894565-7c8a-4e9c-aff2-bff12020f59b` | same six, owned id in `updated` |
| r1 delete | 9 | `099467e1-76c9-43c1-b155-035bf2c54fe0` | five stable keys, owned id tombstoned |
| r2 insert | 10 | `865bdd0c-38cc-4b50-adc5-cfe31b7a0564` | five stable keys plus `osconn14-bb5edc68f4a84c14a93cf8166dd52568` |
| r2 update | 11 | `1b922f57-038f-41fd-b6b8-5804c562d3e5` | same six, owned id in `updated` |
| r2 delete | 12 | `0e919da4-9fce-4e5d-8a96-b58bc3d4ac74` | five stable keys, owned id tombstoned |

Canonical snapshot SHA-256 of the five live documents is `c8965e1755d8e0fff0cb38599eec48ca748caa833bbd517e1362033199a5bb95` at pins 6, 9, and 12. `_oo.pt_9de307c6323540063935bc9920722a23760107700d652c85db40d7f6ad4a` (generation 6) and `_oo.pt_673a7926ef6d97ac56fbe19b6fcf10ce4357a5e17f30d95ea28a926d7d20` (generation 12) each contain 5 rows. Delete detection on these cycles was reconcile. The source documents were removed with the fixture DBA identity, not by the reader.

## Continuous schedule

The binding stays `enabled` true, `period_ms` 1000, method C5, cycle reconcile. `native-serve` would enqueue every due period and the producer always emits `acknowledged_source_generation + 1`, so an unchanged tick would open another physical generation. The owned observer `/tmp/oo-opensearch-connection-whole/c5-observe.py` copies the production shadow, runs `c5-cycle` against that copy, and calls `native-fire` / `native-step` only when the probe reports an insert, update, tombstone, or typed change. The production shadow is not written by the probe. A three-tick quiet proof and the following daemon both stayed on generation 12 with `fires` 0. Daemon pid 66510, `--seconds 28800`. At status read, ticks were 154, `last_action` quiet, `reader_mutations` 0, live count 5.

## Live lease

`native-state`: generation 12, `awaiting_publish`, applied true, `applied_pos` 0, enabled true, period 1000, pin 12, task `0e919da4-9fce-4e5d-8a96-b58bc3d4ac74`. `source_write_attempts` 0 remains the generation-6 recovery fact. It is not a claim that the public plane is current.

## Open exit

`_oo.physical_mappings` max `run_generation` for source `oo_dl01w1_44`, config digest `006876333fa1dcbe780111c763867398c585a53e6a56330b3485a83e7bb3bf5f`, and `ledger.ledger_rows` is 12. The running gateway catalog is still bound to generation 6 (`binding.json`, physical `pt_9de307c6…`). `ensure_generation` requires that maximum to equal the bound generation, so `catalog_for` now fails `StaleGeneration` and the public CLI returns `SCHEMA_INVALID` / `policy catalog is not available`. Seen query ids: `q_7eZbKDsfbIsqIvIeXfwGZQ3BxC`, `q_pkHbG7YWDiO5VC8xBVNl9XX2Z7`. Before these rounds the same gateway returned count 5, generation 6 bound, query `q_2sDVomlZpHbotjAzTUx4ZskwG6`.

`oo_r_ledger` still has SELECT on 14 columns of the generation-6 table and on no column of `pt_673a7926…`. `pg_default_acl` has no table default SELECT for that role. This turn did not GRANT and did not rebind the gateway or the maintenance catalog. Generation-12 public gold, a new pack, and a new dialogue are not claimed. The generation-6 gold file is unchanged and is not a current-generation certificate. Publication stays unsigned. S5, the generation-6 CAS, the live cancel `q_JjCRmU3gOeVVrRxFkL39YtYryr`, and the three-turn dialogue were not replayed. Formal sign-off remains 0/10. The per-table ACL transition for generation 12 stays with PG pid 99653.
