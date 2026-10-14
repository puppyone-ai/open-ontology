# Doris connection whole, 2026-10-14

Run `e875c8b248464b3f90acafcae65ac52d` is `recipe_completed`. The succeeding step exited 0 and was observed running `tools/source-onboarding/product_flow.py` and `tools/agent-product-verifier/dialogue.py`. `whole.json` SHA-256 `dfc4ddd8490a4e579b50eb6840e99af560706f6f96f790be6d5beca741873ea4` has `full_product_chain_complete` true, `periodic_insert_update_delete_verified` true, `recovery_verified` true, and `published` false. The doris row in `~/.cache/oo-source-onboarding/engines.json` points at that proof. Registry file SHA-256 `b0fa635c68d652dcfa30cab016ff82100421ec0d8e03abf786df3bf2d5fe747b`.

## Six-field entry

`/tmp/oo-doris-connection-whole/six-field.json` mode 0600, SHA-256 `464df0fb1c4d328a9622c21b43c0aa2825dffe220e222cc34e988d36328ee61a`. Keys are `protocol` plus connection `type`, `database`, `host`, `port`, `user`, `secret_ref`. The file has no `recipe` and no `out`. The pointer `six-field.json.run.json` records this run and the same input digest. Recipe SHA-256 `f35765f0f3d6822a19918cf7d302ae33ef9cf84b172f97fde74cc9ba06e726f8`. Pinned `tools/doris-preview/whole.py` SHA-256 `36fec9341bf59c4ef1541939b3196a20a923f93cc05f1576f465fdfbb2dae6d0`.

The completing command was `python3.14 tools/source-onboarding/onboard.py --input /tmp/oo-doris-connection-whole/six-field.json --resume`.

## Capture, C4/C5, gold, recovery

Lifecycle phases through `import_c4`, `gold`, and `recovery` are set on this run. Gold SHA-256 `43276de2b6275c9378d7acc565738ad76d09d565f10825d703cf1fdee93825da`: `all_equal` true, typed cells 109, typed cell diff 0. Recovery SHA-256 `c8dbb97031ac0e14bbd955dc4a031f94a124b63009cdb7ff702282b690479d1e`: `cancel_status` `cancelled|cancelled`, `load_state` `not_sealed`, `loader_invoked` false, watermarks equal before the refusal and after it, `recovery_verified` true. `jobs.tasks` rows `doris-cancel-once` and `doris-cancel-maintain` are both `cancelled`. This resume did not replay an ACKed partition, did not add `p202605`, and did not issue fixture DML.

## S0–S10 and dialogue

Build `b_1e205c67e1ad4861ca2b9438db5a0c44`. Product receipt SHA-256 `ddb24406bba0e834a51d30b42e2361f5e81f81470611fd66442ee74b3527283f`. Stages S0–S10 succeeded. S5 is `validated`, `builder_spawned` true, `structural_draft` false, `cache_hit` true, `llm_calls` 0, model `grok-4.7`, evidence `ed7c4168a49851d802e7aade1a03401618c38627cfa281c0b925d40576a65d52`. The sealed bundle catalog file is that same SHA-256. Native role pack on the product receipt SHA-256 `8eedcb1d74f6bfe36dcaded4c37029f263a0556386cc7d232b95a34f39389ca8`.

Dialogue verdict SHA-256 `9cb5d12cbe5f14b19b41a076a3b5656e5a3d93dfa1424f3733067c1f80389550`: outcome `verified`, turns 2, model `grok-4.7`, reasoning effort `low`, tools `describe`, `freshness`, `knowledge_cat`, `knowledge_search`, `query`. Query ids `q_hOFRHyio6wLmOnHauumYObEQ2J` and `q_tsCLibEJFW2qbuuak26e1qTo01`. Both ids are in the gateway WAL. Transcript SHA-256 `ace68d7acfcf03cd983aa4ca8bec6b112424bc08f3d86c0821631258217886d8`.

The live gateway binding is generation 10, catalog version `e37b34fe0e315dce71d85f9360821f9b64fd15b1b95f5e0b7c626928e3d47de5`. Installed command SHA-256 `d7732d8077c2c73a3149ebae64934f54bdd39905a8cb039b419df061e0793e40`. Installed role pack SHA-256 `524155bebad84b5fd640d98c4fc58cf07d9d6fb426cb8e0c33797744fb0cc0f2`. Public OO CLI queries for region east and region north returned 2.50 and 44.10 before this resume, and the dialogue queries above exited 0 during the observed step.

## Fixed bytes

Reader ELF `/tmp/oo-doris-connection-whole/driver-stripped/doris-reader` SHA-256 `b874654f023e4beb16b4376161f04a4fb33113ad337591ec05dc68bb8b592aba`. Loader `/tmp/oo-pg46-runtime-finish/oo-pipeline` SHA-256 `2ff14f824c6d4391e6487de6dd133e190966039ec77ce2345aad9cae1b1188e4`. Admission `/tmp/oo-pg46-runtime-finish/continuous/oo-source-admission` SHA-256 `07e7651a98b4ef76582f67666e1245065546db204a50899554981619efc05dbf`. This exit did not rebuild those ELFs and did not write root Cargo.lock. Lifecycle file SHA-256 `ea68e10f9752dfcb7cf18409b1239092e0855a5270af88f8f59c5b2c4eaa985a`.
