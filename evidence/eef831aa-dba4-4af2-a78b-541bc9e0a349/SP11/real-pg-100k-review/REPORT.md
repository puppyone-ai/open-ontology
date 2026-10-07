# Independent 100k PostgreSQL review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a349`. Verdict: `APPROVE_LIMITED` for one trusted-operator chain of the unchanged 100000-row fixture on product archive `6f87942f1c66ba1a9787b7d7194e58a507af63ef`.

This review does not accept a new child, a parent unit, M1-I01, M1-I02, semantic publication, public authentication, natural language, the harness, WAL, runsc, or a release.

## Packets read before this run

- Frozen contract `testing/real-db/pg-e2e/CONTRACT.md`.
- a317 first chain on `10ecd7654fd7939b6655a0e50a84214a7fca316e`: inspect reached `awaiting_review`; apply stopped at `reviewed-policy-wide` exit 11, stderr `reviewed-policy: semantic`. Target COPY was not reached.
- a317 `numeric-approved-rerun` on `21cc96241a841b47dbbcb116549030e38a643ba0`: reviewed policy, packs, ACL, and analyze passed; `query-wide-qty` exited 6, stderr `query-pg: guard`. Target COPY was not reached.
- a317 `in-approved-rerun` on `6f87942f1c66ba1a9787b7d7194e58a507af63ef`: the author recorded a full chain through restart. This review did not copy those live receipts. It ran a new private database and compared the target `COPY` text to an independent formula.

Fixture commit `f4381843c7f376f196bc30c78c18d9701dc29a6d`, tree `79e2ddc120b2117f90d3e79ff61571490243f23b`, matches `HEAD:testing/real-db/pg-e2e`. The driver and fixture SQL were mounted read-only. No gold line, decision, row count, or expected exit was edited.

## Result

Linux smoke of the archive exited 0. Focused driver tests and Clippy exited 0. The one live chain exited 0. The independent oracle exited 0 (`oracle/result.txt`, `oracle_ok=true`).

`linux/chain/chain.exit` is 0. `linux/chain/docker-run.exit` is 0. `linux/chain/phases/apply.exit` is 0. `linux/chain/phases/oracle.exit` is 0. All 56 chain phase exits are 0. `live/work/apply.stderr` and `live/work/apply.stdout` are empty. `linux/chain/watch.log` is `load_ok`, `priv_ok`, `copy_ok`, `restart_ok`.

`live/run/apply-outcome.json` status is `ready_for_trusted_query`. `analyze_published` is false. `estimate_is_exact` is false. `database_state` is `child_compilers_applied_no_release`. `database_rollback` is false. `database_release` is false. Wide provision effect is `installed`. Narrow provision effect is `replay`. Hidden column is `shop.party.national_id` with reason `classification.absent`. `schema_hash` is `eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74`. `source_cut` is 317. `target_fence` is 1. `mapping_run_generation` is 317. Config identity is `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`.

`ready_for_trusted_query` is the product status on this operator config. The same outcome file keeps `m1_i01` and `m1_i02` at `not_accepted`. The caller in `live/work/inspect.json` and `live/work/apply.json` is `trusted_local_admin`.

## What this chain compiled

Product bytes are `git archive` of `6f87942f1c66ba1a9787b7d7194e58a507af63ef`. Dirty worktree files were not the compiler source. `source/blobs/sql_guard.rs` matches git blob `6d7aef7bb95c9550a7e6f1bc7f3f22f7c098de4f`, 218901 bytes. `reviewed_policy.rs` blob `8f723806217954ad264fa45970dc1ddd9ea94fe9`, `query_executor.rs` blob `fc3a65254881c62f8321bc01e405d6b3d45dabb1`, and `query-pg.rs` blob `f404eb1879cdfbbcc3c3a64547c10a40939402ed` match `git rev-parse` of that commit. `tools/pg-onboarding` tree is `b0e6fd768c8d919df3b8d6683dd44d444608c78c`. `Cargo.lock` and `Cargo.toml` have an empty diff from `21cc96241a841b47dbbcb116549030e38a643ba0` to this archive.

