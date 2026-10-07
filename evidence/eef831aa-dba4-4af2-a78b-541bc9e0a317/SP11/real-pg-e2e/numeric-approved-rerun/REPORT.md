# Numeric-approved rerun

Session `eef831aa-dba4-4af2-a78b-541bc9e0a317`. Product archive `21cc96241a841b47dbbcb116549030e38a643ba0`, the a340 `APPROVE_LIMITED` numeric commit. The original apply seam on `10ecd7654fd7939b6655a0e50a84214a7fca316e` stays in the parent evidence tree. Owned fixture and gold under `testing/real-db/pg-e2e` are the tree `79e2ddc120b2117f90d3e79ff61571490243f23b` from commit `f4381843c7f376f196bc30c78c18d9701dc29a6d`. This report does not accept a parent unit, M1-I01, M1-I02, semantic publication, public authentication, the harness, runsc, or a release.

## Result

Linux smoke of the archive exited 0. Inspect reached `awaiting_review`. Apply passed reviewed policy, both role packs, column ACL, mapping-reader provision, and analyze, then stopped at `query-wide-qty`.

`linux/chain/chain.exit` is 1. `linux/chain/docker-run.exit` is 1. `linux/chain/phases/apply.exit` is 1. `live/work/apply.stderr` is `pg-onboarding: query-wide-qty exit`. `live/work/apply.stdout` is empty. `live/run/steps/query-wide-qty.exit` is 6. `live/run/steps/query-wide-qty.stderr` is `query-pg: guard`. That stdout and stdin are empty. `live/run/audit/` has `wide-count.jsonl` and `wide-null.jsonl` only. `live/run/apply-outcome.json` has `status` `failed`, `step` `query-wide-qty`, `detail` `exit`, `database_state` `changes_may_have_been_applied`, `database_rollback` false, and `database_release` false.

The chain was not rerun. The frozen SQL, the decision table, the row formulas, and the expected exit 0 on `wide-qty` were left as written.

## Blocking seam

Archive `crates/oo-gateway/src/sql_guard.rs` blob `1db8f3ed11a2d816d485bdd2865f37aff3e5cbed`. A copy is `source/blobs/sql_guard.rs`. `git hash-object` of that copy matches the blob.

`admit_op` admits an `A_Expr` only when `kind` is `AEXPR_OP`. Any other kind returns `operator_form`. `AexprIn` becomes `GuardError::Unsupported { construct: "IN predicate" }`. `scan_tree` walks `A_Expr` and `List` and returns success for that shape. `admit_where` runs before `admit_sort`. `large_relation` runs only after `admit_where` returns.

The frozen statement is `SELECT entry_id, qty FROM shop.entry WHERE entry_id IN (0, 1, 2) ORDER BY entry_id`. The request is `live/run/queries/wide-qty.request.json`. `entry_id` and `qty` are on the wide allow list, semantic identifier and quantity, pii none.

`crates/oo-gateway/src/query_executor.rs` blob `fc3a65254881c62f8321bc01e405d6b3d45dabb1` maps every `guard_select` error to `ExecError::Guard`. `crates/oo-gateway/src/bin/query-pg.rs` blob `f404eb1879cdfbbcc3c3a64547c10a40939402ed` maps that to exit 6 and the stderr line `query-pg: guard`. The construct string is not printed. The audit file is not created.

`shop.entry` `estimated_rows` is 90000. The request catalog `large_table_row_threshold` is 1000000. `query-wide-count` exited 0 and returned int `90000`. `query-wide-null` exited 0 and returned text `p00000`, null, and bool true. Those two statements do not use `IN`.

## Cleared on this archive

`reviewed-policy-wide.exit` and `reviewed-policy-narrow.exit` are 0. Archive `reviewed_policy.rs` blob `8f723806217954ad264fa45970dc1ddd9ea94fe9` admits `base_type` name `numeric`, and `compatible` admits `numeric` only with `Quantity`. Catalog `shop.entry.amount` remains `type_oid` 1700. The parent evidence seam, exit 11 `reviewed-policy: semantic` on archive `10ecd765`, is the previous commit. This rerun passed that step. The numeric probe that would read `amount` was not started. Target COPY compare was not started. `live/actual/` is empty.

`tools/pg-onboarding` tree `b0e6fd768c8d919df3b8d6683dd44d444608c78c` is the same tree as `10ecd765`.

## What completed

