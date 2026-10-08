# Handoff

The card is not accepted. `report/report.json` has `benchmark_loop_pass` false, `integration_passed` false, and `parent_acceptance` false.

What is measured:

- Chain `linux/chain/chain.exit` 0 on archive `6f87942f1c66ba1a9787b7d7194e58a507af63ef`, rows 2000 / 8000 / 90000.
- pg-builder `be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba` against grok-4.7. Fresh HTTP is `score-input/builder-first.json` (three sockets opened) and the second stored call in `live/builder/second-partial.json` (one socket opened). `score-input/builder-hit.json` is the later cache hit and does not replace those calls.
- Wrong-model, hidden-query, and citation-false controls match the freeze, so their question verdicts are `pass`.
- The runner binds the approved query policy id `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`. Dialogue propose on that config exited 3, token `config`, before HTTP. The pack knowledge id `know-a317` fails the dialogue slug rule. The wide bundle was not rewritten.

What remains before this card can pass the loop:

- A pack whose policy id is the approved query digest and whose knowledge id is a dialogue slug, copied by the real compiler from source documents that already contain both. Commit `b9f1e4fc30f144d1cf7dc61aa7765f58164a9caa` accepts a 64-hex policy id and does not rewrite knowledge. The reviewed chain inputs still say `pol-a317` and `know-a317`. Dialogue still rejects that hyphen. This card does not edit those inputs and does not rewrite `live/run/bundles/wide.json`.
- The bearer file is 6 bytes. Production dialogue still accepts only 16 to 256 bytes. `development_local` is the only mode that accepts 6 bytes, and only with origin `http://127.0.0.1:<port>`. The stored propose config is production mode (`https://127.0.0.1:8443`, no transport field) and fails on the knowledge slug before that check.
- After a proposal exits 0, execute must return cells `[[{kind:int,value:90000}]]` for `q-entry-count` with citation status `ok`. HTTP 200 remains insufficient.
- Seven whole-product records are `notrun`. Integration stays false. This card does not call the loop closed.

Relay pin `bde5e4772bb83407de5e9a50f12c120ce5eca30e` forwards the caller bearer and has no client-token mint. Dialogue never reached HTTP, so that relay did not change the propose result.

Do not reload this 100k database to reproduce the score. The score inputs and `report/report.json` are the measurement. Codex updates the main ledger. This card does not.

## Quota-restored recheck 2026-10-08T07:46:38Z

Own containers, volumes, and networks labeled `oo.owner=a389` are absent. Phase files are unchanged: `chain.exit` 0, `pg/up.exit` 0, `bench.exit` 1, `propose-approved.exit` 3, `score-approved.exit` 1, `linux/propose/focused.exit` 0. `benchmark_loop_pass`, `integration_passed`, and `parent_acceptance` remain false. `q-entry-count` remains fail. `q-wrong-model`, `q-hidden`, and `q-citation-false` remain pass.

`crates/oo-builder/src/reviewed_effective.rs` blob `7345b036685401d543e174a45a414e0e0db19613` (commit `9daeaa05e34c154391a757fc78fb47f09c954c6a`) and `crates/oo-builder/src/role_pack.rs` blob `1327bad464fe8f510cc7baf8566ad71e3d00df2f` (commit `eff1959bf028a65b20b7fca855d79c23e4b188fd`) still reject an identity token longer than 63 bytes. `tools/ontology-dialogue` at HEAD is pin `e31635b213911c725159a23dbccc8463cc45cdd7`: `grammar.rs` blob `d753913cf81afe2c97f02c555a26c1a7eb65ba92`, `propose.rs` blob `7d614158e0ecf8ea40d81198e2a0ae2baefb4fbf`. A policy id must be 64 lowercase hex. A knowledge slug rejects a hyphen, so `know-a317` stays illegal there. Bearer length stays 16 to 256. The wide manifest is still policy `pol-a317` and knowledge `know-a317`.

No builder was started. The 100k fixture was not reloaded. `builder-first.json`, `second-partial.json`, and `builder-hit.json` were not regenerated. Gold, thresholds, and `FREEZE.json` were not edited. The card stays incomplete until a real compiler emits one pack that dialogue and query-pg both accept, and the original positive example is run against that pack.

## Producer re-read 2026-10-08T09:50:22Z

`specs/minimal-product-loop-plan.md` names this card as the P0 100k question-benchmark support line. a405 owns the one-command user flow. docs/09 §6.2 says Pi reads the pack and calls `oo` through bash. MCP is not a prerequisite. Admin, multi-user APIs, Git publication, customer UAT, and production runsc stay deferred. This card does not sign them, and they do not clear `q-entry-count`. The full Plan scope is unchanged. `benchmark_loop_pass` stays false.

HEAD commit `b9f1e4fc30f144d1cf7dc61aa7765f58164a9caa` changed the producer. Blobs: `reviewed_effective.rs` `b30207b6b3a0fec04a41b83663362889c44abaac`, `role_pack.rs` `51d000f11e0190df38e81141102d8d88c45e90f6`, dialogue `grammar.rs` `294c620bfd4cfc1d0ad27af234d783d5b71a2c58`, dialogue `propose.rs` `4266388aea3996d0788edef6b3492c487bb449f1`. A policy id may be the old slug or 64 lowercase hex. The compiler copies the id from its input. `check_knowledge_id` still calls `check_slug` and the source comment says `know-a317` is not rewritten. Dialogue `slug` still rejects a hyphen, so `parse_config` still returns `config` for the stored approved config. `development_local` allows a 6-byte bearer only for `http://127.0.0.1:<port>`. Production length stays 16 to 256.

Own `oo.owner=a389` containers, volumes, and networks are absent. Phase exits are unchanged. No builder was started. The 100k chain and the stored grok-4.7 documents were not rerun. The wide manifest is still `pol-a317` / `know-a317`. The chain source that emits those ids is `testing/real-db/pg-e2e/src/run.rs`, which is outside this card's write boundary.
