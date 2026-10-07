# Real PostgreSQL fixture

Session `eef831aa-dba4-4af2-a78b-541bc9e0a317`. Fixed product archive `10ecd7654fd7939b6655a0e50a84214a7fca316e`. Owned paths are `testing/real-db/pg-e2e/**` and this evidence directory. This report does not accept a parent unit, M1-I01, M1-I02, semantic publication, public authentication, the harness, runsc, or a release.

## Result

Inspect of the public fixture reached `awaiting_review`. Apply stopped at `reviewed-policy`.

`linux/chain/chain.exit` is 1. `linux/chain/docker-run.exit` is 1. `linux/chain/phases/apply.exit` is 1. `live/work/apply.stderr` is `pg-onboarding: reviewed-policy-wide exit`. `live/work/apply.stdout` is empty. `live/run/steps/reviewed-policy-wide.exit` is 11. `live/run/steps/reviewed-policy-wide.stderr` is `reviewed-policy: semantic`. That stdout is empty. `live/run/apply-outcome.json` has `status` `failed`, `step` `reviewed-policy-wide`, `detail` `exit`, `database_state` `changes_may_have_been_applied`, `database_rollback` false, and `database_release` false.

The chain was not rerun. The frozen decision table and the `numeric(18,4)` amount column were left as written.

## Blocking seam

Archive `crates/oo-gateway/src/reviewed_policy.rs` blob `95e68417e6fe1be024123e3aa2b7c2301402eb21`. A copy is `source/blobs/reviewed_policy.rs`.

`admit_all` walks `binding.allow` only. The wide allow list has 13 columns. `shop.party.national_id` is absent from it. The effective command hides that column with reason `classification.absent`. The compiler does not call `column_spec` for it.

`shop.entry.amount` is on the allow list. Its decision is semantic `quantity` and pii `none`. The catalog column is `type_name` `numeric`, `typtype` `b`, `type_oid` 1700. `physical_type` returns `ReviewedPolicyError::Semantic` when `base_type` rejects the name. `base_type` admits only `bool`, `int2`, `int4`, `int8`, and `text` (lines 904-905 and 913-915).

`compatible` does admit `SemanticType::Quantity` for `int8` (lines 931-938). `shop.entry.qty` is `int8` with semantic `quantity`. That pair is not the rejecting column. The first allow-list column whose catalog type is outside `base_type` is `shop.entry.amount`.

`crates/oo-gateway/src/bin/reviewed-policy.rs` blob `34f7824250c39a58f177e70f21dc6db02b5527bd` maps `Semantic` to exit 11 and the stderr line `reviewed-policy: semantic` (lines 36 and 261). The child stderr matches that static line.

## What completed

PostgreSQL 17.10, one private container `oo-a317-pg`, id `c095dab760fa0dc3272d8ffaa5c8678ab2578456039492597dc94f796d9caf82`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `server_version=17.10 (Debian 17.10-1.pgdg12+1)`, `autovacuum=off`, `ssl=true`. Server certificate subject `CN=oo-a317-pg`, SAN `DNS:oo-a317-pg`. Memory 1073741824, 1 CPU, pids-limit 256, capabilities dropped then `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID` added. No published port. `pg/up.exit` is 0.

Source load printed `COPY 2000`, `COPY 8000`, and `COPY 90000`. `linux/chain/phases/load-wait.exit` is 0. Membership watch printed `membership_ready`.

