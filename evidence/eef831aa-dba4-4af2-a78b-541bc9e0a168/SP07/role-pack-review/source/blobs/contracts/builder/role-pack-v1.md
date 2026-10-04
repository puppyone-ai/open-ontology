# Role Knowledge Pack

Contract `oo.builder.role-pack/v1`. Compiler id `oo-builder/role-pack/v1`.

`compile_role_pack` reads two JSON documents and returns one in-memory pack. The visibility document is a snapshot a future policy adapter would supply. This compiler does not authenticate the caller, does not grant, does not read a database, and does not treat `review.state: approved` or `classification.auto_accept` as permission to see a column.

This slice does not finish SP07-W06. Authentication, the policy adapter, filesystem mount, the read API, search, real RLS proof, token budgets, file splitting, domain files, and publication gates G1–G13 stay open.

## Inputs

Both documents are scanned with `oo_core::json_scan::parse_json_value` before serde. Duplicate keys, depth, node count, trailing data, and documents over 1048576 bytes fail with the scanner's own code. The compiler replaces the scanner message with `the JSON document was rejected` and does not echo the body.

| Document | Protocol |
|---|---|
| Effective OOS | `oo.builder.effective-oos/v1` |
| Visibility snapshot | `oo.builder.role-visibility/v1` |

The effective document carries `project`, `ontology_version`, `catalog_version`, `policy`, `knowledge`, one `ontology.yaml` object, and the OOS objects this compiler can read: datasets, relationships, metrics, glossary terms, verified examples, enums, entities, and one calendar. Those objects use the existing `oos/v1` serde types. `decisions`, `resolution`, `guides`, `samples`, `profiles`, `domains`, `candidates`, `credentials`, and `privileges` are `unsupported.field`. They are not interpreted.

The snapshot carries the same project, ontology version, catalog version, policy id, policy version, knowledge id, and knowledge version, plus `principal`, `role`, `issued_for_compiler`, a dataset allowlist, and byte limits. `samples`, `most_common_vals`, `credentials`, `privileges`, and `grants` are `unsupported.field`. A per-dataset `sample_values`, `most_common_vals`, or `samples` field is `unsupported.samples`.

`issued_for_compiler` must be `oo-builder/role-pack/v1`. A mismatch is `binding.compiler`. Any other binding field that differs from the effective document is `binding.project`, `binding.ontology_version`, `binding.catalog_version`, `binding.policy`, or `binding.knowledge`. The compiler does not accept a stale policy or knowledge version.

Identity tokens:

| Field | Pattern |
|---|---|
| project, principal, policy id, knowledge id | `^[a-z][a-z0-9_-]{0,62}$` (length 1..=63) |
| role | `^oo_r_[a-z][a-z0-9_]{0,57}$` and length <= 63 |
| versions, including a statistics-proof `ref` | `^[A-Za-z0-9._+-]{1,64}$` |

`compile.per_role` must be true (`compile.per_role`). `compile.sample_values.default: raw` refuses the pack (`sample.raw_refused`). `masked` and `none` continue, and no sample is emitted either way. `include_schemas` must be non-empty and unique. `include_unreviewed` accepts only `column_descriptions`, `dataset_descriptions`, `caveats`, and `relationships`, each once.

Datasets, relationships, metrics, glossary terms, enums, entities, and the calendar are checked with the existing `validate_*` functions. A failure is `oos.invalid` at that function's path. Unknown serde fields are `schema.rejected`. Text that contains `secret://`, `postgres://`, `mysql://`, or `password=` is `source.credential` and is not copied.

## Visibility

Each allowlist row names one effective dataset and a non-empty column list. Unknown datasets, unknown columns, duplicates, and rejected columns fail. An allowlisted column with `pii` null or absent is `classification.unknown_pii`. Any allowlisted column whose classification is missing or not `confirmed` is `classification.incomplete`. The compiler does not invent a class and does not apply `classification.auto_accept`.

`row_security` is `none` or `rls`. `global_stats` is `permitted` or `denied`. These flags are snapshot claims. The OOS dataset type has no RLS flag and no sample field, so the compiler cannot discover them from the effective document. `rls` plus `permitted` requires `global_stats_proof: {ref, proven: true}`. Any other proof, including a proof on a non-RLS dataset or a proof with `proven: false`, is `evidence.unproven`. The manifest sets `row_security_reproven` to false. This compiler does not re-prove the claim and does not query PostgreSQL.

