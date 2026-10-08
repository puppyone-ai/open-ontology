# Handoff

Coordinator and owners. Names of operations are free. A new `operationId` line is not the delivery. The success schema must require the fields below, on a fixed git commit, with no disqualifying const. This crate calls `127.0.0.1` only, after `GET /control/v1/identity/current` returns the same CSRF and `authenticated` true, and only on a path under `/control/`.

## a373 — OC-01 and OC-02

Input for OC-01: verified `__Host-oo-session`, `X-OO-CSRF`, and `Origin`. Server source seal. No caller YAML and no caller pack. Body may name `shop.party`, `shop.account`, and `shop.entry`.

Output: `ontology_yaml_sha256`, `generated_files`, `source_seal_sha256`, `spec_version` `oos/v1`, and `table_row_counts`. The approved private fixture is shop.party 2000, shop.account 8000, shop.entry 90000. The sum 100000 is not a per-file `row_count`.

Input for OC-02: approve, reject, and defer, each bound to the candidate, the source, and the identity revision. Then the effective compile for that build.

Output: `decisions_yaml_sha256`, `curated_overlay_sha256`, `effective_yaml_sha256`, and `reseal_count` at least 1. The body contains approve, reject, and defer. `ledger_authenticated` false is refused. `compiled_pack` const `not_published` is not this edge.

Current pin `4a7a155d801e47590f3c60927abf6035da4fc2ca118d318f7f8204330f149cf0` does not contain that success schema. Status `blocked_contract`.

## a374 — OC-03

The committed CLI contract and the two JSON schemas were read. They are not yet a loopback success response.

Still required: `method` and `path` under `/control/`, and a success body with `authenticated` true, a non-null commit id (`commit` or `new_commit`), a tree id (`tree` or `new_tree`), the previous head (`expected_head` or `old_commit`), `ontology_blob`, `decisions_blob`, and the curated blob ids (`curated_blob` or `blobs` / `new_blob`). `new_commit` null is outcome unchanged and is refused. The E57 receipt with `authenticated` false is a different operation.

This crate will not spawn `curated-git-apply` or git. Dirty `src/git.rs`, `src/lib.rs`, and untracked `src/reviewed.rs` are not a fixed archive. Status after the contract pin move is `blocked_contract` / `schema_missing`.

## a392 — OC-04

Input: the OC-03 commit id, one role, scope `test_scope` or `production`, and the verified session. The caller uploads neither gates nor pack bytes.

Output: `source_commit`, `gate_results` covering G1 through G13 including G5a and G5b, `role_fence`, `replay`, and `active_cas`. A test-scope activation is `certificate_class` `test_scope_not_production` and `scope` `test_scope`. `production_ready` true and `certificate_class` `production` are refused. `production_admissible` stays false until the external gates are real.

Current OpenAPI pin `de281a35aa6f6a75e2a6c4cb8e1a9fb3d2f76dc20799db164c024d1bd4239946` does not contain that success schema. Status `blocked_contract`.

## a397 — OC-05

Put the contract in a git commit. The success body needs `active_release_id`, `pack_sha256`, `source_commit`, `policy_sha256`, `knowledge_sha256`, and `content_sha256`, with `active` true. `compiled_pack` `published`, `compiled_pack` `not_published`, and `private_fixed` are refused. A 409 `pack_unpublished` is not the active pack.

The worktree file is untracked. Observed SHA-256 at the pre-commit re-read is `51876a007ec288e0e8c094445e895e38bc9663e0eeaf3840e6acbf842a10063e`. The pin was not moved. Status `unpinned`. It is not called.

## a403 — after those four commits

Re-read each file, record the new pin, then run the five operations on one private 100k source and record the hashes. Prove model failure, CAS conflict, restart, stale override, and revoke with no review, Git, or publish side effects. Keep the full release unsigned while external production gates are unmet. That journey is OC-06. It is not this commit.
