# ClickHouse run c2c83cdbf24740a19338c87df7d7957f

Formal signoff 0/10. Publication `not_claimed`. Customer metadata LIMIT 5 was not run.

Registry `whole=true` after `register_engine.py --whole-run` of this run. `whole.json` bytes match the operator run, so proof sha256 `265c78ee7434da881fddf1139c93f40f408d543c63fcf931a959487bd32d2c05` is the gate document, not a unique evidence id. Operator commit `695834b76078b1a186bf705ccdb682c745e45b06` and `/tmp/oo-clickhouse-connection-whole/whole-run` stay the staged S0–S10 of that day. Run `c815b18bc91d4e27a3f2706874138bb1` was not resumed.

## This run

- Input sha256 `868ca65f6c58a9c0d86c2162584b05d42be6d4a2436103debe0f8fd9b9e74745`. Recipe sha256 `d16bb19cbc0709adca00487c2bf72ba97ba1519e3957e5bdfc3c27944d96fb8d`.
- Periods: 3.0041474159806967s insert 107 generation 9, then 7.00336154201068s physical delete 107 generation 10. Keys ended at 101/3/8, 103/1/0, 104/1/4, 105/1/5.
- Replay scope `this-run c4-p2 replay`: before 4, after 4, `second_import=accepted`, lease `6d131e68-5304-48ed-bb33-a168e05b2840` inserted false. Open `jobs.leases` now 0.
- Generation 11 load `ec5f4d0c-0965-4f37-8518-0e8615f56ac3` is `awaiting_publish`, typed diff 0. Observed partition ids `202401,202402,202403,209912`. 202403 is generation 11. 209912 stays generation 3, row count 0.
- Live watermark `S0|lagging|partition-signature/v1|true|false|11`. Cycle receipt still records the period-2 string ending in generation 10. Source commit clock is null. Cross-table grade is B.
- Source FINAL matches the four keys above. Pending `system.mutations` for `oo_dl01w40` is 0. Reader `QueryFinish` insert/alter/delete in the last day is 0. Three `ExceptionBeforeStart` refused inserts stay: one `oo-ch-conn-whole-reader-insert` and two `oo-ch-default-reader-insert`.
- Product build `b_e40e75018cac19da2b3f7b29fcc03cc7`. S5 `validated`, `builder_spawned` true, `structural_draft` false, `cache_hit` true, `llm_calls` 0. Cached identity grok-4.7, usage 938+1634=2572. Amount is money, unit `unspecified`.
- Gateway probe `q_XwpcUHC3PIwIQfMVoBbdrx5dvy`. Cancel `q_U1yWHM8lNRo9BEfH13GzXNEsaG`: audit durable, outcome sent, query `TIMEOUT` / sqlstate 57014. `oo_r_reader` column grants are generation 11.
- Verified dialogue grok-4.7, reasoning low, 2 turns, tools `describe`, `freshness`, `knowledge_cat`, `knowledge_search`, `query`. Query ids `q_jVJPcLrWWRzsCH2W2Z1H4mjTMX`, `q_q7YFBObnHXUYBB7eNrLM2ffJsu`. Audit sha256 `2f410a4fc407cd9acf9047aada2f20e9eecbfacc2131552405cb69e73fdf145a`.

## Kept failures

Catalog generation 10 stderr `oo-pipeline: target database`. Prepare build `b_242fe0a87a04318b00fcaeee5c6ec4c2` S5 `model.candidates_invalid` (model money/USD, rules still quantity). Product build `b_077be4285dc6f003e93732c28b74d224` review `membership.refused` while amount pii was unknown. ACL exit 4 `scan` before stdin was attached. First dialogue missed `knowledge_cat` and is in `dialogue-attempt1-missing-knowledge-cat`.

## Fixed ELF

Host rustc is 1.98.1 and did not rebuild these. Each pinned ELF contains `1.94.0` and is Linux ARM aarch64. `oo-pipeline` sha256 `2ff14f824c6d4391e6487de6dd133e190966039ec77ce2345aad9cae1b1188e4` printed `oo-pipeline: usage` inside `linux/arm64` (exit 2). ACL ELF sha256 `aea3dad54671b9608a2ab1a739a2160681bc9cd8f4eacf08a49ce3c45a79cf05`.
