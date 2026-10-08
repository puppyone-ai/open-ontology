# Handoff

The card is not accepted. `report/report.json` has `benchmark_loop_pass` false, `integration_passed` false, and `parent_acceptance` false.

What is measured:

- Chain `linux/chain/chain.exit` 0 on archive `6f87942f1c66ba1a9787b7d7194e58a507af63ef`, rows 2000 / 8000 / 90000.
- pg-builder `be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba` against grok-4.7. Final score input `score-input/builder-hit.json` is a three-call cache hit with socket `not_opened`. The miss and the partial documents remain `score-input/builder-first.json` and `live/builder/second-partial.json`.
- Wrong-model, hidden-query, and citation-false controls are `fail` verdicts.
- Dialogue producer `e31635b213911c725159a23dbccc8463cc45cdd7` refused the real wide pack at config validation. Exit 3. Stderr `ontology-dialogue: config`.

What remains for a later card, on a pack the dialogue grammar can load:

- `policy.id` must be 64 lowercase hex and `knowledge.id` must be a slug, and both must be the pack's own ids. This run did not rewrite `live/run/bundles/wide.json`.
- After a proposal exits 0, execute must return cells `[[{kind:int,value:90000}]]` for `q-entry-count` with citation status `ok`. HTTP 200 remains insufficient.

Relay pin `bde5e4772bb83407de5e9a50f12c120ce5eca30e` forwards the caller bearer and has no client-token mint. Dialogue never reached HTTP, so that relay did not change the propose result.

Do not reload this 100k database to reproduce the score. The score inputs and `report/report.json` are the measurement. Codex updates the main ledger. This card does not.
