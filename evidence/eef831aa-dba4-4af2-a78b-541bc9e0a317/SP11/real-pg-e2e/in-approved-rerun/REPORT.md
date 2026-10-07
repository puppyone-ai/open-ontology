# IN-approved rerun

Session `eef831aa-dba4-4af2-a78b-541bc9e0a317`. Product archive `6f87942f1c66ba1a9787b7d7194e58a507af63ef`, parent `149944271ef1c2cb555954d65248413df2166f1c`, subject `gateway: admit a bounded literal IN list`. a347 recorded `APPROVE_LIMITED` for that commit. The commit message says the predicate stays `IN` or `NOT IN`, with 1 to 32 scalar literals, and that the commit does not accept SP06-W02, M1-I01, M1-I02, or the a317 goal. This report does not accept a parent unit, M1-I01, M1-I02, semantic publication, public authentication, the harness, runsc, or a release.

The original apply seam on `10ecd7654fd7939b6655a0e50a84214a7fca316e` stays in the parent evidence tree. The `query-wide-qty` seam on `21cc96241a841b47dbbcb116549030e38a643ba0` stays in `numeric-approved-rerun`. Owned fixture and gold under `testing/real-db/pg-e2e` are the tree `79e2ddc120b2117f90d3e79ff61571490243f23b` from commit `f4381843c7f376f196bc30c78c18d9701dc29a6d`. That tree is also `HEAD:testing/real-db/pg-e2e`. `21cc96241a841b47dbbcb116549030e38a643ba0` is an ancestor of this archive. `Cargo.lock` and `Cargo.toml` have an empty diff between those two commits. `tools/pg-onboarding` tree `b0e6fd768c8d919df3b8d6683dd44d444608c78c` is unchanged.

## Result

Linux smoke of the archive exited 0. The focused owned-driver run exited 0. The one live chain exited 0.

`linux/chain/chain.exit` is 0. `linux/chain/docker-run.exit` is 0. `linux/chain/phases/apply.exit` is 0. `live/work/apply.stderr` and `live/work/apply.stdout` are empty. `live/run/steps/query-wide-qty.exit` is 0. Its stderr is empty. The statement is the frozen `SELECT entry_id, qty FROM shop.entry WHERE entry_id IN (0, 1, 2) ORDER BY entry_id`. The three returned cells are int `0` with int `-9223372036854775808`, int `1` with int `9223372036854775807`, and int `2` with int `0`. Summary status is `succeeded`, `truncated` false, `returned_rows` 3, `rollback_confirmed` true. `live/run/audit/wide-qty.jsonl` exists.

`live/run/apply-outcome.json` status is `ready_for_trusted_query`. `analyze_published` is false. `estimate_is_exact` is false. `database_state` is `child_compilers_applied_no_release`. `database_rollback` is false. `database_release` is false. Wide provision effect is `installed`. Narrow provision effect is `replay`. Hidden column is `shop.party.national_id` with reason `classification.absent`. `schema_hash` is `eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74`. `source_cut` is 317, `target_fence` is 1, `mapping_run_generation` is 317. Config identity is `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`.

`ready_for_trusted_query` is the product status on this operator config. The same outcome file keeps `m1_i01` and `m1_i02` at `not_accepted`.

## Archive delta that this chain compiled

`source/blobs/sql_guard.rs` is the archive file. `git hash-object` of the copy is `6d7aef7bb95c9550a7e6f1bc7f3f22f7c098de4f`. Byte size is 218901. The previous archive blob, recorded in `numeric-approved-rerun/source/BLOBS.txt`, is `1db8f3ed11a2d816d485bdd2865f37aff3e5cbed`.

`admit_op` calls `admit_in` when `kind` is `AEXPR_IN`. `admit_in` requires a column on the left, a list of 1 to 32 `AConst` literals on the right, and an operator token `=` or `<>`. `admit_in_literal` resolves that same-type operator. Columns, functions, casts, parameters, subqueries, array expressions, `ANY`/`ALL`, and unary operators still return `GuardError::Unsupported`. `IN_LIST_LIMIT` is 32. This fixture uses three integer literals. This line did not rerun the a347 bounded-IN campaign.

