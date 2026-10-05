# Reviewed policy adapter

Contract `oo.gateway.reviewed-policy/v1`. Library `oo_gateway::reviewed_policy`.

`compile_reviewed_policies` reads one bounded `reviewed-effective-oos` command result, the sealed captured-draft artifact that produced it, the decision array that produced it, and one explicit administrative binding. It returns the full `ColumnAclPolicy` set for one role and the `CompiledPolicySet` from the existing `compile_column_acl_set`.

The command result is `oo.builder.reviewed-effective-oos-command/v1` as frozen in `contracts/builder/reviewed-effective-oos-v1.md`. This adapter checks that contract's protocol, compiler, provenance statements, and canonical hashes. It also checks the typed effective OOS with the core dataset and relationship validators. A document that only carries the protocol string is refused.

This adapter does not call `compile_structure_draft`. That recompile stays in `oo-builder`. The caller passes `schema_hash` and `artifact_sha256` recorded from the accepted compiler run. Those two values are the trust root. The adapter recomputes the captured-draft canonical hashes and compares them with the artifact, the command provenance, and that root. A self-consistent document whose artifact hash differs from the recorded root is refused. A draft that was not produced by `compile_structure_draft` is outside this adapter's seal; the recorded artifact hash is what binds the run the caller accepted.

The role, login, config digest, generation, and allowlist are administrative mapping input. They are not an authentication decision and they are not a grant. The adapter does not open a database, does not call `install_column_acl` or `install_column_acl_set`, and does not return a published or installed outcome. `row_filter` on every returned policy is `None`.

Physical types are checked against the sealed catalog embedded in the artifact. A later live reproof remains `load_policy_catalog` in the existing column-ACL installer. The a218 grant proof for `bool`, `int2`, `int4`, `int8`, and `text` is referenced and is not reclosed here.

Classification on each `ColumnSpec` comes from the typed effective column after the decision binds. The allowlist does not carry a semantic type, a PII level, or a classification state. This is the gap left by operator-labeled `query-pg` policies.

## API

```rust
pub struct ReviewedIdentity {
    pub project: String,
    pub ontology_version: String,
    pub catalog_version: String,
    pub policy_id: String,
    pub policy_version: String,
    pub knowledge_id: String,
    pub knowledge_version: String,
}

pub struct ReviewedSourceSeal {
    pub source_id: String,
    pub schema: String,
    pub schema_hash: String,
    pub artifact_sha256: String,
}

pub struct ReviewedAllow {
    pub dataset: String,
    pub column: String,
    pub explicit_allow: bool,
}

pub struct ReviewedBinding {
    pub identity: ReviewedIdentity,
    pub seal: ReviewedSourceSeal,
    pub config_digest: String,
    pub generation: i64,
    pub role: String,
    pub login: String,
    pub allow: Vec<ReviewedAllow>,
}

pub struct ReviewedColumnPolicies {
    pub policies: Vec<ColumnAclPolicy>,
    pub compiled: CompiledPolicySet,
}

pub enum ReviewedPolicyError {
    Budget,
    Scan,
    Protocol,
    Shape,
    Seal,
    Identity,
    Decision { reason: &'static str },
    Semantic,
    PiiAllow,
    Duplicate,
    Forbidden,
    Rls,
    Policy(PolicyError),
}

pub fn compile_reviewed_policies(
    command_output: &str,
    artifact_json: &str,
    decisions_json: &str,
    binding: &ReviewedBinding,
) -> Result<ReviewedColumnPolicies, ReviewedPolicyError>
```

`ReviewedPolicyError` implements `Display` and `Error`. Display text is a fixed sentence plus, for `Decision`, one of the reason words below. It does not echo a document body. A column name appears only after it has passed the core column newtype and only when it is ASCII graphic and at most 128 bytes. `Policy` forwards `PolicyError`'s display.

`code()` returns `oo.gateway.reviewed-policy/v1/` plus the suffix:

| Variant | Suffix |
|---|---|
| `Budget` | `budget` |
| `Scan` | `scan` |
| `Protocol` | `protocol` |
| `Shape` | `shape` |
| `Seal` | `seal` |
| `Identity` | `identity` |
| `Decision` | `decision` |
| `Semantic` | `semantic` |
| `PiiAllow` | `pii_allow` |
| `Duplicate` | `duplicate` |
| `Forbidden` | `forbidden` |
| `Rls` | `rls` |
| `Policy` | `policy` |

Decision reasons are `unknown`, `missing`, `stale_schema`, `stale_fact`, `rejected`, `unbound`, `mismatch`, and `hidden`.

One call stamps one role. Two roles are two calls. Any failure refuses the whole set. The adapter does not drop the failing column and compile the rest.

## Check order

