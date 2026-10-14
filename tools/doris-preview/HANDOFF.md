# Doris handoff

Owner `5106fed0-5bcd-4aaf-8df7-838c95a7d7e2`. Worker `doris-connection-whole`. Source id `dorispreview`.

Formal signoff stays false. Published stays false. Full source stays 0/10. This file does not register an engine and does not claim `source_fresh`.

## 2026-10-15 invocation — journal failed

Private input `/tmp/oo-doris-connection-whole/direct-input-2.json` sha256 `8414b2adeea0c527d1c1e4280ba292d3f6fc6a77c427860d7149e5a2e8ab6108`. Run `/tmp/oo-doris-connection-whole/direct-run-2`. Recipe `/tmp/oo-doris-connection-whole/whole-recipe-direct.json` sha256 `d16c8cd7c7ebb07823f85be14179976d4e5471929b380aaa616a812e75cfb674`. Journal state `failed`. `engines.json` was not written.

Pins: python `f2bb9677ea7661fcf4703fa704c2049d7b7066dc4a1ffb0cfd1ceceed3b2a79c`; `tools/doris-preview/whole.py` `0367d145b032c386d85d9c85b8739865ed93a2e4344fdcb08f61998a6f723fa7`; `product_flow.py` `7784238585f46927f245879732f0d3714c7dedc513952a4fcadbac00051b95db`; `dialogue.py` `816ffbaefc30bd5e2fd633c44a2dc4c20d21db5cd7eff8ad09eb3ba9810d7afc`.

Steps: product exit 0, `reused_completed`, build `b_1e205c67e1ad4861ca2b9438db5a0c44`, S0–S10 succeeded, receipt sha256 still `ddb24406bba0e834a51d30b42e2361f5e81f81470611fd66442ee74b3527283f`, producer `b20148b6a634819f8503d35dd019d64d19b71e1efdd280dbf846e3e40f93674b`, maintenance llm 0, collection `12cfb998ab05aef8cd92e55106b4c251240ded5a3c0d52eaece4c31fe1fad54c`, role `oo_r_dorispreview`, role file `54381ced9567f68ff7cb426b6b1bfc9ae3ca50d614cd6f0981c5aae398dd85a6`. Consumption sha256 `d0d237e4e6545afec43cda06f494bf39316d21eb7da404899657a3d6798f8c2e` has `schema_digests` length 7 and no `schema_digest`. Bind exit 0, source generation 19, token hash `d81edb4f055ef16ae8f5171558ae08ec8c481e468a759048ff8da4c24e087dde` (same grant, plaintext not recorded). Dialogue exit 0: model `grok-4.7`, turns 2, seed `6288737835991493665`, semantic ids `agg_qty.sku-m.qty` and `str_key.population`, query ids `q_ihd29aTdtCYqoBXxBL97RnBKaf` and `q_wlu8ISJcWacZV60vxGlBjij7NQ`, gold/truth sha256 `afcb23a133dd5213721224d874aa224e2c1392edbbdf5dc00b79d321556b32fb`, verdict sha256 `b1af985065707f2dc566bbee4173bccf229dc590ff702c26a7423b4427219edf`, audit sha256 `a4ecf0dbf30c899e278e8eb1f73567aca4e2fd20656012aec06d31d5eaef9c67`. Close exit 2. stderr: `doris-whole: KeyError: 'schema_digest'`.

`prove_whole` reads `fresh['schema_digest']` at `tools/source-onboarding/onboard.py:297`. A collection consumption is built at line 228 as `schema_digests` only. That KeyError is not `Refused`. Close had already written the three completion flags true; those flags are now false in `direct-run-2/whole.json`, with `prove_whole` and `journal_state` recorded. The journal file was not edited. Do not resume this run. Do not pass `--whole-run`. Another dialogue on this identity is not required.

Earlier failure retained: `/tmp/oo-doris-connection-whole/direct-run`, illegal semantic id `agg_qty.skuM.qty`, no model call, journal failed.

## Root repin, not executed here

```
/opt/homebrew/opt/python@3.14/bin/python3.14 tools/source-onboarding/register_engine.py --engine doris --recipe /tmp/oo-doris-connection-whole/whole-recipe-direct.json --secret-registry /tmp/oo-doris-connection-whole/secret-registry.json
```

Omit `--whole-run`. Expected record: `whole` false, `awaiting_run` true. Live registry row is still `/Users/redux/.cache/oo-source-onboarding/cards/doris-product-d83608a5.json` sha256 `f1f0b47c883a91371a5fbe58a9ccc9ad00d4df7ac0d1a992937fb1773116976c`, `whole` false, `awaiting_run` true. Six-field input `/tmp/oo-doris-connection-whole/six-field.json` stays connection-only (`type`, `host`, `port`, `database`, `user`, `secret_ref`). Secret ref `secret://doris-connection-whole/reader`.

