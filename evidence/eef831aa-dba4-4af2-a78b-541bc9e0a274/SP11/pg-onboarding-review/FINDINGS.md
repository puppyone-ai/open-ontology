# Findings

## Proven on the fresh cluster

- Inspect stops at `awaiting_review` with a sealed manifest. `source_cut` is 48 and `target_fence` is 1. `mapping_run_generation` equals the source cut and is not the fence.
- Config identity matches `a4a889fba1436935fb1fe1af9c30a8a79109794d94600fa4ce9d2e1b21f7b618`. Mapping rows were not read.
- Apply uses the sealed manifest plus the operator review, decision, visibility, and allow files produced by the frozen oracle from the captured draft. The CLI does not invent those classifications.
- `install-column-acl` effect is `installed` for `oo_r_clerk` and `oo_r_peek`. Grantor is `oo_fixture_admin`.
- Provision of the shared login is `installed` for clerk and `replay` for peek.
- `analyze-pg` has `published` false and `estimate_is_exact` false.
- Before analyze, the clerk read exits 5 with `query-pg: policy`. After the bundles, hidden, insert, and `_oo_pos` ordering exit 6 with `query-pg: guard`. Positive reads exit 0. Amount cells are `-9223372036854775808`, `9223372036854775807`, and `0`.
- Hidden column `shop.customer.private_note` has reason `classification.absent`.
- Comment marker `甲注评` is absent from the catalog and the captured draft.
- Success outcomes keep `database_rollback` false and `database_release` false. `database_state` is `child_compilers_applied_no_release`.
- The captured draft file remains after the successful apply.

## Repair behavior, no second database

The ignored live test was not part of these runs. The frozen suite, executed by the actual `pg-onboarding` binary and its library, passed 24 tests.

- `json_doc::tests::duplicate_root_nested_and_escaped_keys_refuse` refuses a repeated root key, a nested `source_cut`, a nested role slug, an escaped duplicate, and a duplicate inside an array object.
- `duplicate_root_key_refuses`, `duplicate_nested_key_refuses`, `duplicate_generation_key_refuses`, `duplicate_nested_source_cut_refuses`, and `duplicate_role_slug_refuses` refuse before a run directory or apply step file exists. Detail is `shape`.
- `cli_duplicate_config_stderr_is_static` is the CLI. Stderr is `pg-onboarding: config shape` and does not echo the duplicated password text.
- `spawn.rs` `parse_json_slice` calls `json_doc::parse`. Child JSON and operator JSON use that visitor. `serde_json` is not the duplicate-key authority.
- `cli_failed_child_keeps_marker_and_static_stderr` drives the CLI. A child writes `side-effect-marker-ok`, a later child exits 1, and the outcome is `changes_may_have_been_applied` with `database_rollback` false and `database_release` false. Stderr is `pg-onboarding: provision-mapping-reader-clerk exit`.
- `init_deadline_failure_does_not_claim_unchanged` and `catalog_failure_after_import_does_not_claim_unchanged` use the same conservative state after a child has been spawned.
- `duplicate_decisions_refuse_before_review` writes `unchanged_by_this_tool` because no child has been spawned. `duplicate_manifest_refuses_before_spawn` writes no apply outcome.
- `preflight_deadline_refuses_before_run_dir` rejects deadline 0 before the run directory exists.

## Preserved harness failures

- `linux/manifest` prints `manifest=ok` and `manifest.exit` is 0, but the SHA-256 walker did not run. `/tmp/sha256_manifest` returned `Permission denied`. Diagnosis: `linux/manifest/DIAGNOSIS.txt`. No `SHA256SUMS` was produced by that attempt.

- `linux/preoverlay` exit 1. The tests had already passed. The checker omitted the `json_doc::` module path. Container `8b30606ed50c210721843f4bbea7075911c2c4a2478a14d1af6d58971271f57a`.
- `linux/preoverlay-attempt-2` exit 125. Docker reported the re-extracted archive path missing. Diagnosis: `linux/preoverlay-attempt-2/DIAGNOSIS.txt`.

`linux/preoverlay-attempt-3/inside.sh` prints `oo_task=sp11-pg-onboarding-review-preoverlay-2`, and the container label `oo.task` has that same string. The directory and container name are attempt 3. The bytes under test are the archive tool.

## Limits

- The frozen `live_chain` oracle hardcodes source id `a261src`, host `oo-a261-pg`, and the `oo_a261_*` role names. This review did not edit that file. The database cluster, network, volume, CA, and container name are a274. The hostname is a network alias on the a274 network.
- The runtime membership grant is fixture SQL in `fixture/grant-runtime.sh`, performed by the host watcher. It is not inside the CLI.
- Password bytes are not zeroized by this review. The tool drops the `String` when it drops.
- This evidence does not publish semantics, authenticate a caller, answer natural language, or mount the harness.
- The ops console and Compose parent described for SP11-W06 are outside this CLI.
- No outcome in this review drops a role, drops a schema, rolls a database back, or publishes a release.

## Non-claims on the successful apply outcome

`parent_acceptance` false. `development_unit_approval` false. `component_approval` false. `m1_i01` and `m1_i02` are `not_accepted`. `semantic_publication` is `missing`. `authentication` and `authorization` are `not_claimed`. `wal` is `missing`. `natural_language` is `missing`. `harness_mount` is `not_performed`. `hand_grant_business_columns` false. `hand_classified_policies` false. `role_login_binding` is `configuration_not_auth`. `public_auth` false. `mapping_rows_read` false. `config_identity` is `recomputed_from_non_secret_request_fields`. `analyze_published` false.
