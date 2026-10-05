# PG CLI chain evidence

Session `eef831aa-dba4-4af2-a78b-541bc9e0a211`. Fixed product archive `a661dcdf72ff91561ae6b7cb79916e582884d08a`. Owner path `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a211/SP11/pg-cli-chain/`. This directory is M1-I02 evidence. It does not accept M1-I02, does not close a parent checkbox, and does not release.

Approval remains pending a208. The known executor review a205 REQUEST_CHANGES is retained. a203 is the API integration on base `0ee7cf86`. This run does not repeat that harness claim. a207 remains the only writer of the connector manifest and the root lock. No product crate, Cargo.toml, Cargo.lock, or ledger file was edited.

The final builder exited 0. `linux/chain/chain.exit` is 0. `linux/chain/docker-run.exit` is 0. `live/outcome.txt` is `pg_cli_chain_ok`. `OOMKilled=false`. Memory 6442450944. NanoCpus 1000000000. Container `167ec9fb393ce994e26bf81793dd8aba5c0ec2124470e84b684914eed55f9892`, removed after the run.

## What ran

One private TLS PostgreSQL 17.10, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, container `oo-a211-pg`, database source `oo_a211_src` and target `oo_a211_dst`. CA SAN is DNS `oo-a211-pg`. No host port. The builder image is `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, Rust 1.94.0, linux/arm64, `--pull never`. The chain builder is 6 GiB and 1 CPU. Capabilities dropped. No privileged mode, host network, docker socket, host home, or shared cargo cache.

Smoke ran before the overlay. `linux/smoke/smoke.exit` is 0. The chain then rebuilt from the immutable archive. Focused tests on the archive passed: `oo-builder` captured_pg_cli, review_pg_cli, role_pack_cli; `oo-pipeline` catalog_pg_cli_live, import_pg_cli_live; `oo-gateway` query_pg_cli. Live env vars for those product tests were unset. Clippy `-D warnings` passed for the harness and for the three product bins. Phase exits are under `linux/chain/phases/`.

The harness is Safe Rust (`src/main.rs`). Fixture SQL, row comparison, and the SHA-256 inventory tool are Rust. The product bins are the archive builds:

- `oo-pipeline` `6b13eeef7c0f6fe68dd5c04cd3c2f8688fd2b7d1f98e5447f03e4c47bb2316ed`
- `oo-builder` `a7bc5a7f9836e93d67cd8274305d1690ea1b0c19ec6bd1f2b4fbce2d414271e1`
- `query-pg` `2b44ec467671833569648e8bafe923118f3f3688e825b532b8b5a063dc2c0f32`

Standalone `cargo build --offline` rewrote its private lock and dropped registry packages. That lock was rejected. The overlay member `a211-harness` was built inside a private copy of the archive workspace. `linux/chain/resolve-mode.txt` is `overlay`. `lock-registry-added.txt` and `lock-registry-removed.txt` are empty. `cargo update` was not run (`cargo-update-invoked.txt` is `no`).

Recipe origins are in `origin/recipes.txt`. a200 blobs supply `pg/up.sh` `1784718c92323df0f216e9f5bd8fee182f4ff24d`, `pg/pg-wrap.sh` `e12a368394c724f8f9d3436ad5f4873e139baa66`, smoke launch `20b391304358e474c189203eeb525b46fc2ea194`, smoke inside `d3613027a864b2a40d24642b861374027227f41f`, extract-libclang `1c23dbd6ab58cb0f42d218075fd55c0a8c4fe5ea`, `tools/sha256.rs` `b69a2af13099ce4baf2d04e79c5692d9988ab67d`, `tools/select-debs.rs` `565787c25c25bf451343cf4bc0431619c0611899`. a204 preoverlay launch blob `3737c1a8321814ff3b1070ddf8b89d6864c63f6f`. a199 preoverlay launch blob `7a706f544e6d44dfd46f0c7a1095f72ba582afdf`. The overlay fallthrough follows a203 `linux/harness-inside.sh` blob `7b67c4fca15c19ae14ef0afb7804c1ad686fc038`. The a203 effective-oos synthesis is not in this harness.

## Command chain

Same fixture and the same catalog bytes:

1. `oo-pipeline catalog-pg` exit 0. Stdout protocol `oo.source.pgsql/v1`. Comment marker `甲注评` is absent from that stdout.
2. Those stdout bytes are the stdin of `oo-builder captured-pg-draft`. Exit 0. Protocol `oo.builder.captured-pg-draft-command/v1`.
3. The same catalog object plus nine structural review items is the stdin of `oo-builder review-pg-draft`. Exit 0. `draft_hash` changed from `7279ee3e02d4a06fb580046bc9d07766624a9f39f6ea64fd423368c1376f0adb` to `a18aa2e170156e1a7f6343062f21e2a0ff868a521555813e1070089a4bc03bd3`. Schema hash `ecd342e7e76f36ca464e17f745078128b430bd880472daa2129efb0dfb9c3b7e`. Nine queue items are accepted. Wrapper fields stay `publication=unavailable`, `grants=not_authorized`, `live_catalog_review=pending`, `row_import=not_proven`. `provenance.pg_catalog_integration` stays `not_accepted`. Effective column `pii` and `semantic_type` stay null.
4. `oo-builder role-pack` was given the review stdout unchanged. Exit 1. Stdout empty. Stderr: `oo.builder.role-pack-command/v1/field.unknown role-pack command has an unknown field`.
5. A second probe wrapped `draft` as `effective_oos` with visibility `{"protocol":"oo.builder.role-visibility/v1"}`. Exit 1. Stdout empty. Stderr: `oo.builder.role-pack/v1/schema.rejected the effective document does not match the role-pack contract`. No role pack was written.

Import, on the same source fixture:

1. Public installer `install` returned `bootstrapped` (`live/install-outcome.txt`).
2. `oo-pipeline import-pg` before `install_full_import` exited 1, stdout empty, stderr `oo-pipeline: target schema is not ready`.
3. `install_full_import` returned `installed`.
4. `oo-pipeline import-pg` exited 0. Protocol `oo.pipeline.import-pg-result/v1`. `run_id` `a89279dc-c8fc-4658-807c-63050629d414`. `state` `awaiting_publish`. `source_generation` 11. `target_generation` 1. `table_count` 2. Party digest `c7471857c23d13a7ca7839777f4db49d6f391973d27708ca13f8decfa9705224` (2 rows). Ledger digest `f0e19cd760f0eb7f6422b28d777dcbeb38381a572228530fcfca9a2a1af8ad6f` (3 rows). Physical names match `pt_` and `pc_` plus 60 lowercase hex digits. Mapping `run_generation` is 11, the source generation. Config digest `92c2601544129601251968db9bc4e75575bf5152912c10da2f5209d63eca933d`. `pg_publication` count is 0. Reader `trade.write_audit` count stayed 0 after the writer inserted one source row.

Column ACL has no CLI. `install_column_acl` granted the first table of each role and refused the second:

- `oo_r_clerk` party: `installed`. Ledger: `existing column grants do not match`.
- `oo_r_peek` party (`party_id`, `display_name`): `installed`. Ledger: `existing column grants do not match`.

No hand GRANT was added for ledger. `query-pg` success is the granted party table only.

`query-pg` with a draft-shaped policy (`unclassified`, `pii.class=unknown`) exited 5. Stdout empty. No audit file.

With the operator policy (`confirmed`, `pii.class=none`, semantic type `operator_label`), before ANALYZE, clerk exited 5. Stdout empty. No audit file. Both physical tables had `reltuples` `-1` (`live/reltuples-before.txt`). The harness then ran admin `ANALYZE` on those two tables only. Afterward reltuples were 2 and 3 (`live/reltuples-after.txt`). `import-pg` does not analyze.

After that ANALYZE:

- Clerk multi-turn, party only, exit 0. Turn 0 returned the two party rows (`int`, `text`, `bool`, `null` then `int`, `text`, `bool`, `text`). Turn 1 returned `party_id` and `display_name` for the same two rows. Both summaries are `succeeded`, `returned_rows` 2, `end_audit_ok` true, `truncated` false. Audit `oo.gateway.query-pg-audit/v1` has admit then complete for `oo_r_clerk`. The audit file has no `SELECT` text and no password.
- Ledger request, policy naming ledger, exit 6, stderr `query-pg: guard`, stdout empty, `query_ledger_audit_present` false.
- Peek `SELECT party_id, display_name` exit 0, two rows, audit for `oo_r_peek`.
- Peek `SELECT note` exit 6, stderr `query-pg: guard`, no audit file.
- Clerk `INSERT` exit 6, stderr `query-pg: guard`, no audit file.

The direct login session, after `SET ROLE`, required SQLSTATE 42501 for peek `SELECT` of the ungranted physical `note` column and for clerk `INSERT`. Peek `SELECT` of `display_name`, ordered by the granted `party_id` column, returned `甲公司` and `乙部`. Ordering by `_oo_pos` is denied because that column is outside the column grant (attempt 8).

## Seams recorded in this run

- No CLI emits `effective_oos`. `role-pack` rejects both the review command stdout and the structure draft wrapped as `effective_oos`.
- `install_column_acl` grants column `SELECT` for one table per role. The second table returns `existing column grants do not match` and does not add grants. Ledger stays ungranted. `query-pg` therefore has no successful ledger rows.
- `query-pg` policies are an operator document. They are not a role-pack artifact. `confirmed` and `pii none` are operator labels. The draft still has null `pii` and `semantic_type`.
- Import state `awaiting_publish` is the fence written by `import-pg`. `pg_publication` count stayed 0. Semantic publication is missing. Authentication is not claimed. No harness mount was performed.
- `import-pg` has no install subcommand. The not-ready probe exits 1 before `install_full_import`.
- Column ACL has no CLI.
- `import-pg` leaves `pg_class.reltuples` at `-1`. `query-pg` with `unknown_rows=refuse` exits 5 until an admin `ANALYZE`. This harness records that exit, then analyzes only the two imported physical tables.
- Mapping `run_generation` is source generation 11. `target_generation` is 1.

## Limits

Whole M1 is not done. M1-I01 stays a separate API path. This evidence does not add an accepted development unit. Failed attempts 1 through 9 are preserved under `linux/chain/attempt-*` with `why.txt`. Raw command bytes are under `live/steps/`. The numeric summary is `live/outcome.json`.