Columns absent from the allowlist stay out, including columns whose review is `approved` and whose PII is already classified. Structural review does not grant.

## What is compiled

Only allowlisted datasets are written. Hidden columns are removed from those files. The compiler does not write a count or a name of a hidden column.

Dependency closure is fail-closed and is not a SQL parser. Every ASCII identifier in the text is compared, case-insensitively, with every hidden column name in the effective document. A hidden column named `id` drops any text that uses that token. Qualified `schema.table` is a dataset reference only when that pair is a known dataset or `schema` is in `include_schemas`. An unknown table in an included schema drops the object. `pg_catalog`, `information_schema`, and schemas that start with `_oo` drop the object. `oo_dict` and `oo_entity` are not visibility edges. An alias such as `r.amount` is not a dataset when `r` is outside `include_schemas`, but `amount` still matches a hidden column of that name.

| Object | Kept when |
|---|---|
| Relationship | Every endpoint column is allowlisted, the relationship is not rejected, and it is review-approved, `declared_fk`, or `include_unreviewed` contains `relationships` |
| Metric | `certified` is true, status is not `draft`, and the SQL, label, description, caveats, synonyms, control SQL, time dimension, and enum filters stay inside the visible set |
| Glossary term | `status` is `approved`, `maps_to` names a visible dataset or a kept metric but not both, and the term text has no hidden column identifier |
| Verified example | `uses` entries are `metric:`, `rel:`, `dataset:`, or `column:` references that were kept, and the id, SQL, and question have no hidden column identifier |
| Enum | The `schema.table.column` path is an allowlisted column |
| Entity | The primary dataset, key, display, every `datasets` entry, and every member key, name, phone, and strong key are visible. Otherwise the whole entity is omitted |

`declared_fk` without an approved review is labeled source-declared. That label is not a grant. Other unapproved relationships included through `include_unreviewed` are labeled `UNREVIEWED`. Rejected relationships are skipped.

Certified metrics with `deprecated` status stay, with a deprecation line. Draft and uncertified metrics are omitted. Owner, `verified_by`, `review.by`, and `classification.by` are omitted. Control SQL is not copied and is not executed. The metrics file says reconciliation is not claimed. A metric whose enum has `undetermined` or `unexplained` values, or a non-empty `unexplained_values` list, gets an unconfirmed-code warning.

