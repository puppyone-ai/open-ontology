# Reviewed effective OOS

Contract `oo.builder.reviewed-effective-oos/v1`. Compiler id `oo-builder/reviewed-effective-oos/v1`.

`compile_reviewed_effective` reads one sealed `oo.builder.captured-pg-draft-command/v1` artifact, one list of explicit classification decisions, and one explicit ontology settings object. It returns one `oo.builder.effective-oos/v1` document that `compile_role_pack` already accepts. This compiler does not open a connection, call a model, grant, authenticate, or publish. M1-D18 publication stays open.

The structure-draft compiler leaves column `pii`, `semantic_type`, and `classification` unset. A curated value for those fields is echoed and is not a decision. This compiler does not copy them and does not read `ontology.classification.auto_accept`. A column becomes classified only when a decision below binds it.

## Command

`reviewed-effective-oos` reads one JSON document from stdin and writes one command result to stdout. No arguments. An argument exits 2 and prints usage. The process does not connect, grant, or publish.

Stdin is read up to 1048577 bytes. A longer stream is not read further. The core scanner then rejects a document above 1048576 bytes, a duplicate key, a depth or node overrun, and trailing data. The scanner contract is `oo.builder.reviewed-effective-oos-command/v1`. Scanner failures keep the scanner code. The message is a fixed sentence and does not echo the body.

The document is one object. `serde` `deny_unknown_fields` rejects any other top-level field.

| Field | Shape |
|---|---|
| `protocol` | the string `oo.builder.reviewed-effective-oos-command/v1` |
| `artifact` | one object, the sealed `oo.builder.captured-pg-draft-command/v1` result |
| `settings` | one object, the explicit version settings below |
| `decisions` | one array of classification decisions. Empty is valid |

A protocol other than the command protocol is `protocol.mismatch`. A null artifact, settings object, or decisions array is `schema.invalid`.

## Seal

The artifact is accepted only when it is the captured-draft command result and it recompiles.

The artifact object has exactly `protocol`, `wrapper`, `evidence_envelope`, and `draft`. `protocol` is `oo.builder.captured-pg-draft-command/v1`. The wrapper protocol is `oo.builder.captured-pg-draft/v1` and the wrapper compiler is `oo-builder/captured-pg-draft/v1`. These wrapper statements stay as the captured-draft contract wrote them:

| Field | Value |
|---|---|
| `grants` | `not_authorized` |
| `publication` | `unavailable` |
| `certification` | `none` |
| `llm` | `not_used` |
| `source_connection` | `not_used` |
| `row_import` | `not_proven` |
| `live_catalog_review` | `pending` |
| `historical.pg_catalog_integration` | `not_accepted` |
| `historical.pg_catalog_contract_commit` | `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` |

`wrapper_hash` is the canonical SHA-256 of the wrapper with that field removed. The evidence envelope is canonicalized and its SHA-256 must equal `wrapper.evidence_envelope_hash`. `compile_structure_draft` is then run on that envelope. The draft bytes must equal the canonical embedded draft, and their SHA-256 must equal `wrapper.draft_hash`. `provenance.manifest_hash` must equal the structure-draft preimage and `wrapper.manifest_hash`. The draft's own provenance keeps `publication=unavailable`, `pg_catalog_integration=not_accepted`, `certification=none`, `llm=not_used`, and `source_connection=not_used`.

A mismatch is `seal.rejected` at `/artifact`. The compiler does not repair a hash.

`generated` and `effective` are taken from that recomputed draft. A non-null `pii`, `semantic_type`, `classification`, or `review` on a generated or effective column is `source.classified`. The compiler does not trust a class that the structure draft is not allowed to emit. Curated may still echo those fields. They stay in the curated hash and are not copied.

## Settings

`settings` is caller-supplied. It is not read from the draft and it is not defaulted from a schema name.

| Field | Rule |
|---|---|
| `project` | `^[a-z][a-z0-9_-]{0,62}$` |
| `ontology_version`, `catalog_version` | `^[A-Za-z0-9._+-]{1,64}$` |
| `policy.id` | the project slug, or exactly 64 lowercase hex digits `[0-9a-f]{64}` |
| `policy.version`, `knowledge.version` | `^[A-Za-z0-9._+-]{1,64}$` |
| `knowledge.id` | the project slug |
| `ontology` | one existing `OntologyFile` (`oos/v1`) |

`policy.id` matches when it is the project slug, or when its length is 64 and every byte is an ASCII digit or `a` through `f`. The compiler stores those bytes unchanged. Uppercase hex, a digest length other than 64, a non-hex byte, `/`, a space, a newline, or a control character is `settings.invalid` at `/settings/policy/id`. The static message is `settings are outside the explicit version binding`.

`knowledge.id` uses only the project slug. A hyphen stays legal. The compiler copies `know-a317` unchanged.

`ontology-dialogue` uses this same knowledge slug for `binding.knowledge_id` (`tools/ontology-dialogue/CONTRACT.md`). `know-a317` and `know_a317` are legal here and in that config. Dialogue does not rewrite the id. `binding.policy_id` must be the 64-lowercase-hex form. That form is the digest this compiler stores from the policy id parameter, and the digest `query-pg` requires on `identity.policy_id` (`contracts/gateway/query-pg-cli-v1.md`). One approved digest this rule accepts is `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`. A slug-only policy id remains legal in these settings and is refused as a dialogue config.

`ontology.compile.per_role` must be true. `ontology.compile.sample_values.default` must not be `raw`. `include_schemas` must be non-empty and unique. Every effective dataset schema must be one of those names. A failure is `settings.invalid` or `settings.schema`. `auto_accept` is stored because `OntologyFile` requires it. It classifies nothing.

## Decisions

