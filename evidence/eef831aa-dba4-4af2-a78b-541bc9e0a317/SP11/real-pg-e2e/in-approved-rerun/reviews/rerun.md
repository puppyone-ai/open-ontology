# IN-approved rerun review

Reviewer: this session, after the live chain exited 0 and before inventory and cleanup. Scope is the frozen contract, the unchanged driver, and this preserved run. This review does not approve M1-I01, M1-I02, semantic publication, public authentication, the harness, runsc, or a parent unit.

## Result

The run matches the frozen contract on archive `6f87942f1c66ba1a9787b7d7194e58a507af63ef`. `linux/chain/phases/apply.exit` is 0. Child `query-wide-qty.exit` is 0. Child stderr is empty. The three rows are int8 minimum, int8 maximum, and zero, in `entry_id` order. `live/run/audit/wide-qty.jsonl` exists. Apply outcome status is `ready_for_trusted_query`, `analyze_published` false, `estimate_is_exact` false, `database_state` `child_compilers_applied_no_release`.

The statement in `wide-qty.request.json` is the frozen `SELECT entry_id, qty FROM shop.entry WHERE entry_id IN (0, 1, 2) ORDER BY entry_id`. Archive `source/blobs/sql_guard.rs` blob `6d7aef7bb95c9550a7e6f1bc7f3f22f7c098de4f` routes `AEXPR_IN` to `admit_in`. The list length check admits 1 to 32 scalar literals. This statement has three.

No gold line, decision, or expected query exit was edited. The chain was not started a second time. The driver tree was not edited. The parent failure tree and `numeric-approved-rerun` were not rewritten.

## Boundary evidence that did run

Inspect exit 0, status `awaiting_review`, identity `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`, cuts 317 and fence 1. Import rows 2000, 8000, 90000. `check-inspect` exit 0.

`reviewed-policy-wide` and `reviewed-policy-narrow` exit 0. Both role packs, both column ACL installs, and both mapping-reader provisions exit 0. Pack digests are stable. `analyze-pg` exit 0 with `published` false.

`import-not-ready` exit 1, stderr `oo-pipeline: target schema is not ready`. `query-wide-before` exit 5, stderr `query-pg: policy`. `query-wide-hidden`, `query-wide-insert`, `query-wide-pos`, `query-narrow-hidden`, `query-narrow-phone`, and `query-narrow-amount` exit 6, stderr `query-pg: guard`.

`query-wide-count`, `query-wide-null`, `query-wide-fk`, `query-wide-phone`, and `query-narrow-party` exit 0 with the gold cells named in `REPORT.md`. `check-apply` exit 0.

Target COPY checks exit 0 for party, account, and entry. Fixture COPY bytes and target COPY bytes match. The amount probe exit is 0 and the cell is decimal coefficient `12345678901234` scale 4. Fresh party count is int 2000 before and after one restart.

Source load COPY counts match the row totals. Reader insert SQLSTATE 42501. Writer insert rolled back. One statement cancel SQLSTATE 57014. `watch.log` has one `priv_ok` and one `restart_ok`.

Smoke of archive `6f87942` before the chain: `pg-onboarding` 24 passed, 1 ignored. Focused driver tests: 9 passed, clippy exit 0. Chain driver tests: 9 passed. Builders were 6 GiB, 1 CPU, cap-drop ALL, pinned Rust 1.94, and were removed. PostgreSQL is the pinned 17.10 image, 1 GiB, 1 CPU, no published port. `cargo update` was not invoked.

## Still open

The outcome non-claims keep `m1_i01` and `m1_i02` at `not_accepted`, `semantic_publication` at `missing`, `public_auth` false, `harness_mount` at `not_performed`, and `analyze_published` false. Those fields are the remaining parent gates. This review does not convert `ready_for_trusted_query` into acceptance.

`pack_catalog.rs` differs between `21cc962` and `6f87942`. The bundle files in this run are outputs of the compiled `6f87942` binary. This review checks their digests and the query results. It does not review the pack-catalog diff.

## Request

No change to the frozen contract. No second chain. Parent acceptance stays false.
