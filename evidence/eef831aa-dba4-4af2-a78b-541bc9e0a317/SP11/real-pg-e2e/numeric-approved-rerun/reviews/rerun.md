# Numeric-approved rerun review

Reviewer: this session, after the live chain stopped and before inventory and cleanup. Scope is the frozen contract, the unchanged driver, and this preserved run. This review does not approve M1-I01, M1-I02, semantic publication, public authentication, the harness, runsc, or a parent unit.

## Result

The failure matches the frozen contract and the archived compiler. `linux/chain/phases/apply.exit` is 1. Child `query-wide-qty.exit` is 6. Child stderr is exactly `query-pg: guard`. Parent stderr is exactly `pg-onboarding: query-wide-qty exit`. Apply outcome status is `failed` and `database_state` is `changes_may_have_been_applied`.

The statement in `wide-qty.request.json` is the frozen `SELECT entry_id, qty FROM shop.entry WHERE entry_id IN (0, 1, 2) ORDER BY entry_id`. Archive `source/blobs/sql_guard.rs` `admit_op` refuses `AexprIn` through `operator_form` as `Unsupported` with construct `IN predicate`. `query-pg` maps every guard error to exit 6 and drops the construct. No qty audit file exists.

`shop.entry` estimated rows are 90000 and the request threshold is 1000000. `query-wide-count` and `query-wide-null` exited 0 on this same apply, so the guard is not refusing every post-bundle read.

No gold line, decision, or expected query exit was edited after this result. The chain was not started a second time. The driver tree was not edited.

## Boundary evidence that did run

Inspect exit 0, status `awaiting_review`, identity `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`, cuts 317 and fence 1. Import rows 2000, 8000, 90000. `check-inspect` exit 0.

`reviewed-policy-wide` and `reviewed-policy-narrow` exit 0. Both role packs, both column ACL installs, and both mapping-reader provisions exit 0. `analyze-pg` exit 0 with `published` false.

`import-not-ready` exit 1, stderr `oo-pipeline: target schema is not ready`. `query-wide-before` exit 5, stderr `query-pg: policy`. `query-wide-hidden`, `query-wide-insert`, and `query-wide-pos` exit 6, stderr `query-pg: guard`.

Source load COPY counts match the row totals. Reader insert SQLSTATE 42501. Writer insert rolled back. One statement cancel SQLSTATE 57014. `watch.log` has one `priv_ok`.

Smoke of archive `21cc962` before the chain: `pg-onboarding` 24 passed, 1 ignored. Chain driver tests: 9 passed. Both builders were 6 GiB, 1 CPU, cap-drop ALL, pinned Rust 1.94, and were removed. PostgreSQL is the pinned 17.10 image, 1 GiB, 1 CPU, no published port. `cargo update` was not invoked.

## Not observed

Pack digest check, narrow queries, `wide-fk`, `wide-phone`, the numeric amount probe, target COPY compare, fresh query, and restart have no outputs. `live/actual/` is empty. Those absent outputs are not treated as passes. Reviewed policy admitted `numeric` quantity. This run did not decode an amount cell.

## Request

No change to the frozen contract. The coordinator seam is the archived `IN` predicate refusal inside `query-pg` guard exit 6, after reviewed policy, packs, ACL, and analyze have exited 0. Parent acceptance stays false.