The chain compiled the archive and the owned driver. It did not rerun the bounded-IN unit file or the numeric-policy unit campaign.

## Resources

PostgreSQL 17.10, container `oo-a349-pg`, id `7429ceedd711efe4bd957946e298a28272c04e6ee71cdb62141ec717bc638305`, network alias `oo-a317-pg`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/server.txt` records `server_version=17.10 (Debian 17.10-1.pgdg12+1)`, `autovacuum=off`, `ssl=true`, and `COPY 2000`, `COPY 8000`, `COPY 90000`. Certificate subject `CN=oo-a317-pg`, SAN `DNS:oo-a317-pg`. Memory 1073741824, 1 CPU, privileged false, network `oo-a349-eef831aa-net`, cap-drop ALL, cap-add `CAP_CHOWN`, `CAP_DAC_OVERRIDE`, `CAP_FOWNER`, `CAP_SETGID`, `CAP_SETUID`. `pg/ports.json` is `{}`. `pg/up.exit` is 0.

The frozen driver hardcodes host `oo-a317-pg` and the `oo_a317_*` roles. Those names stayed inside the fixture. The container name, network, volumes, labels, tmp, and evidence directory are a349-private.

Smoke builder `5ad3a8239c123cc0e57be4ed69859f56eea556a1b3a89ecb5c0bfd1b427ba12d`, removed as `oo-a349-smoke`. Focused builder `b21a2b92bbf449ef5975f756b2f505cc3599aa682c266bcc1aac25e13b941eb2`, removed as `oo-a349-focused`. Chain builder `27d8fa820192d790c4a95e63d2ffaa315344e5bd3b276506d57b6e96d786db51`, removed as `oo-a349-chain`. All three: 6442450944 bytes, 1 CPU, cap-drop ALL, privileged false, OOMKilled false, pinned Rust image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Toolchain `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Each `cargo-update-invoked.txt` is `no`.

`linux/smoke/test-count.txt` is `passed=24 failed=0 ignored=1`. The ignored test is `live_chain`. Clippy on `pg-onboarding` finished and `clippy.exit` is 0. `overlay-absent.txt` is `overlay=absent`. `tool-tree.txt` is `b0e6fd768c8d919df3b8d6683dd44d444608c78c`. `fixed-commit.txt` is `6f87942f1c66ba1a9787b7d7194e58a507af63ef`.

`linux/focused/test-count.txt` is `passed=9 failed=0 ignored=0`. Clippy on `pg-e2e` finished and `clippy.exit` is 0. `linux/chain/driver-test.log` records the same 9 passed.

## Chain observations

Inspect exit is 0. `live/run/outcome.json` status is `awaiting_review`. The marker `甲注评` is absent from `catalog.json`, `captured-draft.json`, `inspect-manifest.json`, `outcome.json`, and `apply-outcome.json`.

`import-result.json` rows, copied into `live/measurements/import.txt`, are party 2000, account 8000, entry 90000. Product canonical digests are party `a5d41ec2bf132ab9f7c63ac9b33719c2cfb38849a5de087f3c2cb37218dcdcd7`, account `1b8bd1c8b99b141e93f15876d7220254cb7a1793697b485eaeeed65db6db0fc2`, entry `5e26275fb119aa3121ee25aaa5a33bcbd2273a1ea36080ee08ada7bfe4513429`. Those are the product hasher output. The independent check is the oracle formula against the target `COPY` bytes.

Child exits 0 through the positive queries: `import-pg`, `init-pg`, `catalog-pg`, `captured-pg-draft`, `review-pg-draft`, `reviewed-policy-wide`, `reviewed-policy-narrow`, `role-pack-wide`, `role-pack-narrow`, `role-pack-bundle-wide`, `role-pack-bundle-narrow`, `reviewed-effective-oos`, `install-column-acl-wide`, `install-column-acl-narrow`, `provision-mapping-reader-wide`, `provision-mapping-reader-narrow`, `analyze-pg`, `query-wide-count`, `query-wide-null`, `query-wide-qty`, `query-wide-fk`, `query-wide-phone`, `query-narrow-party`.