## Capture, left as two legal shapes

`part_day` load is C4, source generation 16, target generation 28, partition `p202604`. C5 load tables: `dup_events`, `uniq_acct`, `nz_obs`. `agg_qty`, `dec_exact`, `str_key` stay generation 1. `source_semantics.json` still labels all seven tables C5 and was not rewritten. C3 is `not_used`. C1 is false. No DELETE around OO014. Profiles §9.6 and the production-route card allow C4 partition refresh or documented C3/C5 per table, not an external CDC claim for Doris 2.x/3.x. SP04, SP09, and SP13 parent items stay unchecked.

The current wrapper now derives `current-source-semantics.json` from the four generation-bound request/load receipt pairs when a new normal run closes. It records the C4/C5/static split without rewriting the immutable prior Self-build bundle; the prior all-C5 bundle hash remains attached for provenance. The composer checks method, generation, and `p202604` against the receipts before emitting this sidecar.

The next normal random draw excludes the successful `agg_qty.sku-m.qty` / `str_key.population` pair from `direct-run-2`; they are retained in the prior-sequence ledger. The candidate bank retains four other source-grounded facts, and each candidate is re-read from the live fixture before the selection manifest is frozen.

## 2026-10-15 official default attempt — gold preflight failed

The registered six-field attempt at `/Users/redux/.cache/oo-source-onboarding/runs/3a2d988a7fa1492e8093ad197342b955` used recipe SHA `b4c7ce3244e38f902f64ddee151b48fcc382def0bf7b1efc3d39c1733d0a614b` and wrapper pin `4b0b545abb878ff7b996663d17fdea57b8add9304c0762a93de37852751c36f4`. It reused the retained S0–S10 product and receipt (`ddb24406…27283f`) and produced maintenance consumption SHA `1c50813627f69beac374fe313085a7cd39a6acd48591225f39abe9df72d3d8c0` with maintenance LLM calls 0. Bind exited 2 before any model call with `RuntimeError: gold_moved`; journal stays failed with product succeeded and bind failed.

The direct FE read was healthy and matched the source values: `part_day` 2026-03-11 west `33.25`, 2026-04-22 west `8.80`, `uniq_acct` id 1 `conn_whole`, `agg_qty` sku M `9.75`, row counts `agg_qty` 5 and `str_key` 7. The cause was the local gold map dropping the last two expected values when their question candidates were removed. The current wrapper restores all six independent FE facts to the gold map while keeping the four unused candidates out of random selection. This failed run and its recipe hash remain untouched; a new registered recipe/run is required for the corrected pin.

## Retained, not replayed

e875 product `/Users/redux/.cache/oo-source-onboarding/runs/e875c8b248464b3f90acafcae65ac52d/product`. Receipt sha256 `ddb24406bba0e834a51d30b42e2361f5e81f81470611fd66442ee74b3527283f`. Historical whole proof sha256 `dfc4ddd8490a4e579b50eb6840e99af560706f6f96f790be6d5beca741873ea4` belongs to that run and is not attached to collection `12cfb`. Do not resume e875. `product/oos` is mode 0700 and `ontology.yaml`, `effective-oos.yaml`, `source.json` are mode 0600; bytes were not rewritten. Prior random sequences stay prior. The new pair above is the invocation proof. Live recheck inside close saw `uniq_acct` id 5 label `aa427c2`, `cancel-aa427/recovery.json` `recovery_verified` true, and `cycles-aa427/period2.json` containing `aa427c2`. Those facts do not set the completion flags. No second IUD, no GRANT, no new S5. Customer writes 0. Wrapper `--self-check` printed `ok` and exited 0 on 2026-10-15.

## Resources still up

- `oo-doris-connection-whole-pg` database `dorisconnection2`
- `oo-doris-connection-whole-api` `127.0.0.1:18473`
- `oo-doris-connection-whole-gateway` listen `127.0.0.1:18573`
- FE `oo-dl01w1-43-ns2-fe` `172.16.143.10:9030` database `oo_dl01w1_43`
- Token `/tmp/oo-doris-connection-whole/gateway/client.token` mode 0600. Grant file `gateway/grant.json`. Bearer file `/tmp/oo-product-model-20261012/bearer`. Plaintext is not recorded here.
- Card `/tmp/oo-doris-connection-whole/source-card-direct-run-2.json`. Broker key `doris_direct_invocation_20261015` in `~/.cache/oo-grok-connection-full-20261014/CURRENT-DEPENDENCIES.json`.

## Blocker owned outside this crate

`tools/source-onboarding/onboard.py` line 297 must accept a collection that has `schema_digests` and no `schema_digest` (`fresh.get('schema_digest')` matches the collection branch on line 228). Until `prove_whole` returns on a `recipe_completed` journal for this recipe, the normal default source is not whole.
