# Findings

Verdict: `APPROVE_LIMITED`. Scope is one trusted-operator run of the frozen 100000-row fixture on archive `6f87942f1c66ba1a9787b7d7194e58a507af63ef`.

## Proven on this run

- One private PostgreSQL 17.10 loaded `shop.party` 2000, `shop.account` 8000, and `shop.entry` 90000 from the driver-emitted `COPY` text. TLS SAN is `DNS:oo-a317-pg`. Autovacuum is off. No host port. Container name `oo-a349-pg`, alias `oo-a317-pg`.
- Source reader insert returned SQLSTATE 42501. Source writer insert of one party row committed nothing: `INSERT 0 1` then `ROLLBACK`.
- One cancellation returned SQLSTATE 57014. The watcher log records a single `priv_ok`, a single `copy_ok`, and a single `restart_ok`.
- Inspect sealed the catalog and stopped at `awaiting_review`. Config identity `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`. `source_cut` 317. `target_fence` 1. The comment marker is absent from the sealed catalog and captured draft.
- `import-not-ready` exited 1 with `oo-pipeline: target schema is not ready`.
- Default identity budget fields include `max_rows` 10000. The entry snapshot of 90000 rows still completed. The oracle records `budget_certificate=false`.
- Reviewed policy, role pack, column ACL, mapping-reader provision, and analyze exited 0 for both roles. Bundle digests stayed stable across restart: wide `38a4c9905b82f8bb6049a3dcf258a8573b7851f3b017565d5738eedfc407a3ba`, narrow `4a7948facd55a42c5e53c093ab8a4d93b2f156866378432a42c5d53095f7c0d1`. Analyze left `published` false and `estimate_is_exact` false.
- Apply exited 0. Status is `ready_for_trusted_query`. Hidden `shop.party.national_id` reason is `classification.absent`. Wide provision is `installed`. Narrow provision is `replay`. Caller is `trusted_local_admin`.
- `query-wide-count` returned int 90000. `query-wide-null` returned `p00000`, null, and true. `query-wide-qty` returned int8 min, int8 max, and 0. `query-wide-fk` returned account `0` and `p00000`. `query-wide-phone` returned `tel-00001`. `query-narrow-party` returned `p00001` and `n00001`.
- `query-wide-before` exited 5 with `query-pg: policy`. Hidden, insert, `_oo_pos`, narrow phone, narrow amount, and narrow `national_id` queries exited 6 with `query-pg: guard` and empty stdout.
- The amount probe for `entry_id = 3` exited 0 and returned decimal coefficient `12345678901234` at scale 4. The oracle `COPY` gold for that row is the text `1234567890.1234`.
- A fresh wide count of `shop.party` returned int 2000. After restart, that query stdout was byte-identical.
- `oracle/oracle.rs` recomputes the contract formulas without linking `pg-e2e`. It compared all 100000 target `COPY` lines and the driver-emitted fixture lines. `oracle_ok=true`. Sizes are 71528, 156574, and 2781472.
- Copied archive blobs match `git rev-parse` of `6f87942f1c66ba1a9787b7d7194e58a507af63ef`. `sql_guard.rs` is `6d7aef7bb95c9550a7e6f1bc7f3f22f7c098de4f`.

## Prior a317 seams, left in their own trees

The first a317 chain stopped at reviewed-policy semantic on archive `10ecd765`. The numeric-approved rerun stopped at `query-wide-qty` exit 6 on archive `21cc962`. This run, on `6f87942`, passed both of those children and continued through target `COPY`, the decimal probe, cancellation, and restart. Those older evidence trees were not rewritten.

## Preserved

- No gold line, decision, row count, source cut, or expected query exit was edited.
- The chain was started once. There is no failed-attempt directory in this review.
- The bounded-IN unit campaign and the numeric unit campaign were not repeated. The whole architecture matrix was not run.
- `note-probe` still writes `numeric_cell_set=null_bool_int_text`. The probe row decoded here is a decimal cell. Read `live/work/probe.stdout`.
- Password files, `ca.key`, and `server.key` stayed under `/tmp/oo-a349-eef831aa-dba4-4af2-a78b-541bc9e0a349/secrets`. A fixed-string scan of this evidence tree against each of those files was clean.
- No root manifest edit, no root lock edit, no `cargo update`, no product crate edit.

## Limits that stay open

- `m1_i01` and `m1_i02` stay `not_accepted`.
- Semantic publication stays `missing`.
- Public authentication stays false. Authentication and authorization stay `not_claimed`.
- Natural language stays `missing`.
- Harness mount stays `not_performed`.
- WAL stays `missing`.
- `analyze_published` stays false. `database_release` stays false.
- The caller is the fixture operator `trusted_local_admin`. The runtime membership grant is fixture SQL.
- The product-wide full 87 parent goal is unchanged.
- `ready_for_trusted_query` does not close those gates.
