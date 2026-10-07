# Chain evidence review

Reviewer: this session, after the live chain stopped and before cleanup. Scope is the frozen contract, the owned driver, and the preserved run. This review does not approve M1-I01, M1-I02, semantic publication, public authentication, the harness, runsc, or a parent unit.

## Result

The failure matches the frozen contract and the archived compiler. `linux/chain/phases/apply.exit` is 1. Child `reviewed-policy-wide.exit` is 11. Child stderr is exactly `reviewed-policy: semantic`. Parent stderr is exactly `pg-onboarding: reviewed-policy-wide exit`. Apply outcome status is `failed` and `database_state` is `changes_may_have_been_applied`.

The wide allow list in `reviewed-policy-wide.stdin` is the 13 frozen decisions. `national_id` is omitted. `shop.entry.amount` is present with semantic `quantity`. The catalog type on that column is `numeric`, oid 1700. Archive `source/blobs/reviewed_policy.rs` `base_type` does not admit `numeric`, and `physical_type` returns `Semantic`. The bin blob maps that error to exit 11 and the observed stderr. `compatible` does admit int8 quantity, so `qty` is not the refusing column.

No gold line, decision, or expected query exit was edited after this result. The chain was not started a second time.

## Boundary evidence that did run

Inspect exit 0, status `awaiting_review`, identity `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`, cuts 317 and fence 1. Import rows 2000, 8000, 90000. Marker `甲注评` absent from catalog and captured draft. `check-inspect` exit 0.

Source load COPY counts match those row totals. Reader insert SQLSTATE 42501. Writer insert rolled back. One statement cancel SQLSTATE 57014. `watch.log` has one `priv_ok`.

Smoke of the archive before driver edits: 24 passed, 1 ignored. Focused driver tests after the edits: 9 passed, Clippy exit 0. Both builders were 6 GiB, 1 CPU, cap-drop ALL, pinned Rust 1.94, and were removed. PostgreSQL is the pinned 17.10 image, 1 GiB, 1 CPU, no published port.

## Not observed

Pack bytes, column ACL, analyze, `query-pg`, fresh count, numeric probe, target COPY compare, and restart have no outputs. `live/actual/` is empty. Those absent outputs are not treated as passes. The static numeric oid refusal in `query_executor.rs` was not executed.

`reviews/driver.md` was written before this chain. Its statement that the live chain had not run is true of that review and stale after this one.

## Request

No change to the frozen contract. The coordinator seam is the archived `base_type` refusal of `numeric` during `reviewed-policy`, with `query-pg` numeric oid 1700 still unexecuted behind it. Parent acceptance stays false.
