# Findings

## Proven on the fresh cluster

- Inspect stops at `awaiting_review` with a sealed manifest, catalog, import result, and captured draft.
- `bind_generation` keeps `source_cut` and refuses a fence equal to that cut. The live fence was 1 and the cut was 48.
- Config identity matches the frozen preimage over non-secret fields and the import CLI default budget. Mapping rows were not read.
- Apply uses the sealed manifest plus the operator review, decision, visibility, and allow files. The CLI does not invent those classifications.
- `install-column-acl` effect is `installed` for both roles. Grantor is `oo_fixture_admin`.
- Provision of the shared login is `installed` for clerk and `replay` for peek.
- `analyze-pg` receipt has `published` false and `estimate_is_exact` false.
- Before analyze, the clerk read exits 5 with `query-pg: policy`. After the bundles, hidden, insert, and `_oo_pos` ordering exit 6 with `query-pg: guard`. Positive reads exit 0. Audits for the successful reads have admit and complete lines and do not store the SQL text.
- A second apply in the same run directory fails and the stderr contains `terminal`. The captured draft file remains.
- Comment marker `甲注评` is absent from the catalog and captured draft outputs.
- Subprocess stdin, stdout, and stderr are regular files created with `create_new`. The CLI does not run a shell.

## Preserved failures

- `linux/smoke` exit 101. `reviewed_policy_cli` panicked because `oo-builder`, `reviewed-effective-oos`, and `reviewed-policy` were not built and `OO_A234_OO_BUILDER`, `OO_A234_REVIEWED_EFFECTIVE`, and `OO_A234_REVIEWED_POLICY` were unset. Diagnosis: `linux/smoke/DIAGNOSIS.txt`.
- `linux/postedit` exit 101. `check_install` compared a JSON `Value` with `&Value`.
- `linux/postedit-attempt-2` exit 101. Clippy `collapsible_if` in three sites and `needless_range_loop` in SHA-256 word loading. The tests had already passed.
- `linux/postedit-attempt-3` exit 101. Clippy `useless_format` on two constant policy fragments in `tests/orchestration.rs`.
- `linux/chain` exit 101. `analyze-pg` rejected `statement_timeout_ms` 60000. The compiler maximum is 30000 (`MAX_TIMEOUT_MS` in `crates/oo-pipeline/src/target_statistics.rs`). Stderr was `analyze-pg: bound`, exit 3. Diagnosis: `linux/chain/DIAGNOSIS.txt`.

## Residuals

- An apply failure outcome still uses `database_state` `unchanged_by_this_tool` after child compilers have run. `database_rollback` and `database_release` are false, and the step files keep the child outputs. The wording is the frozen failure string. It does not undo database changes.
- `serde_json` 1.0.151 keeps the last duplicate key. Child compilers reject duplicate keys in their own documents.
- Password bytes are dropped when the `String` drops. That is not a memory wipe.
- The runtime membership grant is fixture configuration in `fixture/grant-runtime.sh`, performed by the host watcher. It is not a business-column grant and it is not inside the CLI.
- This evidence does not publish semantics, authenticate a caller, answer natural language, or mount the harness.
- The ops console and Compose parent described for SP11-W06 are outside this CLI.
- a254 quantity evidence and its review are not accepted by this session.

## Non-claims on the successful apply outcome

`parent_acceptance` false. `development_unit_approval` false. `component_approval` false. `m1_i01` and `m1_i02` are `not_accepted`. `semantic_publication` is `missing`. `authentication` and `authorization` are `not_claimed`. `wal` is `missing`. `natural_language` is `missing`. `harness_mount` is `not_performed`. `hand_grant_business_columns` false. `hand_classified_policies` false. `role_login_binding` is `configuration_not_auth`. `public_auth` false. `mapping_rows_read` false. `config_identity` is `recomputed_from_non_secret_request_fields`. `analyze_published` false.
