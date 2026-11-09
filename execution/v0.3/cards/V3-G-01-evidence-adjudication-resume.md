## Resume: V3-G-01-evidence-adjudication

The previous run of this card stopped mid-task on a model relay 502, not on a defect. Its commits are on this branch; uncommitted changes in the worktree are its unfinished work (rule 5: review, keep what is correct, finish). Continue the card below from where it stopped; do not redo committed parts. Do not delete files with rm -f; use git or targeted edits.

## V3-G-01 — evidence-based adjudication replaces per-item review (D28)
Plan: specs/v0.3-quickstart-and-competitive-plan.md. Commit as the owner, no agent trailers, trailer `Plan: <card id>`. Never print or commit model keys, endpoints or private terms. Do not touch unrelated containers. Each card ends with unit tests plus the named integration test; report commits, tests, residuals.

Builder side (crates/oo-builder, tools/pg-builder). For each warn class compute evidence on the Agent DB replica and decide:
- relation without declared FK: containment ratio (non-null child values in parent key, full or large sample), parent uniqueness, type/length compatibility, name similarity, JOIN co-occurrence in views/query log, measured cardinality. Adopt if containment>=0.98 and parent unique, or >=0.9 with name or JOIN evidence. Reuse existing containment code (community.rs, role_pack.rs inclusion).
- sensitive column: name rules + value format hit rate (phone, id card, email, bank card), cardinality, entropy. Name-only suspicion with negative value profile => not sensitive.
- metric: reconcile on replica (non-empty, unit vs column semantics, no aggregation over duplicate keys, equals row-wise recompute, stable across windows); fail => demote to example query.
- entity key/display: full distinct ratio, non-null ratio; display = most readable unique/near-unique text column; else plain dataset.
- code table: coverage >=0.99 => complete.
Every decision records {class, decision, evidence, scores, rule_id}; thresholds in one config. Output feeds V3-A-02.
Test D28-T: tools/oo-test harness computing precision/recall per class on T6 gold (testing/engines/gold) and OO-Logistics (bench/datagen); relation recall must not drop below current human-review baseline. Fixed evidence.

Write scope: crates/oo-builder, tools/pg-builder, tools/oo-test (D28-T harness), new migration only if needed (report it). Output must follow `execution/v0.3/contracts/adjudication.md` exactly (graph block included); add a JSON-schema test. Do not edit crates/oo-cli UI files (a separate TUI lane owns them); the compiler acceptance of `oo.publish-confirm/v1` lives in the builder/compiler side and is yours.
