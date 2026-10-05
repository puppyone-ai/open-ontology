# Findings

Fixed archive `a661dcdf72ff91561ae6b7cb79916e582884d08a`. Evidence session `eef831aa-dba4-4af2-a78b-541bc9e0a211`. These findings are observations from the compiled CLI chain and the public installer APIs the CLIs require. They are not an acceptance result.

## Role pack has no bridge from the review command

`oo-builder review-pg-draft` stdout is protocol `oo.builder.captured-pg-draft-command/v1`. `oo-builder role-pack` requires `oo.builder.role-pack-command/v1` with `effective_oos` and `visibility`.

Probe 1 fed the review stdout bytes unchanged. Exit 1. Stdout length 0. Stderr `oo-builder: role-pack: oo.builder.role-pack-command/v1/field.unknown role-pack command has an unknown field`. Files: `live/steps/role-pack-raw.exit`, `.stdout`, `.stderr`.

Probe 2 sent one object whose `effective_oos` is the review `draft` and whose visibility is only `{"protocol":"oo.builder.role-visibility/v1"}`. Exit 1. Stdout length 0. Stderr `oo-builder: role-pack: oo.builder.role-pack/v1/schema.rejected the effective document does not match the role-pack contract`. Files: `live/steps/role-pack-wrapped.*`.

No command in this chain emits an effective-oos document. The harness did not synthesize a successful pack. a203's hand-built effective envelope is not used here.

## Column ACL installs one table per role

`install_column_acl` is a library function in `crates/oo-gateway/src/policy.rs`. There is no column-ACL CLI (`outcome.json` `column_acl_cli` is `absent`).

`install_in_transaction` creates the `NOLOGIN` role and grants columns only when the role does not already exist. When the role exists, a different physical table returns `PolicyError::GrantMismatch`, display text `existing column grants do not match`, and does not call `grant_usage_and_columns`.

Observed on this database:

- `live/column-acl-clerk-party.txt`: `installed`
- `live/column-acl-clerk-ledger.txt`: `existing column grants do not match`
- `live/column-acl-peek-party.txt`: `installed`
- `live/column-acl-peek-ledger.txt`: `existing column grants do not match`

`outcome.json` records `column_acl_second_table` as that same string and `column_acl_ledger` as `not_granted`. The harness stops if a second install succeeds. It does not hand-GRANT ledger.

`query-pg` clerk success is two `SELECT`s of `trade.party` only. Exit 0. Exact typed rows matched the fixture, including bool and the null note. The separate ledger request exited 6 with stderr `query-pg: guard`, empty stdout, and no audit file (`query_ledger_exit` 6, `query_ledger_audit_present` false). Peek success is `party_id` and `display_name` only. `SELECT note` and clerk `INSERT` each exited 6 with `query-pg: guard` and no audit file.

Direct `SET ROLE oo_r_peek` `SELECT` of the ungranted physical note column returned 42501. Direct `SET ROLE oo_r_clerk` `INSERT` returned 42501. `SELECT` of the granted display name, ordered by the granted party_id physical column, returned `甲公司` and `乙部`.

## Import is awaiting_publish

`import-pg` has no install subcommand. Before `install_full_import`, the command exited 1 with empty stdout and stderr `oo-pipeline: target schema is not ready` (`live/steps/import-pg-not-ready.*`). After the public installer, the command exited 0 with `state` `awaiting_publish`, `source_generation` 11, `target_generation` 1, `table_count` 2 (`live/steps/import-pg.stdout`).

`_oo.physical_mappings.run_generation` for source `a211src` is 11. `register_one` in `crates/oo-pipeline/src/full_import.rs` stores the source generation on the mapping. It is a different number from `target_generation`.

`pg_publication` count is 0. Reader writes in `trade.write_audit` stayed 0. The writer role inserted one source row and that audit count became 1.

`awaiting_publish` is the import fence. It is not semantic publication. The reviewed draft still has `publication=unavailable`, `grants=not_authorized`, `live_catalog_review=pending`, `row_import=not_proven`, and `pg_catalog_integration=not_accepted`. Effective columns keep null `pii` and null `semantic_type`. Nine structural accepts changed `draft_hash` and did not fill those fields.

## query-pg refuses unknown reltuples until ANALYZE

Both imported physical tables had `pg_class.reltuples` `-1` after `import-pg` (`live/reltuples-before.txt`). `load_policy_catalog` in `crates/oo-gateway/src/policy_catalog.rs` treats reltuples `-1` with `unknown_rows=refuse` as `CatalogError::UnknownRows`. `query-pg` maps that to exit 5 and writes no audit file.

The clerk request before ANALYZE exited 5, stdout empty, audit absent (`live/steps/query-clerk-before-analyze.*`). The harness then executed admin `ANALYZE` on the two imported physical tables only. reltuples became 2 and 3 (`live/reltuples-after.txt`). The same clerk request then exited 0. `import-pg` does not analyze. The operator policy used after that point is labeled in `outcome.json`: confirmed and pii none are operator labels, not draft fields.

A draft-shaped policy exited 5 with no audit file before any ACL install (`query_draft_policy_exit` 5).

## Auth, harness, and scope

`outcome.json` records `authentication` `not_claimed`, `harness_mount` `not_performed`, and `semantic_publication` `missing`. Passwords stayed in files under the private tmp directory. Command children were spawned with a cleared environment. Evidence outputs were scanned against the password files (`linux/chain/phases/secret-scan.exit` is 0).

This run does not claim M1-I02 accepted, does not claim the parent goal, and does not release. a208 approval is still pending. a205 REQUEST_CHANGES is retained.
