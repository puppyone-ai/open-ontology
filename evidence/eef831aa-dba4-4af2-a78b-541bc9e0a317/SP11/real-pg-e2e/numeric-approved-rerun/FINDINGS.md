# Findings

## Proven on this rerun

- One private PostgreSQL 17.10 loaded `shop.party` 2000, `shop.account` 8000, and `shop.entry` 90000. TLS SAN is `DNS:oo-a317-pg`. Autovacuum is off. No host port.
- Source reader insert returned SQLSTATE 42501. Source writer insert of one party row committed nothing: `INSERT 0 1` then `ROLLBACK`.
- One cancellation returned SQLSTATE 57014. `pg_cancel_backend` returned `t`. The watcher log records a single `priv_ok`.
- Inspect sealed `catalog.json`, `captured-draft.json`, and `import-result.json`, then stopped at `awaiting_review`. Config identity `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`. `source_cut` 317 and `target_fence` 1.
- `import-not-ready` exited 1 with `oo-pipeline: target schema is not ready`. The inspect command then exited 0.
- Default identity budget fields include `max_rows` 10000. The entry snapshot of 90000 rows still completed.
- `reviewed-policy-wide` and `reviewed-policy-narrow` exited 0. `base_type` on this archive includes `numeric`. `numeric` is compatible with `Quantity` only.
- Role pack, column ACL, and mapping-reader provision for wide and narrow exited 0. `analyze-pg` exited 0 with `published` false and `estimate_is_exact` false.
- `query-wide-count` returned int 90000. `query-wide-null` returned `p00000`, null, and true. Audits exist for those two statements.
- `query-wide-before` exited 5 with `query-pg: policy`. Hidden, insert, and `_oo_pos` queries exited 6 with `query-pg: guard`.
- Apply then stopped at `query-wide-qty`, child exit 6, stderr `query-pg: guard`. Parent stderr is `pg-onboarding: query-wide-qty exit`.

## Product seam

The frozen positive statement uses `entry_id IN (0, 1, 2)`. Archive `sql_guard.rs` `admit_op` refuses every `A_Expr` whose kind is not `AEXPR_OP`. `operator_form` names `AexprIn` as `IN predicate` and returns `GuardError::Unsupported`. `query-pg` prints only `query-pg: guard` and exit 6 for every guard error. The inner construct is discarded, and no audit file is written.

`shop.entry` has 90000 estimated rows. The large-table threshold on the request is 1000000. `large_relation` is reached only after the where-clause admit returns. `query-wide-count` already read that table through the same guard.

`entry_id` and `qty` are confirmed columns on the wide policy in the qty request. The equality statement `query-wide-null` was admitted. The `ORDER BY` of the qty statement is visited only after `admit_where`.

The amount column stayed `numeric(18,4)` with semantic `quantity`. Reviewed policy accepted it. `query-pg` did not execute the numeric probe, so this rerun does not show decoded `numeric` cells. The contract's numeric probe records the real exit and has no expected exit of 0.

## Preserved

- Parent evidence for archive `10ecd765` stays in place, including its `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, and `cleanup.log`.
- This rerun's raw exits, requests, and the empty qty audit stay in this directory.
- No gold line, decision, row count, or expected query exit was edited after the qty failure. The chain was not started a second time.

## Limits

- The immutable `tools/pg-onboarding/tests/live_chain.rs` still hardcodes generation 48. This fixture uses generation 317 and the driver under `testing/real-db/pg-e2e`. That driver was not edited for this rerun. Chain `driver-test` recorded 9 passed.
- Independent COPY gold was loaded into the source. The target physical tables were not copied out. `live/actual/` is empty.
- Password files and the CA private key stayed under `/tmp/oo-a317-eef831aa-dba4-4af2-a78b-541bc9e0a317/secrets` until owned cleanup. They are not in this evidence tree.
- No root manifest edit, no root lock edit, no `cargo update`, no product crate edit. a313 remains the root manifest and lock writer.
- Semantic publication, public authentication, natural language, harness mount, and runsc stay missing. M1-I01 and M1-I02 stay `not_accepted`.