1. Each of `command_output`, `artifact_json`, and `decisions_json` is at most 1048576 bytes. A longer input is `budget` and is not parsed.
2. Each input passes `oo_core::json_scan::parse_json_value` with scanner contract `oo.gateway.reviewed-policy/v1`. Duplicate keys, depth, node count, and trailing data are `scan`. `serde_json::from_str` is not the scanner.
3. `command_output` bytes equal the canonical JSON of the parsed command value. Canonical form sorts object keys, keeps array order, and escapes strings as the reviewed-effective compiler does (`\"`, `\\`, `\b`, `\f`, `\n`, `\r`, `\t`, and `\u00xx` for other controls below U+0020). A trailing newline or pretty-printed equivalent is `protocol`. The canonical writer lives in this module. It does not use `oo-builder` and it does not use `oo_core` value frames. Digests use `oo_core::api::payload_sha256`.
4. Any object in the command output whose key is `samples`, `credentials`, `privileges`, `password`, `secret`, or `row_filter` is `forbidden`. This walk happens before typed field rejection, so those names are not reported as an unknown field. The artifact catalog may contain privilege records. Those records are not copied onto a policy, and this walk does not read the artifact.
5. The command object has exactly `protocol`, `compiler`, `effective_oos`, `hidden`, `omitted_relationships`, and `provenance`, with `deny_unknown_fields`. `protocol` is `oo.builder.reviewed-effective-oos-command/v1`. `compiler` is `oo-builder/reviewed-effective-oos/v1`. A missing field, an unknown field, or a wrong JSON type is `shape`. Any other protocol or compiler is `protocol`.
6. The artifact object has exactly `protocol`, `wrapper`, `evidence_envelope`, and `draft`. `protocol` is `oo.builder.captured-pg-draft-command/v1`. The wrapper protocol is `oo.builder.captured-pg-draft/v1` and the wrapper compiler is `oo-builder/captured-pg-draft/v1`. Wrapper statements match the captured-draft contract: `grants=not_authorized`, `publication=unavailable`, `certification=none`, `llm=not_used`, `source_connection=not_used`, `row_import=not_proven`, `live_catalog_review=pending`, `historical.pg_catalog_integration=not_accepted`, `historical.pg_catalog_contract_commit=b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`. The draft protocol is `oo.builder.structure-draft/v1`. Draft provenance keeps `publication=unavailable`, `pg_catalog_integration=not_accepted`, `certification=none`, `llm=not_used`, and `source_connection=not_used`. The evidence envelope protocol is `oo.builder.catalog-evidence/v1`. A statement or shape miss is `seal`.
7. Recomputed hashes must equal the fields that carry them. `wrapper_hash` is the canonical SHA-256 of the wrapper object with `wrapper_hash` removed. The canonical SHA-256 of `evidence_envelope` equals `wrapper.evidence_envelope_hash`. The SHA-256 of the canonical draft bytes equals `wrapper.draft_hash` and `provenance.draft_hash` on the draft. The manifest preimage is the canonical SHA-256 of the draft after removing `provenance.manifest_hash`, and it equals both `draft.provenance.manifest_hash` and `wrapper.manifest_hash`. `generated_sha256` and `curated_sha256` are the canonical SHA-256 of `draft.generated` and `draft.curated`. `draft.provenance.schema_hash` equals `wrapper.schema_hash`. Every hash is 64 lowercase hex digits. A miss is `seal`.
8. `binding.seal.schema_hash` equals the artifact schema hash. `binding.seal.artifact_sha256` equals the canonical SHA-256 of the whole artifact. Both equal `command.provenance.schema_hash` and `command.provenance.sealed_source_sha256`. Command provenance `authentication`, `authorization`, `grants`, `publication`, and `semantic_publication` are each the string `not_claimed`. A miss is `seal`.
9. A repeated `dataset` and `column` pair in `decisions_json`, or a repeated pair in `binding.allow`, is `duplicate`. This check is before `decisions_sha256`, so an ambiguous list is not compiled. The decisions array has at most 4096 elements; a longer array is `budget`. After the list is unique, the canonical SHA-256 of the parsed decisions value equals `command.provenance.decisions_sha256`. The decisions input itself need not be canonical bytes. A hash miss is `seal`.
10. `effective_oos.project`, `ontology_version`, `catalog_version`, `policy.id`, `policy.version`, `knowledge.id`, and `knowledge.version` equal `binding.identity`. `evidence_envelope.source.source_id` equals `binding.seal.source_id`. A miss is `identity`. Display for that case is `reviewed document identity does not match the administrative binding`.
11. The effective ontology is one `OntologyFile`. `compile.per_role` is true. `compile.sample_values.default` is not `raw`. `include_schemas` is non-empty, unique, and contains `binding.seal.schema`. A miss is `shape`, except a raw sample default, which is `forbidden`.
12. Each effective dataset passes `validate_dataset`. Relationships are parsed as `Relationship` values and checked with `validate_relationships` under `RelationshipsFile { schema_version, relationships }` using the ontology `spec_version`. A validation failure is `shape`. Dataset names are core `QualifiedTable` values and column names are core `TableName` values. The adapter does not read a knowledge graph, glossary, or ontology entity name to invent a logical identifier.
13. On `draft.generated` and `draft.effective`, a column object with non-null `pii`, `semantic_type`, `classification`, or `review` is `seal`. Those layers are the unclassified structure draft. The command's `effective_oos` is where an applied decision is required to appear.
14. For each allow entry, in order, the dataset is `{binding.seal.schema}.{table}` and the column exists on that effective dataset. Absence is `decision` reason `unknown`, unless the artifact review queue has a `structural_column` item for that dataset and column whose `state` is `rejected`, or the generated column `status` is `rejected`. Those two cases are `rejected`. No matching decision is `missing`. The decision's `schema_hash` must equal the trusted schema hash; otherwise `stale_schema`. The decision's `fact_fingerprint` must equal the `review_queue` item whose `item_id` is `column/{dataset}/{column}` and whose `kind` is `structural_column`; a missing item is `unbound`, and a different fingerprint is `stale_fact`. The artifact effective column `status` of `rejected` is `rejected` even when a classification object is present. The effective output column must then carry the decision's `pii`, `semantic_type`, and reviewer `by` and `at`, with `classification.state` `confirmed` and `review` null. Any other classification is `mismatch`. A requested column that also appears in `hidden` after this bind is `seal`. `hidden` does not authorize a column the decision did not bind, and a stale, missing, or rejected decision is `decision` rather than a successful omit.
15. The sealed catalog is `artifact.evidence_envelope.catalog.tables`. The allowlisted table matches `schema_ident` and `name_ident`. The column matches `name_ident`. `typtype` is `b`. `type_name` equals the effective column `data_type` and is one of `bool`, `int2`, `int4`, `int8`, `text`. A miss is `semantic`. `rls_enabled` true, `rls_forced` true, or a non-empty `policies` array on that table is `rls`. `rls_enabled` false with an empty policy array is accepted. Tables that are not allowlisted are not granted and their RLS flags do not by themselves refuse the set.
16. Semantic type and physical type must agree. `bool` accepts only `boolean_flag`. `int2`, `int4`, and `int8` accept `identifier`, `foreign_key`, `code`, and `status_code`. `text` accepts `identifier`, `foreign_key`, `entity_name`, `code`, `status_code`, `category`, `address`, `phone`, `email`, `person_name`, and `free_text`. `money`, `quantity`, `ratio`, `event_time`, `date`, `audit_time`, `geo`, `json`, and `system` are `semantic`. A missing `data_type` is `semantic`.
17. PII uses the decision's level and the allow entry's `explicit_allow`. `none` with `explicit_allow` false becomes `PiiClass::None`. `personal`, `sensitive`, or `restricted` with `explicit_allow` true becomes `PiiClass::Declared { explicit_allow: true }`. `explicit_allow` true on `none`, or false on a declared level, is `pii_allow`. The adapter does not downgrade a declared level to `None`.
18. Allow entries group into one `ColumnAclPolicy` per table. Table order is first appearance. Column order inside a table is allow order. `project`, `config_digest`, `generation`, `role`, and `login` are copied from the binding onto every policy. `status` is `ClassStatus::Confirmed`. `semantic_type` is the effective column's semantic type string. `row_filter` is `None`. An empty allowlist, after the document checks, is `Policy(EmptyPolicySet)`.
19. `compile_column_acl_set` then runs on that vector. Its `PolicyError` is returned as `Policy` and refuses the whole set. Gateway identifier rules, digest shape, generation of at least 1, the 32-table and 512-column bounds, metadata names, and classification admission stay in that function. A core-valid name that fails those rules is `policy`, not a second grammar.

Every returned policy shares the binding's project, config digest, generation, role, and login. `compiled.role()` is `oo_r_` plus the role slug when compile succeeds. `compiled.len()` equals the number of distinct allowlisted tables.

## What stays open

This adapter does not authenticate the caller, the role, or the login. It does not install, grant, or publish. It does not write an audit WAL. It does not prove row-level security on a live server. It does not prove a non-superuser grantor.

M1-D14A, M1-D14B, and SP06-W01 stay open. This contract does not accept a parent plan item and it does not release.

The structure-draft recompile remains the compiler's seal. Live physical-type and grant proof remains the existing installer path cited from a218.
