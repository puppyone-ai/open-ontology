# Findings

1. `benchmark_loop_pass` is false. The failing requirement is `q-entry-count`. The approved query policy id is `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`. The corrected propose still exited 3 with token `config` because the pack knowledge id `know-a317` is not a dialogue slug. Dialogue also requires the pack policy id to equal that 64-hex digest. The role-pack compiler's policy id is at most 63 bytes and may contain a hyphen, so the real wide pack cannot satisfy both producers. Execute was not started. The bearer file is 6 bytes and dialogue's bearer rule is 16 to 256 bytes; that check was not reached.

2. The grok-4.7 meter is the stored one. `builder-first.json` has three sockets `opened`, HTTP 200, usage sums, input SHA-256 values, and `declared_cost.status=unknown` with a null amount. Top cache `miss`. Call 1 was a structural draft and was not stored. `second-partial.json` opened one socket for that batch, HTTP 200, then stored it; the other two calls are `not_opened`. Top cache `partial`. `builder-hit.json` is a later full cache hit with every socket `not_opened`. The hit is not the fresh proof. `fresh_socket` in the report is `opened`. These three files were left in place.

3. HTTP 200 is recorded and is not an end-to-end pass. `http_200_alone_is_e2e_pass` is false. The entry-count cells were never returned by dialogue.

4. The three frozen `must_fail` controls matched their expected refusals, so their question verdicts are `pass`: grok-4.3 exit 1, `national_id` query exit 6 with empty stdout, and citation status `invalid` with exit 1. A zero exit, a returned national id, or citation status `ok` would fail the question. Gold cells and thresholds were not edited. `FREEZE.json` hash is unchanged.

5. Fixture gold for the 100000-row chain matches. Party 2000, account 8000, entry 90000. Query close `end_audit_ok` is true. Review is unauthenticated. Wide is installed and narrow is replay.

6. Whole-product keys are `notrun`. `integration_passed` is false. `parent_acceptance` is false. The first bench container was signal 143 after the chain had already exited 0. The chain was not repeated. The cache continuation used the stored drafts and the same database.

7. Focused proof is `linux/focused-4` and `linux/propose` (7 tests, Clippy 0). Earlier focused exits 101 are preserved. The first propose rebuild failed in `failed-attempts/propose-prep-dead-field/` because `Bound.policy_id` was unread. The corrected prep then ran in the same volumes.
