# Review 2: approval order and role variants

Reviewer is the same session that wrote the crate. This is not an independent second-author acceptance. The tree under review is the attempt 5 source. No source change came out of this pass.

`GoldView::defect` stops at the first hit, in the contract order: ambiguity when a clarify question does not have exactly one selected interpretation, then missing gold, then a relative tolerance, then `declared_dual_disagree`, otherwise no gold defect. `fully_approved` requires that declared status, no `unresolved` key, and no defect. Defects 1, 2, and 4 on a fully approved declaration are `review` at `/questions/<n>/review/status`. Defect 3 is `tolerance` at the first relative value column. The missing-gold-approved and relative-tolerance fixtures take those two tokens.

`needs_reannotation` requires `unresolved` to equal the computed defect, or `declared_unresolved` when the gold is complete. The program does not rewrite that status to `fully_approved`. Exit 1 still emits the question in the manifest, the agent view, and the scorer view. The mixed fixture keeps question count 5, with 3 fully approved and 2 needs reannotation. `declared_dual_agree` is copied through and the attestation on the scorer view is `label_only`.

A role group of two or more questions must have distinct roles. The duplicate role is `schema` at the later `/questions/<n>/asker/role`, and that check runs before expected equality. Distinct roles with equal canonical expected objects are `role` at the later `/questions/<n>/expected`. A singleton group is allowed. Canonical expected bytes are `serde_json::to_string` of the expected object, whose map is a `BTreeMap`.

A selected interpretation whose gold SQL differs from the question gold SQL is `schema` at `/questions/<n>/accepted_interpretations/<i>/gold_sql`. The mismatch fixture places that difference on question 1. Two selected interpretations, or none, skip that comparison and become ambiguity.

This review does not treat a declared label as a signature and does not accept the 20% dual-annotation gate.
