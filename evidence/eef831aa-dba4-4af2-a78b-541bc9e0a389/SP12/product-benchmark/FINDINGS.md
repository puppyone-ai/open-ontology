# Findings

1. `benchmark_loop_pass` is false. The failing requirement is `q-entry-count`. `ontology-dialogue propose` exited 3 with token `config`. The live pack policy id is `pol-a317` and the knowledge id is `know-a317`. The producer checks those fields before the bearer read and before HTTP. Execute was not started.

2. The grok-4.7 meter is real. Three batch calls returned HTTP 200, usage sums, input SHA-256 values, and `declared_cost.status=unknown` with a null amount. The first aggregate cache is `miss`. One batch was a structural draft and was not stored. The second generate stored that batch and hit the other two, so its aggregate cache is `partial` and one socket opened. The third generate, on the same cache directory, is aggregate `hit` with every socket `not_opened`. `generate-third.exit` is 0.

3. HTTP 200 is recorded and is not an end-to-end pass. `http_200_alone_is_e2e_pass` is false. The entry-count cells were never returned by dialogue.

4. The three required failure controls are counted as `fail`: grok-4.3 config refusal exit 1, `national_id` query exit 6 with empty stdout, and citation status `invalid` with exit 1.

5. Fixture gold for the 100000-row chain matches. Party 2000, account 8000, entry 90000. Query close `end_audit_ok` is true. Review is unauthenticated. Wide is installed and narrow is replay.

6. Whole-product keys are `notrun`. `integration_passed` is false. `parent_acceptance` is false. The first bench container was signal 143 after the chain had already exited 0. The chain was not repeated. The cache continuation used the stored drafts and the same database.

7. Focused proof is `linux/focused-4` (7 tests, Clippy 0). Earlier focused exits 101 are preserved.