The chain builder compiled the ten archive bins and `pg-onboarding`. Each of those phase exits is 0. `cargo-update-invoked.txt` is `no`. Toolchain text is `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Container `5f3a2f26191e581accb78f620d39bfc2f35f70d3858095699a9ec8c99d2c6da2`, 6 GiB, 1 CPU, cap-drop ALL, OOMKilled false, then removed.

Inspect exit is 0. `live/run/outcome.json` status is `awaiting_review`. `source_cut` is 317, `target_fence` is 1, `mapping_run_generation` is 317. Config identity is `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`, the same digest as `binding.config_digest` on the policy stdin. `check-inspect` exit is 0. Comment marker `甲注评` is absent from `catalog.json` and `captured-draft.json`.

`import-result.json` rows are party 2000, account 8000, entry 90000. Product canonical digests are recorded in `live/measurements/import.txt`. Those digests are the product hasher output. They are not the independent COPY gold. `live/actual/` is empty because the COPY compare did not run.

`import-not-ready` exited 1 with stderr `oo-pipeline: target schema is not ready`. Archive `tools/pg-onboarding/src/inspect.rs` treats exit 0 as `already_installed` and requires that stderr phrase. Inspect then ran `init-pg` and `import-pg`, both exit 0, and the inspect command itself exited 0.

Host epochs: inspect start 1791371383, inspect end 1791371456 (73 seconds), apply start 1791371498, apply end 1791371547 (49 seconds). One `docker stats --no-stream` after inspect: `cpu=0.06% mem=57.92MiB / 1GiB mem_perc=5.66% net=19.2MB / 16MB block=0B / 113MB pids=6`. `import.txt` also records the identity budget defaults `connect_ms` 30000, `statement_ms` 60000, `lock_ms` 30000, `max_rows` 10000, `max_bytes` 8388608. The 90000-row entry snapshot completed with those defaults. This file is a measurement record.

## Privilege and one cancellation

The host watcher ran `priv.sh` once after `inspect.done`. `linux/chain/watch.log` is `load_ok` then `priv_ok`.

Writer exit 0. Output is `BEGIN`, `INSERT 0 1`, `ROLLBACK`. Reader exit 1. `reader.err` is `ERROR:  42501: permission denied for table party`. Sleep exit 1. `sleep.err` is `ERROR:  57014: canceling statement due to user request`. `pg_cancel_backend` returned `t`. `cancel.exit` is 0. `priv-container.exit` is 0. A second cancel was not issued.

## Not reached

`inside.sh` records `apply` and then exits because that phase is 1. These phases have no exit file: `check-apply`, `copy-sql-*`, `check-copy-*`, `fresh`, `probe`, `note-probe`, pack digest, restart. No `live/run/bundles/`, no `live/run/queries/`, no `live/run/audit/`. `query-pg` was built and was not executed. Positive reads, hidden-column guard exits, and the write-guard query exits were not observed. The restart smoke was not observed.

The same archive's `query_executor.rs` `oid_allowed` admits only bool, int8, int2, int4, and text. Its unit test asserts `Type::NUMERIC` is refused, and `TypeRefused { oid: 1700 }` maps to `StatementNotAllowed`. The numeric probe that would have recorded that live exit was not started. A passing trusted-query fixture was not produced. `ready_for_trusted_query` was not reached.

## Builders before this chain

`linux/smoke` exit 0. Archived `pg-onboarding` tests: `passed=24 failed=0 ignored=1`. Container `57a26e1516019cb73f00c90542ffaae4d2e222cc05c80e467df8b7f5e5cce852` removed. 6 GiB, 1 CPU, cap-drop ALL.

`linux/focused` exit 0. Driver tests: `passed=9 failed=0 ignored=0`. Clippy exit 0. Container `1c1105e70cae4a1dfa2a594f547a958fbeb3110e9912ebf3bfbc85f96f544d56` removed. Same limits.

Preserved driver failures, docker-run exit 101: `focused-attempt-1` and `focused-attempt-2` and `focused-attempt-3` failed in `cargo test`; `focused-attempt-4` and `focused-attempt-5` passed tests and failed Clippy. Those trees were kept.

Recipe origins and hashes are `origin/recipes.txt`. `pg/pg-wrap.sh` and `tools/sha256.rs`, `tools/sha256_manifest.rs`, and `tools/select-debs.rs` are the reviewed byte copies named there.

## Non-claims on both outcome files

`parent_acceptance` false. `development_unit_approval` false. `component_approval` false. `m1_i01` and `m1_i02` are `not_accepted`. `semantic_publication` is `missing`. `authentication` and `authorization` are `not_claimed`. `public_auth` false. `natural_language` is `missing`. `harness_mount` is `not_performed`. `wal` is `missing`. `analyze_published` false. `mapping_rows_read` false. `config_identity` is `recomputed_from_non_secret_request_fields`. `caller` in the operator config is `trusted_local_admin`. The runtime membership grant is fixture SQL. The product-wide full 87 parent goal is unchanged.
