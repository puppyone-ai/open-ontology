# Findings

Review of the frozen a235 scope after the green proof. Reviewer is the same session. No subagent. The shared installer, `policy.rs`, `lib.rs`, manifests, and the lock were compared by hash to archive `a48db85fc13148874349d318affd995f7212188d` and were not edited.

## Result

The owned command, test, and contract match the green run. No remaining defect in that scope.

Attempt 1 Clippy `-D warnings` failed because `LedgerMap.entry_id` and `LedgerMap.party_id` were stored and unread (`linux/proof/attempt-1/clippy.log`, exit 101). The tests had already passed, including the live oracle. The fix asserts those two physical columns, plus `memo`, against `tables[1].physical_columns`. The green Clippy log has no warning.

## What the proof shows

- Stdin `{` exits 4 `scan`. A protocol-only object exits 5 `request`. Role count is unchanged.
- Generation 3 against stored maximum 7 exits 7 `policy stale`. `oo_r_stale_gen` is absent.
- A second table `missing_table` exits 7 `policy mapping`. `oo_r_bad_map` is absent and has 0 column grants.
- Role `clerk` across `party` and `ledger` returns `installed`, role `oo_r_clerk`, grantor `oo_fixture_admin`. Party columns are `party_id`, `display_name`, `note`. `ratio` is absent. Ledger columns are `entry_id`, `party_id`, `memo`.
- The same clerk document returns `replay` and the same `tables` array.
- Role `peek` returns `installed` with 4 column grants. Clerk has 6. Table privileges for both roles are 0.
- Column and table privileges on `_oo.physical_mappings` for `oo_r_clerk`, `oo_r_peek`, and `oo_a235_login` sum to 0.
- `pg_publication` count is 0. Both roles are `NOLOGIN` with superuser, createdb, createrole, inherit, bypassrls, and replication all false.
- The login connection, not the superuser admin connection, runs `SET LOCAL ROLE oo_r_clerk`, reads `party_id` 1, and receives SQLSTATE 42501 for `INSERT` of `_oo_pos` and `UPDATE` of `memo`.
- Offline refusals cover an argument, malformed JSON, a 1048577-byte stdin, the wrong protocol, `audit_file`, a claims row filter, an empty policy list, a missing password file, a password that occurs in the document, a CA PEM containing `PRIVATE KEY`, and a symlink password file.

## Installer boundary

`policy.rs` in the proof tree matches the archive hash. Its grant text is membership `WITH INHERIT FALSE, SET TRUE`, `GRANT USAGE ON SCHEMA "_oo"`, and `GRANT SELECT (columns) ON TABLE`. There is no `GRANT` on `physical_mappings` and no `EXECUTE` in that file. The CLI source contains no `GRANT`. The live count covers table and column privileges on `physical_mappings`. It does not query routine privileges. That query was not added after the green run.

`lib.rs` does not re-export `compile_column_acl_set` or `install_column_acl_set`. The binary calls them through `oo_gateway::policy`.

## Handshake

The schema was frozen while the a234 CLI files were untracked. Those blob ids are in `origin/handshake.txt`. Master later added that command in `bcc86734`. Its stdout policy object uses the same fields, and `explicit_allow` is omitted when absent. This command does not accept protocol `oo.gateway.reviewed-policy-cli/v1` and does not execute that binary. The archive build did not contain `install-column-acl.rs`.

## Open

Authentication is not claimed. The grantor in the proof is superuser `oo_fixture_admin`. The audit WAL is absent. `query-pg` was not invoked. The test does not `ANALYZE`. Row-level security and publication stay open. M1-D14D stays open.