PostgreSQL 17.10, one private container `oo-a317-pg`, id `ae06598ca490c38e5664ef3fe7f5655ebf28d8575b42af6c72490c16371364e8`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/server.txt` records `server_version=17.10 (Debian 17.10-1.pgdg12+1)`, `autovacuum=off`, `ssl=true`, and `COPY 2000`, `COPY 8000`, `COPY 90000`. Certificate subject `CN=oo-a317-pg`, SAN `DNS:oo-a317-pg`. Memory 1073741824, 1 CPU. `pg/ports.json` is `{}`. `pg/up.exit` is 0.

Smoke builder `598f8c29789f0c34d46bd36d009784efa1ea4d22acc05c0dde0b16d0038d4959`, 6 GiB, 1 CPU, cap-drop ALL, OOMKilled false. Toolchain `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `linux/smoke/test.log` result lines for `pg-onboarding` are lib 4 passed, `failure_truth` 11 passed, `live_chain` 1 ignored, `orchestration` 9 passed. `cargo-update-invoked.txt` is `no`. `linux/smoke/docker-run.exit` is 0.

Chain builder `c0605e5f13f7993c36360f818a2f58029f16a01ab968a89281c40aebf2dabb88`, same image and limits, removed as `oo-a317-nr-chain`. `linux/chain/driver-test.log` records 9 passed. `cargo-update-invoked.txt` is `no`. The ten bins and `pg-onboarding` phase exits are 0.

Inspect exit is 0. `live/run/outcome.json` status is `awaiting_review`. `source_cut` is 317, `target_fence` is 1, `mapping_run_generation` is 317. Config identity is `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`. `check-inspect` exit is 0.

`import-result.json` rows are party 2000, account 8000, entry 90000. Product canonical digests in `live/measurements/import.txt` are party `a5d41ec2bf132ab9f7c63ac9b33719c2cfb38849a5de087f3c2cb37218dcdcd7`, account `1b8bd1c8b99b141e93f15876d7220254cb7a1793697b485eaeeed65db6db0fc2`, entry `5e26275fb119aa3121ee25aaa5a33bcbd2273a1ea36080ee08ada7bfe4513429`. Those are the product hasher output. They are not the independent COPY gold.

Child exits 0 before the stop: `import-pg`, `init-pg`, `catalog-pg`, `captured-pg-draft`, `review-pg-draft`, `reviewed-policy-wide`, `reviewed-policy-narrow`, `role-pack-wide`, `role-pack-narrow`, `role-pack-bundle-wide`, `role-pack-bundle-narrow`, `reviewed-effective-oos`, `install-column-acl-wide`, `install-column-acl-narrow`, `provision-mapping-reader-wide`, `provision-mapping-reader-narrow`, `analyze-pg`, `query-wide-count`, `query-wide-null`. Bundle files `live/run/bundles/wide.json` and `narrow.json` were written by those children. Their digest check phase was not reached.

`analyze-pg.stdout` has `published` false and `estimate_is_exact` false. Estimated rows are party 2000, account 8000, entry 90000.

`import-not-ready` exited 1 with stderr `oo-pipeline: target schema is not ready`. `query-wide-before` exited 5 with stderr `query-pg: policy`. `query-wide-hidden`, `query-wide-insert`, and `query-wide-pos` exited 6 with stderr `query-pg: guard`.

Host epochs: inspect start 1791389228, inspect end 1791389301 (73 seconds), apply start 1791389345, apply end 1791389398 (53 seconds). One `docker stats --no-stream` after inspect: `cpu=0.07% mem=103.5MiB / 1GiB mem_perc=10.11% net=19.2MB / 16MB block=51.6MB / 117MB pids=6`. Identity budget defaults in `import.txt` are `connect_ms` 30000, `statement_ms` 60000, `lock_ms` 30000, `max_rows` 10000, `max_bytes` 8388608.

## Privilege and one cancellation

`linux/chain/watch.log` is `load_ok` then `priv_ok`. Writer exit 0. Output is `BEGIN`, `INSERT 0 1`, `ROLLBACK`. Reader exit 1. `reader.err` is `ERROR:  42501: permission denied for table party`. Sleep exit 1. `sleep.err` is `ERROR:  57014: canceling statement due to user request`. `pg_cancel_backend` returned `t`. `cancel.exit` is 0. `priv-container.exit` is 0. A second cancel was not issued.

## Not reached

`check-apply`, COPY compare, fresh query, numeric probe, pack digest, narrow queries, `wide-fk`, `wide-phone`, and restart have no exit file. `live/actual/` is empty. A passing trusted-query fixture was not produced. `ready_for_trusted_query` was not reached.

## Non-claims

Both outcome files keep `parent_acceptance` false, `development_unit_approval` false, `component_approval` false, `m1_i01` and `m1_i02` `not_accepted`, `semantic_publication` `missing`, `authentication` and `authorization` `not_claimed`, `public_auth` false, `natural_language` `missing`, `harness_mount` `not_performed`, `wal` `missing`, and `analyze_published` false. The caller in the operator config is `trusted_local_admin`. The product-wide full 87 parent goal is unchanged. a340 approval of the numeric producer was not a run of this 100000-row fixture.
