# Handoff

a403 is waiting on fixed producer commits. This card does not poll. The coordinator's single observation window is separate from this process. When the markers below exist as whole lines in the named files, re-read those files, record the new SHA-256, and only then implement the calls. Do not satisfy a marker by writing YAML, a commit, a gates file, or a pack inside `tools/ontology-lifecycle`.

## a373 — selfbuild-control-api

Add these OpenAPI operations. Input and output are the OC-01 and OC-02 rows in `tools/ontology-lifecycle/REQUIREMENTS-MAP.json`.

- `operationId: exportGeneratedOos`
- `operationId: exportReviewDecisions`
- `operationId: compileEffectiveYaml`

`exportGeneratedOos` writes `ontology/ontology.yaml` (`spec_version: oos/v1`) and `ontology/generated/**/*.yaml` (`schema_version: oos/v1`) from the approved private source. Parsers are `oo_core::oos::parse_ontology_yaml` and `parse_dataset_yaml`. Each file carries sha256, schema hash, evidence hash, and `row_count` 100000 for the 2000/8000/90000 fixture. JSON and Markdown stay compile products.

`exportReviewDecisions` writes `ontology/reviews/decisions.yaml` from real `review.decide` sessions: one approve, one reject, one defer, each bound to `candidate_sha256`, `source_sha256`, the verified subject, and the revision. `parse_decisions_yaml` must accept it. Reject and defer do not enter curated.

`compileEffectiveYaml` merges generated and curated by stable identity (`name`, `id`, `code`, `term`, `metric`, `dataset`, `entity`, or `item`), curated field wins, `status: rejected` drops the effective item, and a changed upstream fact under a curated override becomes a stale review item. `field_candidates_applied` is true and `reseal_count` is at least 1. A later builder run does not change curated hashes.

Current pin: `tools/selfbuild-control-api/openapi/selfbuild-control-api.openapi.yaml` SHA-256 `4a7a155d801e47590f3c60927abf6035da4fc2ca118d318f7f8204330f149cf0`.

## a374 — curated-git-apply

Add `operationId: applyReviewedTree`.

Input: `expected_head`, idempotency key, and blob ids of `ontology.yaml`, `reviews/decisions.yaml`, and the curated YAML from OC-02. The parent is the verified session. `authenticated: false` is refused.

Output: one `update-ref` compare-and-swap commit. Receipt has old commit, new commit, tree, and blob hashes. Restart reads that commit. A mismatched `expected_head` does not move the ref. No push.

Current pin: `tools/curated-git-apply/CONTRACT.md` SHA-256 `4b9f84f013554cee19dcb40209181b0276135a88d098c7261331dc6a7fab740c`.

## a392 — release-control-api

Add `operationId: evaluateGatesFromGit`.

Input: the OC-03 commit, one role, scope `test_scope` or `production`, and the verified session. The caller does not upload pack bytes or `gates.json`.

Output: one result each for G1, G2, G3, G4, G5a, G5b, G6, G7, G8, G9, G10, G11, G12, and G13, plus the role fence, replay, and active CAS. Missing `credential`, `integrity`, `authenticated_owner`, `customer`, or `runsc` keeps production inadmissible. `production` is not rewritten to `test_scope`. A test-scope activation is `certificate_class` `test_scope_not_production`. `compiled_pack` stays `not_published`.

Current pin: `tools/release-control-api/openapi/release-control-api.openapi.yaml` SHA-256 `de281a35aa6f6a75e2a6c4cb8e1a9fb3d2f76dc20799db164c024d1bd4239946`. `decide.rs` SHA-256 `439aa2112925934c7f448f98a220fb889b708cb27fd1c7aa214b21a4d75496f4` returns `production_admissible` false.

## a397 — agent-entry

Add `operationId: bindActiveReleasePack`.

Input: active release operation id, role, pack sha256, source git commit, policy, knowledge, and content hash from `getActiveRolePack`.

Output: `createSession` `pack_sha256` equals that digest. The query audit names that pack. The other role's hidden columns stay absent. Revocation changes the digest and refuses the old pack. A compiled pack that is not the active release is HTTP 409 `pack_unpublished`.

Current pin: `tools/agent-entry/CONTRACT.md` SHA-256 `3634c0a0f580770d5cf24d29d4e4c6d56f3028acfd9ddc0fbcb5554abafd06bf`. That file was not a git commit at this pin.

## Commands already reproducible

```text
ontology-lifecycle inspect --source-root ROOT
ontology-lifecycle run --source-root ROOT --guard EMPTY_DIR
ontology-lifecycle serve --bind 127.0.0.1:PORT --source-root ROOT --once
```

`GET /control/v1/lifecycle/assessment` and `POST /control/v1/lifecycle/runs` with `{}` return 409. A body that claims publication returns 400. Exit 2 means the chain is open. Exit 0 is reserved for `executed` on every gap, which this build does not emit.

## a403 after those commits

Re-pin the delivered files. Call the five operations in order on the same private 100000-row source. Record YAML, decision, git, candidate, active, and agent hashes on each edge. Prove model failure, CAS conflict, restart, stale override, and revoke without forbidden side effects. Keep production unsigned while the external gates are unmet. Do not mark OC-05 from a compiled pack.