`reviewed_policy.rs` blob `8f723806217954ad264fa45970dc1ddd9ea94fe9`, `query_executor.rs` blob `fc3a65254881c62f8321bc01e405d6b3d45dabb1`, and `query-pg.rs` blob `f404eb1879cdfbbcc3c3a64547c10a40939402ed` match the numeric-approved blob list. `git diff --stat` from `21cc962` to `6f87942` under `crates/oo-gateway` also changes `pack_catalog.rs`, `tests/bounded_in.rs`, and `tests/http_gateway_pg_live.rs`. The chain compiled the whole archive. It did not isolate those other diffs, and it did not run the HTTP live test or the bounded-IN unit file.

## What completed

PostgreSQL 17.10, one private container `oo-a317-pg`, id `234f4fc05b45ac0a8843a2287d33a5a1164651603e459c56a4c1ac0185daf804`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/server.txt` records `server_version=17.10 (Debian 17.10-1.pgdg12+1)`, `autovacuum=off`, `ssl=true`, and `COPY 2000`, `COPY 8000`, `COPY 90000`. Certificate subject `CN=oo-a317-pg`, SAN `DNS:oo-a317-pg`. Memory 1073741824, 1 CPU, privileged false, network `oo-a317-eef831aa-net`, cap-drop ALL, cap-add `CAP_CHOWN`, `CAP_DAC_OVERRIDE`, `CAP_FOWNER`, `CAP_SETGID`, `CAP_SETUID`. `pg/ports.json` is `{}`. `pg/up.exit` is 0.

Smoke builder `4404574a5952d15ed73f0541563b37716869aa25d4bbfecc56548789e5cfaa82`, removed as `oo-a317-in-smoke`. Focused builder `a30742ffa231c12c0069a163d387beb972b8feb1010dd5f69abbe6ba62411422`, removed as `oo-a317-in-focused`. Chain builder `53e8164939c2cdbdc41d72cb13603ad0a823716407ed11a33b147d5f6e12e39e`. All three: 6442450944 bytes, 1 CPU, cap-drop ALL, privileged false, OOMKilled false, pinned Rust image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Toolchain `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Each `cargo-update-invoked.txt` is `no`.

`linux/smoke/test-count.txt` is `passed=24 failed=0 ignored=1`. The ignored test is `live_chain`. Clippy on `pg-onboarding` finished and `clippy.exit` is 0. `overlay-absent.txt` is `overlay=absent`. `tool-tree.txt` is `b0e6fd768c8d919df3b8d6683dd44d444608c78c`. `fixed-commit.txt` is `6f87942f1c66ba1a9787b7d7194e58a507af63ef`.

`linux/focused/test-count.txt` is `passed=9 failed=0 ignored=0`. Clippy on `pg-e2e` finished and `clippy.exit` is 0. `linux/chain/driver-test.log` records the same 9 passed. The focused command was the owned driver. It was not the a347 guard campaign.

Inspect exit is 0. `live/run/outcome.json` status is `awaiting_review`. `check-inspect` exit is 0. The marker `甲注评` is absent from `catalog.json`, `captured-draft.json`, `inspect-manifest.json`, `outcome.json`, and `apply-outcome.json`.

`import-result.json` rows, copied into `live/measurements/import.txt`, are party 2000, account 8000, entry 90000. Product canonical digests are party `a5d41ec2bf132ab9f7c63ac9b33719c2cfb38849a5de087f3c2cb37218dcdcd7`, account `1b8bd1c8b99b141e93f15876d7220254cb7a1793697b485eaeeed65db6db0fc2`, entry `5e26275fb119aa3121ee25aaa5a33bcbd2273a1ea36080ee08ada7bfe4513429`. Those are the product hasher output. The independent check is COPY text against `gold::line_at`.

Child exits 0 through apply: `import-pg`, `init-pg`, `catalog-pg`, `captured-pg-draft`, `review-pg-draft`, `reviewed-policy-wide`, `reviewed-policy-narrow`, `role-pack-wide`, `role-pack-narrow`, `role-pack-bundle-wide`, `role-pack-bundle-narrow`, `reviewed-effective-oos`, `install-column-acl-wide`, `install-column-acl-narrow`, `provision-mapping-reader-wide`, `provision-mapping-reader-narrow`, `analyze-pg`, `query-wide-count`, `query-wide-null`, `query-wide-qty`, `query-wide-fk`, `query-wide-phone`, `query-narrow-party`. `check-apply` exit is 0.

`analyze-pg.stdout` has `published` false and `estimate_is_exact` false. Estimated rows are party 2000, account 8000, entry 90000.