Each decision is one object. Unknown fields are rejected. The array is at most 4096 decisions.

| Field | Rule |
|---|---|
| `dataset` | the effective dataset id, `schema.table` |
| `column` | the effective column name |
| `schema_hash` | lowercase SHA-256 |
| `fact_fingerprint` | lowercase SHA-256 |
| `pii` | existing `PiiLevel`: `none`, `personal`, `sensitive`, or `restricted` |
| `semantic_type` | existing `SemanticType` |
| `reviewer` | `{by, at}`. `by` is a `Label`. `at` is a `CivilDate` |

`by` and `at` are both required. The compiler does not insert a reviewer, a date, or a class.

The fact anchor is the review-queue item `column/{dataset}/{column}` of kind `structural_column` on the recomputed draft. The schema anchor is `provenance.schema_hash`.

| Condition | Result |
|---|---|
| Duplicate `dataset` + `column` | `decision.duplicate`. The command is refused. No later decision wins |
| Dataset or column is not on the effective draft | `decision.unknown`. The command is refused. A renamed column name is this refusal |
| Hash is not lowercase SHA-256, or `pii`, `semantic_type`, or `reviewer` is not the existing type | `decision.invalid`. The command is refused |
| No structural-column fingerprint for that column | `decision.unbound`. The command is refused |
| `schema_hash` differs | The decision is not applied. The column is listed in `hidden` with `decision.stale_schema` |
| `fact_fingerprint` differs | The decision is not applied. The column is listed with `decision.stale_fact` |
| Effective column `status` is `rejected` | The decision is not applied, even when the hashes match. The column is listed with `column.rejected`. A stale schema or fact is recorded first |
| Hashes match and the column is not rejected | The column receives that `pii`, that `semantic_type`, and `classification.state=confirmed` with `classification.by` and `classification.at` set from `reviewer` |

The structure-draft merge removes an item whose `status` is `rejected` from the effective list. A review action `reject` on `column/{dataset}/{column}` therefore leaves that column off `effective.datasets`. This compiler reads that list and does not put the column back. A decision that names the removed column is `decision.unknown`. Allowing that name in a role pack is `visibility.unknown_column`.

If an effective column is still present with `status` `rejected`, the decision is not applied and `hidden` uses `column.rejected`. The current structure-draft merge does not emit that shape. The compiler still honors it so a rejected column cannot become classified.

A column that remains and has no applied decision stays structurally present and unclassified: `pii`, `semantic_type`, and `classification` remain null. It is listed in `hidden` with `classification.absent`. Allowing that column in a role pack is `classification.unknown_pii`. This compiler does not drop that column, because the role pack's hidden-name closure needs the name.

An applied decision does not set `review`. Column prose is not approved. `provenance` on the column stays the effective layer's value. Labels, data types, units, descriptions, and status stay the effective layer's values. A `semantic_type` of `money` on a column with no unit fails the existing `validate_dataset` rule and is `oos.invalid`. This compiler does not invent a unit.

## Relationships

Effective relationships are parsed with the existing `Relationship` type and checked with `validate_relationships`. A relationship whose endpoint dataset or column is not on the effective datasets is omitted and listed in `omitted_relationships` with `endpoint_not_effective`. Relationships that remain are copied with their effective `status`, `provenance`, and `review`. A `declared_fk` is not given `review.state=approved`. The same merge removes a relationship whose `status` is `rejected`, so a rejected review does not leave a relationship for this compiler to copy. A relationship that is still present with `status` `rejected` is copied with that status. Visibility closure of an allowlisted pack is `compile_role_pack`, not a second grant in this compiler.

Datasets and relationships are checked with `validate_dataset` and `validate_relationships` before the document is returned. A failure is `oos.invalid` at that function's path. The effective document is also scanned with `oo.builder.effective-oos/v1`. A document over 1048576 bytes is `budget.bytes`.

Metrics, glossary terms, examples, enums, entities, and a calendar are omitted. The sealed draft does not contain them, and this compiler does not invent them.

## Result

Stdout is one canonical JSON object, sorted keys, no trailing newline. Nothing is written until that object parses.

| Key | Meaning |
|---|---|
| `protocol` | `oo.builder.reviewed-effective-oos-command/v1` |
| `compiler` | `oo-builder/reviewed-effective-oos/v1` |
| `effective_oos` | the `oo.builder.effective-oos/v1` object |
| `hidden` | `{dataset, column, reason}` for every effective column that was not classified, sorted by dataset then column |
| `omitted_relationships` | `{name, reason}` for dropped endpoint rows |
| `provenance` | the hashes and the non-claims below |

`provenance` carries `sealed_source_sha256` (canonical SHA-256 of `artifact`), `decisions_sha256` (canonical SHA-256 of the `decisions` array, input order preserved), `generated_sha256`, `curated_sha256`, `draft_hash`, `wrapper_hash`, and `schema_hash`. Generated and curated stay separate hashes. `authentication`, `authorization`, `grants`, `publication`, and `semantic_publication` are `not_claimed`.

`effective_oos` has no `decisions`, `samples`, `credentials`, or `privileges` field. Those names are rejected by `compile_role_pack`.

## Exits

A failure writes one static diagnostic to stderr and writes nothing to stdout. The diagnostic is `reviewed-effective-oos: <code> <message>`. The body is not copied.

| Exit | Cause |
|---|---|
| 0 | The command result was written |
| 1 | Stdin could not be read, is not UTF-8, the command or compiler rejected it, or stdout could not be written |
| 2 | An argument was given |

## Outside this slice

Authentication, grants, row-security proof, semantic publication, and gates G1–G13 stay open. A successful command is not a role pack and not a publication. `compile_role_pack` still requires a caller-supplied visibility snapshot. That snapshot is not a live permission proof.
