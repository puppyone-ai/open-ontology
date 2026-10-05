# Findings

Author check of commit `baf70c5c7b6d01183a178f53c77f65da4a7bb497` against `contracts/gateway/reviewed-policy-v1.md` blob `23e58a66d0c4111a1253c4723998f026c24c2667` and `contracts/builder/reviewed-effective-oos-v1.md`. This is the same session that wrote the code. It is not an independent review. M1-D14B stays unaccepted. No ledger line was edited.

## Hand copy

a211 left the operator to copy PII and semantic type onto a column policy. `compile_reviewed_policies` reads the canonical `reviewed-effective-oos` command, the sealed captured-draft artifact, the decision array, and one administrative binding. `ColumnSpec.status`, `semantic_type`, and `pii` come from the typed effective column after the decision binds. The allowlist carries `dataset`, `column`, and `explicit_allow` only. The adapter calls the existing `compile_column_acl_set`. It does not call `compile_structure_draft` and it does not depend on `oo-builder`.

Role, login, config digest, generation, and the allowlist are mapping input. They are not an authentication decision and they are not a grant. The adapter does not open a database, does not call `install_column_acl` or `install_column_acl_set`, and does not return a published outcome. `row_filter` on every returned policy is `None`. The test `adapter_source_does_not_open_a_database` scans `reviewed_policy.rs` for `tokio_postgres`, `install_column`, and `Client`.

## Seals

The command must be canonical JSON with no trailing newline. Scanner contract is `oo.gateway.reviewed-policy/v1`. Forbidden keys `samples`, `credentials`, `privileges`, `password`, `secret`, and `row_filter` are walked on the command output before typed rejection. The artifact catalog may contain privilege records. That walk does not read the artifact.

Recomputed hashes cover `wrapper_hash` (wrapper with `wrapper_hash` removed), the evidence envelope, the canonical draft, the manifest preimage (draft with `provenance.manifest_hash` removed), `generated`, and `curated`. They must match the wrapper and the command provenance. `binding.seal.schema_hash` and `binding.seal.artifact_sha256` are the trust root and must equal the artifact schema hash, the canonical artifact digest, and `command.provenance.schema_hash` / `sealed_source_sha256`. Non-claims `authentication`, `authorization`, `grants`, `publication`, and `semantic_publication` are `not_claimed`. A self-consistent document whose artifact hash differs from the recorded root is `seal`. `seal_tamper_refuses` passed on the real compiler output.

Gateway contract step 7 names `provenance.draft_hash` on the draft. The a217 command puts `draft_hash` on the command provenance, next to `wrapper.draft_hash`. A real structure-draft provenance object carries `manifest_hash` and `schema_hash` and does not carry `draft_hash`. `verify_seals` compares the recomputed draft digest with `wrapper.draft_hash` and `command.provenance.draft_hash`, and it compares `manifest_hash` and `schema_hash` on `draft.provenance`. The green tests consumed that compiler. Requiring `draft.provenance.draft_hash` would seal-fail every artifact that compiler emits. The contract file was not rewritten.

Duplicate dataset and column pairs in the decisions or the allowlist return `duplicate` before `decisions_sha256`. The decisions bytes need not be canonical. After the list is unique and at most 4096 long, the canonical digest must equal `command.provenance.decisions_sha256`.

## Decisions

Absence of a requested column is `unknown`, unless the review queue item `column/{dataset}/{column}` of kind `structural_column` has state `rejected`, or the generated column status is `rejected`. Those are `rejected`. A missing decision is `missing` even when the compiler hid the column. Schema hash is checked before the fact fingerprint. A missing queue item is `unbound`. A different fingerprint is `stale_fact`. An artifact effective column whose status is `rejected` is `rejected` after the fingerprint matches and before the output comparison. The effective column must then carry the decision PII, semantic type, and reviewer `by` and `at`, with `classification.state` `confirmed` and `review` null. Any other classification is `mismatch`.

A requested column that is also in `hidden` after that bind is `seal`. The contract lists `hidden` as a permitted decision reason word. This adapter does not return `Decision { reason: "hidden" }`. Step 14 of the same contract assigns that case to `seal`. A stale, missing, or rejected decision stays `decision` when the compiler also hid the column. Columns outside the allowlist may be hidden without refusing the set.

## Types and PII

Physical type comes from `artifact.evidence_envelope.catalog.tables`. `typtype` must be `b`. `type_name` must equal the effective `data_type` and be `bool`, `int2`, `int4`, `int8`, or `text`. A miss is `semantic`, including a missing `data_type` and `float4`. RLS (`rls_enabled`, `rls_forced`, or a non-empty `policies` array) is checked after that type match. Tables that are not allowlisted do not refuse the set. Live physical-type and grant reproof stays `load_policy_catalog` on the existing installer. The a218 proof was not reclosed.

`bool` accepts only `boolean_flag`. `int2`, `int4`, and `int8` accept `identifier`, `foreign_key`, `code`, and `status_code`. `text` accepts `identifier`, `foreign_key`, `entity_name`, `code`, `status_code`, `category`, `address`, `phone`, `email`, `person_name`, and `free_text`. `money`, `quantity`, `ratio`, `event_time`, `date`, `audit_time`, `geo`, `json`, and `system` are `semantic`.

`none` with `explicit_allow` false becomes `PiiClass::None`. `personal`, `sensitive`, or `restricted` with `explicit_allow` true becomes `PiiClass::Declared { explicit_allow: true }`. The other two combinations are `pii_allow`. The level is the decision's level after it has matched the typed column. The adapter does not downgrade a declared level.

## Identity and compile

Project, ontology version, catalog version, policy id and version, and knowledge id and version must match the effective document. `source_id` must match the evidence envelope. The identity display is `reviewed document identity does not match the administrative binding`. One call stamps one role. Table order is first appearance. Column order is allow order. An empty allowlist, after the document checks, is `Policy(EmptyPolicySet)`. Gateway identifier, digest, generation, metadata-name, and bound failures from `compile_column_acl_set` stay `Policy`. Core `QualifiedTable` and `TableName` parse the dataset and column. The adapter does not read a knowledge graph.

## Bounds

`lib.rs` versus the fixed archive is `pub mod reviewed_policy` and the `pub use`. Blob `b18192528299c4744170bb12d1b0c89088b9cb66`. Crate docs were not rewritten. `policy.rs`, manifests, and `Cargo.lock` were not edited. a207 remains the only connector-manifest and root-lock writer. No dependency was added.

M1-D14A, M1-D14B, and SP06-W01 stay open. This adapter does not authenticate, install, grant, publish, or write an audit WAL. It does not prove live row security or a non-superuser grantor.