Column prose is included when `review.state` is `approved`. Provenance `declared` is not approval. Other column prose is included only when `include_unreviewed` contains `column_descriptions`, and then it is prefixed `UNREVIEWED:`. Dataset descriptions and caveats have no review field. They are omitted unless `dataset_descriptions` or `caveats` is listed, and then they are quoted as `UNREVIEWED` and fenced. A fence that contains `` ``` `` is rewritten with `'''`. User-authored labels, grain, source labels, descriptions, enum codes, and enum labels that contain a hidden column identifier are omitted.

PII other than `none` is named in the column note. Samples are omitted for every column. `most_common_vals` is not a field on the OOS model and is not emitted.

`row_count_estimate` is emitted only when that dataset's snapshot allows global statistics under the rule above. Enum `coverage` and per-code evidence row counts follow the same rule for the enum's dataset. Relationship `name_match` and `type_compatible` are included only when both ends allow global statistics. `containment`, `seen_in_query_log`, and `confidence` are never copied. A deprecated column names `replaced_by` only when that path is allowlisted. Otherwise the note says the replacement is not visible and does not name it.

A missing calendar is reported. The compiler does not invent a civil month.

## Output

These files are always produced. Table, enum, and entity files are added for the objects that survive selection. `domains/` is not produced. Examples are one file, `examples/verified.sql`, not one file per domain.

```text
README.md
INDEX.md
tables/<schema>.<table>.md
metrics.md
calendar.md
glossary.md
entities/<entity>.md
enums/<schema>.<table>.<column>.md
examples/verified.sql
relationships.md
manifest.json
```

Markdown and SQL use `\n` and end with one newline. `manifest.json` is canonical JSON with no trailing newline. Paths are Unicode NFC, unique, and relative. `normalize_pack_path` rejects an empty path, a path over 256 bytes, NUL, other controls, `\`, `:`, a leading `/`, a leading `./`, a trailing `/`, `//`, and any empty, `.`, or `..` segment. The error path is `/` and does not echo the rejected text. `RolePack::file` normalizes and then looks up that exact key. It does not touch a filesystem, follow a symlink, or search.

`manifest.json` lists every content file and not itself. Each entry has `path`, `sha256`, and `bytes`. `sha256` is lowercase hex from `oo_core::api::payload_sha256`, via this crate's `canonical::sha256_hex`. The manifest also records project, role, policy, knowledge, ontology version, catalog version, compiler, `spec_version: oos/v1`, and these fixed statements: `authentication` and `authorization` and `publication` are `not_claimed`, `mount` is `not_performed`, `samples` is `omitted`, `domains`, `file_splitting`, and `search` are `unsupported`, `row_security_reproven` is false. `tokens.status` is `unsupported` and `tokens.estimator` is `none`. `readme_token_budget` and `table_file_token_budget` are copied from the ontology as labels. They are not counted and they are not enforced. Byte length is not a token count.

`manifest_sha256` is the hash of the manifest bytes. `pack_digest` is the hash of the canonical JSON array of `{bytes, path, sha256}` for every file, including `manifest.json`, in path order. It is not a hash of the outer artifact.

The artifact is one canonical JSON string, with no trailing newline and no principal:

| Field | Meaning |
|---|---|
| `protocol` | `oo.builder.role-pack/v1` |
| `compiler` | `oo-builder/role-pack/v1` |
| `pack_digest` | Digest defined above |
| `manifest_sha256` | Hash of `manifest.json` |
| `entry_count` | File count, including the manifest |
| `total_bytes` | Sum of file byte lengths, including the manifest |
| `files` | `{path, sha256, bytes, text}` in path order |

The same files produce the same artifact. Project and role are inside the files, so two projects do not share an artifact. Principal is not inside the files. Two principals with the same role snapshot share the artifact and the pack digest, and they do not share a cache key.

The cache key is one line:

```text
oo.builder.role-pack-cache/v1|project=<project>|principal=<principal>|role=<role>|policy=<id>@<version>|knowledge=<id>@<version>|ontology=<ontology_version>|catalog=<catalog_version>|compiler=oo-builder/role-pack/v1|pack=<pack_digest>
```

`cache_key_sha256` is the hash of that line. The caller limits are required and must fit the ceilings: `max_file_bytes` 1..=262144, `max_total_bytes` 1..=1048576, `max_entries` 1..=128, and `max_total_bytes >= max_file_bytes`. A file over `max_file_bytes` is `budget.file`. The pack over `max_total_bytes` is `budget.total`. Too many files is `budget.entries`. The compiler does not split a file. Object ceilings, exceeded before rendering, are `budget.objects`: 64 datasets, 256 columns on a dataset, 256 relationships, 128 metrics, 256 terms, 128 examples, 128 enums, 256 values on an enum, 64 entities.

## API

```rust
pub fn compile_role_pack(
    effective_json: &str,
    visibility_json: &str,
) -> Result<RolePack, RolePackError>;

pub fn normalize_pack_path(path: &str) -> Result<String, RolePackError>;
```

`RolePack` exposes `artifact`, `pack_digest`, `cache_key`, `cache_key_sha256`, `manifest_sha256`, `total_bytes`, and `entry_count`. `file` returns one normalized path or `path.absent`. The file map is private. `RolePackError` is `{code, path, message}` with a static message. Compiler codes use the prefix `oo.builder.role-pack/v1/`. Scanner failures keep the scanner code.

No new crate dependency is required. Hashing goes through the existing canonical helper.

## Outside this slice

The OOS model has no sample payload, no `most_common_vals`, no RLS flag, no domain object, no unverified-example type, and no tokenizer. This compiler refuses or omits those gaps. It does not claim a publish gate, a token count, a split file, a domain chapter, a search result, a mount, an authentication, or a proof that row security holds. A later mount or read API can use the same artifact bytes. This crate does not perform that mount.
