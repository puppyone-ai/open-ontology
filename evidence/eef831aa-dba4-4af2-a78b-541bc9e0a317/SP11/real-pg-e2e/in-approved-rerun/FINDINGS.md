# Findings

## Proven on this rerun

- One private PostgreSQL 17.10 loaded `shop.party` 2000, `shop.account` 8000, and `shop.entry` 90000. TLS SAN is `DNS:oo-a317-pg`. Autovacuum is off. No host port.
- Source reader insert returned SQLSTATE 42501. Source writer insert of one party row committed nothing: `INSERT 0 1` then `ROLLBACK`.
- One cancellation returned SQLSTATE 57014. `pg_cancel_backend` returned `t`. The watcher log records a single `priv_ok`, a single `copy_ok`, and a single `restart_ok`.
- Inspect sealed `catalog.json`, `captured-draft.json`, and `import-result.json`, then stopped at `awaiting_review`. Config identity `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`. `source_cut` 317 and `target_fence` 1. `check-inspect` exit is 0. The comment marker is absent from the sealed catalog and captured draft.
- `import-not-ready` exited 1 with `oo-pipeline: target schema is not ready`. The inspect command then exited 0.
- Default identity budget fields include `max_rows` 10000. The entry snapshot of 90000 rows still completed. The recorded fields are a measurement, not a certificate.
- `reviewed-policy-wide` and `reviewed-policy-narrow` exited 0. Archive `reviewed_policy.rs` blob `8f723806217954ad264fa45970dc1ddd9ea94fe9` is the same blob as the numeric-approved archive.
- Role pack, column ACL, and mapping-reader provision for wide and narrow exited 0. Bundle digests stayed stable across the two reads: wide `38a4c9905b82f8bb6049a3dcf258a8573b7851f3b017565d5738eedfc407a3ba`, narrow `4a7948facd55a42c5e53c093ab8a4d93b2f156866378432a42c5d53095f7c0d1`. `analyze-pg` exited 0 with `published` false, `estimate_is_exact` false, and estimated rows 2000, 8000, 90000.
- Apply exited 0. Status is `ready_for_trusted_query`. Hidden `shop.party.national_id` reason is `classification.absent`. Wide provision is `installed`. Narrow provision is `replay`.
- `query-wide-count` returned int 90000. `query-wide-null` returned `p00000`, null, and true. `query-wide-qty` returned the three int8 rows `-9223372036854775808`, `9223372036854775807`, and `0` for `entry_id` 0, 1, and 2. `query-wide-fk` returned account `0` and `p00000`. `query-wide-phone` returned `tel-00001`. `query-narrow-party` returned `p00001` and `n00001`. Audits exist for those statements and for the fresh counts and the amount probe.
- `query-wide-before` exited 5 with `query-pg: policy`. Hidden, insert, `_oo_pos`, narrow phone, narrow amount, and narrow `national_id` queries exited 6 with `query-pg: guard`, empty stdout, and no audit file.
- Target `COPY` text matches `gold::line_at` for every party, account, and entry row. The on-disk fixture COPY files and the target COPY files are byte-identical. Sizes are 71528, 156574, and 2781472.
- The amount probe for `entry_id = 3` exited 0 and returned decimal coefficient `12345678901234` at scale 4. The COPY gold for that row is the text `1234567890.1234`.
- A fresh wide count of `shop.party` returned int 2000, then the same container was restarted, and the same statement returned int 2000 again.

## Cleared seam

The numeric-approved rerun stopped at `query-wide-qty` because archive `sql_guard.rs` blob `1db8f3ed11a2d816d485bdd2865f37aff3e5cbed` refused `AexprIn`. This archive blob `6d7aef7bb95c9550a7e6f1bc7f3f22f7c098de4f` admits `AEXPR_IN` through `admit_in` for a column and 1 to 32 scalar literals. The unchanged statement returned the three gold qty values. `query-pg` exit for that child is 0, and `wide-qty.jsonl` was written.

The parent evidence seam, exit 11 `reviewed-policy: semantic` on archive `10ecd765`, remains the first chain. Reviewed policy on this commit exited 0 for both roles, and the amount cell was decoded.

## Preserved

- Parent evidence for archive `10ecd765` stays in place, including its `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, and `cleanup.log`.
- `numeric-approved-rerun` stays in place, including the qty exit 6, the empty qty audit of that run, and its reports.
- No gold line, decision, row count, source cut, or expected query exit was edited. The chain was not started a second time. The driver tree was not edited.
- `crates/oo-gateway` at `6f87942` also differs from `21cc962` in `pack_catalog.rs`, `tests/bounded_in.rs`, and `tests/http_gateway_pg_live.rs`. This line compiled that archive and did not review those diffs as a separate unit. The two pack digests above are the bytes this chain's `role-pack-bundle` wrote.

## Limits

- The immutable `tools/pg-onboarding/tests/live_chain.rs` still hardcodes generation 48. This fixture uses generation 317 and the driver under `testing/real-db/pg-e2e`. That driver was not edited for this rerun. Chain `driver-test` recorded 9 passed. Smoke left `live_chain` ignored.
- `note_probe` always writes `numeric_cell_set=null_bool_int_text`. The probe row that this run decoded is a decimal cell. Read `live/work/probe.stdout` for the cell.
- This statement has three integer literals. The archive limit is 32. Shapes outside that bound were not executed here.
- Password files, `ca.key`, and `server.key` stayed under `/tmp/oo-a317-eef831aa-dba4-4af2-a78b-541bc9e0a317/secrets` until owned cleanup. A fixed-string scan of this evidence tree against each secret file was clean for those files. `san.cnf` overlaps `pg/up.sh` on the four public profile lines `basicConstraints=CA:FALSE`, `subjectAltName=DNS:oo-a317-pg`, `keyUsage=digitalSignature,keyEncipherment`, and `extendedKeyUsage=serverAuth`.
- No root manifest edit, no root lock edit, no `cargo update`, no product crate edit from this line. a313 remains the root manifest and lock writer.
- Semantic publication, public authentication, natural language, harness mount, WAL, and runsc stay missing on the outcome file. M1-I01 and M1-I02 stay `not_accepted`. `ready_for_trusted_query` does not close them. The product-wide full 87 parent goal is unchanged.
