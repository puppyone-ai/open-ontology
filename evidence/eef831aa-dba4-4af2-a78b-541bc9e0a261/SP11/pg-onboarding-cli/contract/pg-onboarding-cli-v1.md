# pg-onboarding CLI v1

Frozen before code. Unit M1-D25 / SP11-W06 child: operator PostgreSQL onboarding only. The parent plan item is the ops console and Compose. This unit does not build that console, Compose, natural language, authentication, or a harness mount.

Immutable product archive: `1cd687d3f106fb9bde8c2a4a0b40cf2dddae9368`. Quantity commit `83b4f94a` is an ancestor. This tool does not edit product crates, the root manifest, or the root lock. a207 remains the only connector manifest and root-lock writer.

Owned paths:

- `tools/pg-onboarding/**` standalone workspace (nearest `[workspace]`, no path dependency on product crates)
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a261/SP11/pg-onboarding-cli/**`

Pins: `serde =1.0.229` with `derive`, `serde_json =1.0.151`. Edition 2024. `rust-version` 1.94.0. No `cargo update`. First lock is `cargo generate-lockfile` inside the post-edit builder. `RUSTUP_TOOLCHAIN=1.94.0` overrides the archive `rust-toolchain.toml` channel 1.98.1.

Caller is a trusted local administrator with an explicit config file. This is not public authentication.

## Phases

Binary `pg-onboarding`. Subcommands are only `inspect` and `apply-reviewed`. Arguments are `--config PATH` and `--run PATH`. Exit 0 success, 1 failed step, 2 usage.

`inspect` stops at a sealed captured draft awaiting review. It does not classify, grant, review, or query.

`apply-reviewed` reads that sealed manifest plus the current operator files and runs the real compilers. It does not retarget the database. Host, port, database, administrator, source cut, target fence, catalog bytes, and captured draft come from the manifest.

## inspect config

Protocol `oo.onboarding.inspect/v1`. `caller` must be `trusted_local_admin`.

Binaries, each `{path, sha256}` of a regular non-symlink file: `init_pg`, `import_pg`, `catalog_pg`, `captured_pg_draft`. `import_pg` and `catalog_pg` may be the same `oo-pipeline` file when both hashes match. Argv is fixed in the tool: `init-pg` has no args; `oo-pipeline` receives `import-pg` or `catalog-pg`; `oo-builder` receives `captured-pg-draft`.

`source`: `source_id`, `host`, `port`, `database`, `username`, `secret_ref`, `password_file`, `ca_file`, `tables[{schema,table}]` in selection order.

`target`: `host`, `port`, `database`, `administrator`, `runtime_principal`, `admin_password_file`, `runtime_password_file`, `ca_file`.

Also `shard`, `owner` (UUID), `source_generation` (the requested source cut), `connect_ms` (1..=120000). Optional `runtime_membership_ready_file`, `membership_wait_ms` (default 60000), `comment_markers` (non-empty strings), `deadlines_ms`.

No budget object. Import and catalog omit budget, so the import CLI defaults apply: connect 30000, statement 60000, lock 30000, max_rows 10000, max_bytes 8388608. The recomputed config identity uses those defaults. Password and CA bytes are not part of the preimage.

## inspect steps

The run directory is created with `create_dir` (not `create_dir_all`). An existing directory is refused. Nothing in the directory is overwritten.

1. Pin-check the four binaries.
2. Read password files (regular, non-symlink, at most 1026 bytes, one trailing LF or CRLF stripped, no NUL). Refuse if those bytes appear in the config, in any child stdin, or in any argv.
3. `import-pg` before init. Non-zero exit and stderr containing `target schema is not ready` is the required not-ready result. Exit 0 stops the phase (`already_installed`). The draft is not deleted. Database state is not rolled back.
4. `init-pg`. Require protocol `oo.pipeline.init-pg-result/v1`, `install=bootstrapped`, `full_import=installed`, `historical_v2=false`.
5. If `runtime_membership_ready_file` is set, wait until that path is a regular non-symlink file, or fail `membership` at the deadline. The tool does not `GRANT`. The proof watcher grants `oo_pipeline_runtime` to the runtime login and then writes the file. Pre-init `CONNECT` is fixture SQL, not this tool.
6. `import-pg`. Require protocol `oo.pipeline.import-pg-result/v1`, `state=awaiting_publish`, `table_count` equal to the request table count and to `tables.len()`.
7. `bind_generation(source_cut, target_fence)` returns `source_cut`. It errors if either value is `< 1` or the two values are equal. `source_cut` must equal the requested `source_generation`. `mapping_run_generation` is `source_cut`. It is never the target fence. Policy and analyze generation use `source_cut`.
8. Recompute `oo-pg-config-identity/v1` from the non-secret import fields (`key=value` lines, then `table\tschema\ttable` lines). Record it as recomputed. This tool does not read `_oo.physical_mappings`. Install and analyze fail closed if the digest or generation does not match the stored mapping row.
9. `catalog-pg`. Pass the exact stdout bytes to `captured-pg-draft`. Optional `comment_markers` must be absent from catalog and captured stdout.
10. Write `catalog.json`, `import-result.json`, `captured-draft.json`, `inspect-manifest.json`, and `inspect-manifest.json.sha256` (SHA-256 of the exact manifest bytes). Stop.

Manifest protocol `oo.onboarding.inspect-manifest/v1`, phase `awaiting_review`. Fields include binary pins, the import result, `source_cut`, `target_fence`, `mapping_run_generation`, recomputed `config_identity_sha256`, `catalog_sha256`, `captured_draft_sha256`, captured `draft_hash`, `schema_hash`, endpoint and password-file paths, and `non_claims`. No password bytes.

Inspect outcome file `outcome.json` status `awaiting_review`. A failed inspect writes `outcome.json` status `failed` with the step and `database_state=unchanged_by_this_tool`, and leaves every file already written.

## apply-reviewed config

Protocol `oo.onboarding.apply-reviewed/v1`. `caller` must be `trusted_local_admin`.

No host, port, or database fields. Those come from the manifest.

Binaries: `review_pg_draft`, `reviewed_effective_oos`, `reviewed_policy`, `install_column_acl`, `provision_mapping_reader`, `analyze_pg`, `role_pack`, `role_pack_bundle`, `query_pg`. Review and role-pack may be the same `oo-builder` file when both hashes match.

Operator files, passed through, not invented here: `reviews_file` (JSON array of structural reviews), `decisions_file` (JSON array), `settings` (object forwarded to `reviewed-effective-oos`), `schema`, `query_login {user, password_file}`, `roles[]`.

Each role: `slug`, `allow[{dataset,column,explicit_allow}]`, `visibility_file`, `queries[]`.

Each query: `name`, `when` (`before_analyze` or `after_bundle`), `expected_exit`, optional `expected_stderr`, `statement`. `statement.source` is `request` (sql in the request) or `stdin` (the operator file is copied into the child stdin). Limits `max_rows`, `max_result_bytes`, `wall_clock_ms` come from the operator. Catalog limits are fixed by this tool: max_tables 4, max_columns 64, max_functions 1, max_operators 64, large_table_row_threshold 1000000, `unknown_rows=refuse`. The audit path is `run/audit/<name>.jsonl`. This tool creates the parent directory only. `query-pg` creates the file. SQL is never an argument.

`analyze`: `statement_timeout_ms`, `lock_timeout_ms`, `connect_ms`.

## apply-reviewed steps

Refuse when the manifest bytes do not match the sidecar, when `outcome.json` is not `awaiting_review`, when `apply-outcome.json` exists, or when any apply step file already exists. A new directory is required. The inspect outcome and the captured draft stay in place.

1. `review-pg-draft` stdin is `{"catalog": <parsed catalog bytes>, "reviews": <operator array>}`. Require a captured `draft_hash` different from the reviewed `draft_hash`, and one `schema_hash` across the captured wrapper, the reviewed draft, and the effective provenance.
2. `reviewed-effective-oos` stdin is the command object: protocol `oo.builder.reviewed-effective-oos-command/v1`, artifact object parsed from the review stdout, operator settings, decisions parsed from the operator file. Provenance `authentication`, `authorization`, `grants`, `publication`, and `semantic_publication` must be `not_claimed`. Record `hidden[]` as the compiler emitted it. This tool does not classify `private_note` or any other column.
3. For each role, `reviewed-policy`. `command` is the exact `reviewed-effective-oos` stdout string. `artifact` is the exact `review-pg-draft` stdout string. `decisions` is the exact decisions-file string. Binding generation is `source_cut`. `config_digest` is the recomputed identity. Login is `query_login.user`. Allow is the operator allow list. Require policy `generation == source_cut`, `generation != target_fence`, and `config_digest` equal to the recomputed identity. Keep the policy array bytes, including `"row_filter":null`, for install.
4. `install-column-acl` pastes those policy bytes. Require `effect=installed`, `role=oo_r_` plus the slug, `grantor` equal to the manifest administrator and not `oo_target_owner`. Physical table and column names must match the import result (`pt_` / `pc_` plus 60 hex digits).
5. `provision-mapping-reader` for the shared login. Record `installed` or `replay`. Any other effect fails the step. This tool does not emit `GRANT`.
6. Operator queries with `when=before_analyze`.
7. `analyze-pg` with `source_generation=source_cut` and the recomputed digest. Require `published=false` and `estimate_is_exact=false`.
8. `role-pack` per role. The command is `{"protocol":"oo.builder.role-pack-command/v1","effective_oos":<object>,"visibility":<operator file>}`. Manifest statuses stay `not_claimed` / `not_performed`.
9. `role-pack-bundle --request <command file> --output run/bundles/<slug>.json`. The output must not already exist. Success writes no stdout and no stderr. Statuses stay `authentication=not_claimed`, `publication=not_claimed`, `mount=not_performed`.
10. Operator queries with `when=after_bundle`.

A query step whose exit equals `expected_exit` succeeds, including a non-zero refusal. Optional `expected_stderr` must be a substring of that child's stderr. Any other exit fails the phase. Query requests omit a null `row_filter` key. Identity `role` is `oo_r_` plus the slug. Identity `policy_id` is the 64-hex config digest.

Apply success writes `apply-outcome.json` status `ready_for_trusted_query`. Failure writes `apply-outcome.json` status `failed` with the step and `database_state=unchanged_by_this_tool`. Existing step files and the draft remain. This tool does not drop roles, drop schemas, or publish.

## Subprocess

Each child gets its own stdin, stdout, and stderr regular files created with `create_new` and mode `0600`. The parent polls `try_wait` every 50 ms. At the deadline it `kill`s, then `wait`s. Capture above 8388608 bytes fails the step after the child exits. Environment is cleared, then only `PATH`, `HOME=/tmp`, and `LANG=C.UTF-8` are set. No shell, no secret in argv or env, no runtime install or download. Absolute pin-checked binaries only. Child stderr is not copied to this tool's stderr. This tool's stderr line is `pg-onboarding: <step> <static detail>`.

Default deadlines: import-not-ready, init, catalog, install, provision, analyze, and query 120000 ms; import 180000 ms; captured, review, effective, policy, role-pack, and bundle 60000 ms.

## Outcome non-claims

Every outcome carries:

- `parent_acceptance` false
- `development_unit_approval` false
- `component_approval` false
- `m1_i01` and `m1_i02` `not_accepted`
- `semantic_publication` `missing`
- `authentication` and `authorization` `not_claimed`
- `wal` `missing`
- `natural_language` `missing`
- `harness_mount` `not_performed`
- `hand_grant_business_columns` false
- `hand_classified_policies` false
- `role_login_binding` `configuration_not_auth`
- `analyze_published` false on success only when analyze ran and reported false; otherwise false
- `database_rollback` false
- `database_release` false
- `mapping_rows_read` false
- `config_identity` `recomputed_from_non_secret_request_fields`
- `public_auth` false

`ready_for_trusted_query` is not semantic publication, authentication, natural language, or a full harness. The CLI does not check fixture gold. The Rust oracle does. Duplicate JSON keys in this tool's own config are an accepted residual: `serde_json` keeps the last key, and the child compilers reject duplicate keys in their documents. Password `String`s are dropped, not zeroized.

## Proof fixture

Fresh private TLS PostgreSQL. Two tables (`shop.customer`, `shop.order_doc`), two roles on one `NOINHERIT` login. `amount` is int8 and stays `none` / `quantity`. The hidden column is unclassified. Unknown `reltuples` must be refused before analyze. Hidden SQL, `INSERT`, and `ORDER BY _oo_pos` are operator queries with the compiler's refusal exits. Autovacuum stays off until `analyze-pg`.

Success of the CLI is `ready_for_trusted_query` plus the structured non-claims. The oracle checks amount cells, the count cell, marker absence, peek suppression, and `classification.absent` for the hidden column. A gold miss fails the proof even if the CLI wrote ready.