Required negative children, checked by the driver: `import-not-ready` exit 1, stderr `oo-pipeline: target schema is not ready`. `query-wide-before` exit 5, stderr `query-pg: policy`. `query-wide-hidden`, `query-wide-insert`, `query-wide-pos`, `query-narrow-hidden`, `query-narrow-phone`, and `query-narrow-amount` exit 6, stderr `query-pg: guard`. Those six have empty stdout and no audit file.

Positive query cells:

- `query-wide-count`: int `90000`.
- `query-wide-null`: text `p00000`, null, bool true.
- `query-wide-fk`: int `0`, text `p00000`.
- `query-wide-phone`: text `tel-00001`.
- `query-narrow-party`: text `p00001`, text `n00001`.

Pack digests, read twice and compared by `cmp`: wide `38a4c9905b82f8bb6049a3dcf258a8573b7851f3b017565d5738eedfc407a3ba`, narrow `4a7948facd55a42c5e53c093ab8a4d93b2f156866378432a42c5d53095f7c0d1`. `pack-wide.exit` and `pack-narrow.exit` are 0.

Target COPY from the physical tables, ordered by primary key, matches the Rust gold line for line. `check-copy-party`, `check-copy-account`, and `check-copy-entry` are 0. `cmp` of `live/actual/*.copy` against `live/fixture/*.copy` is identical. Sizes are party 71528, account 156574, entry 2781472 bytes. The three `.err` files are empty. That COPY text includes `national_id`, int8 min and max, the exact `numeric(18,4)` amounts, nulls, and the text foreign keys.

Fresh `SELECT count(*) FROM shop.party` returned int `2000` before restart and again after restart. Both summaries are `succeeded`, `truncated` false, `rollback_confirmed` true. `check-fresh` and `check-fresh-restart` are 0. Audits exist for `fresh-party` and `fresh-party-restart`.

Numeric probe `SELECT amount FROM shop.entry WHERE entry_id = 3` exit is 0. The row cell is `{"kind":"decimal","negative":false,"coefficient":"12345678901234","scale":4}`. That is the gold amount `1234567890.1234`. Summary status is `succeeded`, `returned_rows` 1, `rollback_confirmed` true. `live/measurements/probe.txt` records `probe_exit=0` and `probe_stdout=row`. Its third line, `numeric_cell_set=null_bool_int_text`, is the static string from `note_probe`. The decoded cell is the decimal object in `live/work/probe.stdout`.

Host epochs: inspect start 1791397190, inspect end 1791397263 (73 seconds), apply start 1791397306, apply end 1791397360 (54 seconds), host after inspect 1791397265, host after restart 1791397364. One `docker stats --no-stream` after inspect: `cpu=0.10% mem=57.71MiB / 1GiB mem_perc=5.64% net=19.2MB / 16MB block=0B / 114MB pids=6`. Identity budget defaults in `import.txt` are `connect_ms` 30000, `statement_ms` 60000, `lock_ms` 30000, `max_rows` 10000, `max_bytes` 8388608. The entry snapshot of 90000 rows completed with those recorded defaults. This directory does not treat that text as a budget certificate.

## Privilege, one cancellation, restart

`linux/chain/watch.log` is `load_ok`, `priv_ok`, `copy_ok`, `restart_ok`. Writer exit 0. Output is `BEGIN`, `INSERT 0 1`, `ROLLBACK`. Reader exit 1. `reader.err` is `ERROR:  42501: permission denied for table party`. Sleep exit 1. `sleep.err` is `ERROR:  57014: canceling statement due to user request`. `pg_cancel_backend` returned `t`. `cancel.exit` is 0. `priv-container.exit` is 0. A second cancel was not issued. `pg/restart.id` is `oo-a317-pg`. `pg/restart.err` is empty.

## Non-claims

Both outcome files keep `parent_acceptance` false, `development_unit_approval` false, `component_approval` false, `m1_i01` and `m1_i02` `not_accepted`, `semantic_publication` `missing`, `authentication` and `authorization` `not_claimed`, `public_auth` false, `natural_language` `missing`, `harness_mount` `not_performed`, `wal` `missing`, and `analyze_published` false. The caller in the operator config is `trusted_local_admin`. `role_login_binding` is `configuration_not_auth`. The product-wide full 87 parent goal is unchanged. a347 approval of the bounded literal `IN` producer was not this 100000-row run. This run is the 100000-row run, and it still leaves those parent fields unaccepted.
