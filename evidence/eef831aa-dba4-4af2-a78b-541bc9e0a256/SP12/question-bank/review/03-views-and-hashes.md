# Review 3: agent view, scorer view, and hashes

Reviewer is the same session that wrote the crate. This is not an independent second-author acceptance. The tree under review is the attempt 5 source. No source change came out of this pass.

The agent view emits version, dataset id, suite id, bank version, and questions. Each question emits id, question, asker (role and claims), category, difficulty, required metrics, as_of, split, and version. The walk in `tests/bank.rs` rejects `expected`, `gold_sql`, `typed_gold`, `ordered`, `key_columns`, `value_columns`, `accepted_interpretations`, `control_totals`, `annotations`, `provenance`, `review`, `role_group`, `unresolved`, and `tolerance` anywhere under the agent view. The four canaries `GOLD_SQL_CANARY_7c21e9`, `GOLD_EXPECTED_CANARY_b44e`, `CONTROL_TOTAL_CANARY_9f3a`, and `ANNOTATION_CANARY_c18d` are asserted absent from the agent-view document bytes and present in the scorer-view document bytes.

The scorer view keeps every question, the expected object, typed gold, control totals, annotations, provenance, and the emitted review labels. `question_count` is the full denominator. Typed gold for `pub-competitor` has keys `completion`, `kind`, and `warnings` only, completion `complete`, kind `not_found`, and no `version`, `row_comparator`, or `actual`.

`sha256_hex` calls `oo_core::canonical::hash_bytes`. The FIPS 180-4 vectors for the empty string, `abc`, and the 448-bit alphabet string match. The manifest stores `agent_view_sha256` and `scorer_view_sha256`, and each of the three report hashes equals a fresh digest of that document, including the manifest hash over those two strings. A scale change moves only the manifest hash. A gold SQL change moves the scorer and manifest hashes and leaves the agent hash. A question-text change moves the agent hash. Swapping the first two mixed questions matches `reordered.json` and moves all three hashes. Repeating the mixed input matches the first report bytes. Input key order and whitespace are normalized by a parse-and-compact round trip before that comparison.

The same-input CLI run of `mixed.json` exits 1 twice with equal stdout. The reordered CLI stdout differs. Stderr is empty on those report runs.

This review does not accept SP12-W01 or a 300-question bank.