`analyze-pg.stdout` has `published` false, `estimate_is_exact` false, and estimated rows 2000, 8000, and 90000.

`import-not-ready` exited 1 with `oo-pipeline: target schema is not ready`. `query-wide-before` exited 5 with `query-pg: policy`. `query-wide-hidden`, `query-wide-insert`, `query-wide-pos`, `query-narrow-hidden`, `query-narrow-phone`, and `query-narrow-amount` exited 6 with `query-pg: guard`, empty stdout, and no weakening of the expected exit.

Positive query cells, read by the oracle from this run's stdout:

- `query-wide-count`: int `90000`.
- `query-wide-null`: text `p00000`, null, bool true.
- `query-wide-qty`: int `0` with int `-9223372036854775808`, int `1` with int `9223372036854775807`, int `2` with int `0`.
- `query-wide-fk`: int `0`, text `p00000`.
- `query-wide-phone`: text `tel-00001`.
- `query-narrow-party`: text `p00001`, text `n00001`.

Pack digests were stable across the restart: wide `38a4c9905b82f8bb6049a3dcf258a8573b7851f3b017565d5738eedfc407a3ba`, narrow `4a7948facd55a42c5e53c093ab8a4d93b2f156866378432a42c5d53095f7c0d1`.

Target `COPY` of the physical tables, ordered by primary key, matches the independent oracle for every party, account, and entry row. The driver-emitted fixture `COPY` matches the same formula. Sizes are party 71528, account 156574, entry 2781472 bytes. The three `.err` files are empty. That text includes `national_id`, int8 min and max, the exact `numeric(18,4)` amounts, nulls, and the foreign keys.

Fresh `SELECT count(*) FROM shop.party` returned int `2000`. After restart of the same volume, the stdout bytes of that statement were identical, including int `2000`.

Numeric probe `SELECT amount FROM shop.entry WHERE entry_id = 3` exit is 0. The row cell is `{"kind":"decimal","negative":false,"coefficient":"12345678901234","scale":4}`. That is the gold amount `1234567890.1234`. `live/measurements/probe.txt` records `probe_exit=0` and `probe_stdout=row`. Its third line, `numeric_cell_set=null_bool_int_text`, is the static string from the driver `note-probe`. The decoded cell is in `live/work/probe.stdout`.

Host epochs: inspect start 1791400517, inspect end 1791400589 (72 seconds), apply start 1791400634, apply end 1791400688 (54 seconds), host after inspect 1791400591, host after restart 1791400692. One `docker stats --no-stream` after inspect: `cpu=0.08% mem=103.4MiB / 1GiB mem_perc=10.09% net=19.2MB / 16MB block=51.5MB / 114MB pids=6`. Identity budget defaults in `import.txt` are `connect_ms` 30000, `statement_ms` 60000, `lock_ms` 30000, `max_rows` 10000, `max_bytes` 8388608. The entry snapshot of 90000 rows completed with those recorded defaults. `budget_certificate=false` in the oracle result. The 10000-row field is a measurement of the built-in default. It did not cap this load.

## Privilege, one cancellation, restart

Writer exit 0. Output is `BEGIN`, `INSERT 0 1`, `ROLLBACK`. Reader exit 1. `reader.err` contains `42501` and `permission denied for table party`. Sleep exit recorded `57014`. `pg/restart.id` is `oo-a349-pg`.

## Non-claims

Both outcome files keep `parent_acceptance` false, `development_unit_approval` false, `component_approval` false, `m1_i01` and `m1_i02` `not_accepted`, `semantic_publication` `missing`, `authentication` and `authorization` `not_claimed`, `public_auth` false, `natural_language` `missing`, `harness_mount` `not_performed`, `wal` `missing`, `analyze_published` false, and `role_login_binding` `configuration_not_auth`. The product-wide full 87 parent goal is unchanged.
