# selfbuild-suggestions v1

Contract name: `oo.selfbuild-suggestions/v1`.

This crate is the E50-D1/1 generated-only producer for docs/06 S6 entity and domain candidates, S7 metric hypotheses, and S8 question plus SQL-template candidates, plus the S9 review-queue input those candidates feed. It is not a publisher, not an authenticator, not a grant installer, not a curator, and not the whole of SP13.

`knowledge_verified` is always false. `certified` is always false. `auto_merge` is always false. `curated_written`, `published`, `grants_issued`, and `roles_changed` are always false. A Gateway run can set `gateway_admitted` on an example only after the guard probe below has been refused by the existing `query-pg` binary. Admission is not verification and is not certification.

## What this crate does not do

- It does not write `generated/` or `curated/`, commit Git, or call a model.
- It does not query a source database and does not scan the 100k fixture rows.
- It does not merge entities on phone, person name, national id, or any other weak or unknown key.
- It does not reopen a negative pair.
- It does not copy feedback text into SQL, metric certification, grants, or role policy.
- It does not treat an unvalidated example as verified. With no operator config, every example stays `validation: not_run` and `gateway_admitted: false`.
- It does not implement G1–G13, Knowledge Pack compilation, or semantic publication.

S6 uses the declared foreign-key graph and closed identifiers only. S7 emits hypotheses. S8 validation, when requested, is a subprocess of the already compiled Gateway `query-pg` (`oo.gateway.query-pg/v1`), which loads a policy catalog and runs `DedicatedSession::execute_trusted_select`. This crate does not reimplement that runner.

## Dependencies

Direct pins, exact versions already in the repository root lock:

| crate | version |
|---|---|
| serde | 1.0.229, feature `derive` |
| serde_json | 1.0.151 |
| sha2 | 0.10.9, default features |

No new crate, no version bump, no root manifest edit, no root lock edit, no `cargo update`. The child `Cargo.lock` contains only registry packages whose name, version, source, and checksum already appear in the root lock, plus this package. `rust-version` is 1.94.0. Edition 2024. Safe Rust only.

## CLI

```text
selfbuild-suggestions produce
selfbuild-suggestions produce --config CONFIG
```

The command is one JSON object on stdin, at most 2_097_152 bytes. `CONFIG` is a regular file path. Passwords and CA PEMs stay inside the files named by that config. They are not argv, not environment variables, and not written to stdout or stderr.

| condition | exit | stdout |
|---|---|---|
| extra argument, missing command, empty stdin, stdin over the cap, invalid JSON, duplicate key, unknown field, bad type | 1 | empty |
| seal, scope, or authorization refusal; Gateway config present but the guard probe was not refused | 2 | one outcome |
| candidates produced | 0 | one outcome |

Stderr for exit 1 is `selfbuild-suggestions: malformed <reason>`. Stderr for exit 2 is `selfbuild-suggestions: refused <reason>`. The reason is a static token.

`produce` without `--config` does not spawn a process and does not open a socket. `produce --config` spawns `query-pg` only after the local checks have passed.

## Command

Unknown fields are rejected on every object this crate defines. The embedded `oo.source.pgsql/v1` catalog may contain the connector's existing fields. Those fields are part of the catalog seal preimage. This crate reads protocol, source id, schema hash, table identity, column name, column type name, primary-key names, and foreign-key column pairs. It does not interpret comments, privileges, policies, or owner names as grants.

```json
{
  "schema": "oo.selfbuild-suggestions.command/v1",
  "scope": {
    "project": "public",
    "role": "oo_r_reviewer",
    "evidence_sha256": "64 lowercase hex",
    "source_id": "examples",
    "schema_version": "oos/v1",
    "model_scope": "none"
  },
  "authorization": {
    "authorized": true,
    "catalog_sha256": "64 lowercase hex",
    "relationships_sha256": "64 lowercase hex",
    "profile_sha256": "64 lowercase hex",
    "feedback_sha256": "64 lowercase hex"
  },
  "catalog": {},
  "relationships": {},
  "profile": {},
  "feedback": {},
  "negative_pairs": [],
  "curated_overrides": [],
  "prior_candidate_ids": []
}
```

`scope.project` and `scope.source_id` match `^[a-z][a-z0-9_]{0,62}$`. `scope.role` matches `^oo_r_[a-z][a-z0-9_]{0,30}$`. `schema_version` is `oos/v1`. `model_scope` is `none`. Any other model scope is refused. `evidence_sha256` must equal `authorization.catalog_sha256`. `authorization.authorized` must be true. Each seal is the SHA-256 of the canonical JSON of that section. A mismatch is `seal`.

Canonical JSON sorts object keys, emits no insignificant whitespace, and preserves the serde_json number and string encoding of the parsed value. Duplicate keys anywhere in the input are `duplicate_key`. Depth above 40 or more than 200_000 nodes is `budget`.

`catalog.source_id` must equal `scope.source_id` when the catalog is `oo.source.pgsql/v1`. A projection catalog uses `schema: oo.selfbuild-suggestions.catalog/v1` and the same source id field.

## Catalog projection

`oo.selfbuild-suggestions.catalog/v1` denies unknown fields.

```json
{
  "schema": "oo.selfbuild-suggestions.catalog/v1",
  "source_id": "examples",
  "schema_hash": "64 lowercase hex or the literal unsealed",
  "tables": [
    {
      "schema": "logistics",
      "name": "shipment",
      "primary_key": ["shipment_id"],
      "columns": [{"name": "shipment_id", "type_name": "int8"}]
    }
  ],
  "foreign_keys": [
    {
      "name": "shipment_customer_fk",
      "schema": "logistics",
      "table": "shipment",
      "columns": ["customer_id"],
      "ref_schema": "logistics",
      "ref_table": "customer",
      "ref_columns": ["customer_id"]
    }
  ]
}
```

At most 64 tables, 64 columns on a table, and 128 foreign keys. Identifiers match `^[a-z_][a-z0-9_]{0,62}$`. `type_name` is a lowercase PostgreSQL type token of at most 63 characters. A foreign key must reference one existing primary-key column of the parent, one column, and a type-compatible child column (`int2`/`int4`/`int8` together, or equal type names).

`oo.source.pgsql/v1` uses each table's `foreign_keys[].columns[]` pairs (`name`, `ref_schema`, `ref_table`, `ref_name`) and `primary_key[].name`. The recorded `schema_hash` is copied into provenance. This crate does not recompute `pg-catalog-hash/v1`.

## Relationships, profile, feedback

Relationships use `oo.selfbuild-suggestions.relationships/v1`. An item is a graph edge only when `project` equals `scope.project`, `provenance` is `declared_fk`, `review_state` is `approved`, and the column pair equals one catalog foreign key. Inferred, unapproved, cross-project, or non-declared items are counted and dropped. Duplicate approved pairs collapse.

Profile uses `oo.selfbuild-suggestions.profile/v1`. `sample_policy` must be `none`. A profile column must already exist on the catalog table. Semantic values are `identifier`, `money`, `quantity`, `time`, `boolean_flag`, `phone`, `person_name`, `text`, and `unknown`. There is no sample field. `reltuples`, when present, is a non-negative decimal string of at most 32 characters. It is evidence text, not a fresh scan.

When the profile omits a column, the name and type rules are:

| signal | semantic |
|---|---|
| type `bool` | `boolean_flag` |
| type `date`, `timestamp`, `timestamptz`, or name ending `_at`, `_on`, `_rq` | `time` |
| name `amount`, `je`, `money`, or type `numeric` with one of those names | `money` |
| name `qty`, `quantity`, `sl` | `quantity` |
| name `phone`, `mobile`, `lxdh` | `phone` |
| name `name`, `display_name`, `khmc` | `person_name` |
| primary key | `identifier` |
| otherwise | `unknown` |

`phone` and `person_name` are weak. A primary key is not a strong key unless the profile sets `strong_key` to `validated` on that column. This crate never writes `validated`. `national_id` and other unchecked identifiers stay `unknown`.

Feedback uses `oo.selfbuild-suggestions.feedback/v1`. Each item has `project`, `kind` (`gap`, `reject`, `failure`, `comment`), `text` of 1 to 500 characters with no NUL or other C0 control, and `target` of at most 128 closed characters or dots. At most 32 items. Cross-project items are refused into `scope_drops.feedback` and do not change metrics or grants. Exact duplicates collapse. Feedback is a queue channel `untrusted_feedback`. It is not a metric.

`negative_pairs` are two qualified table names. A pair suppresses that entity edge in both directions and emits `negative_entity`. It does not delete the catalog foreign key. `curated_overrides` are `{key, fact_sha256}`. When `key` matches a generated metric or entity key and the fact hash differs, a `stale_override` queue item is emitted. The generated candidate stays. Curated bytes are not written. `prior_candidate_ids` marks overlaps; the candidate is still emitted once.

## S6

Nodes are catalog tables. Edges are the confirmed declared foreign keys that are not negative pairs. Connected components, members sorted by `schema.name`, are entity candidates. The primary dataset is the member with no outgoing edge and the greatest incoming degree, then the earlier name. If every member has an outgoing edge, the earlier name is primary. Components of one table have provenance `isolated` and lower confidence. Domain id is `commerce` for schema `shop`, `transport` for `logistics` or `tms`, `finance` for `finance` or `erp`, `warehouse` for `wms` or `warehouse`, otherwise the schema name. A component whose members use more than one schema is domain `cross_source` and is still not merged by name. `auto_merge` is false even when a profile column says `strong_key: validated`. `strong_key_status` is `weak` when a member has a `phone` or `person_name` column, `validated_unmerged` when there is no weak column and a profile column is `validated`, and `unknown` otherwise. Weak columns are listed. No estimated pair count is invented. This status never enables a merge.

## S7

One hypothesis per `money` or `quantity` column. `certified` is false, `status` is `draft`, `owner_needed` is true, and `control_total_needed` is true. If the same table has a `time` column, `grain` is `["month"]`, `time_dimension` is that column, and `time_anchor` is the hypothesis `prev_month_start` with `time_anchor_closed` false. Otherwise `grain` is `["row"]`, the time fields are null, and `time_anchor_needed` is true. The SQL template is `SELECT sum(<column>) FROM <schema>.<table>`. It is not sent to Gateway. `validation` is `not_applicable`. Numeric and aggregate text are hypotheses, not admitted statements.

## S8

One example per remaining entity-graph edge. The question names the child column and the foreign-key constraint. The SQL template is:

```text
SELECT <column> FROM <schema>.<table> WHERE <column> IS NOT NULL LIMIT 1
```

Identifiers are the closed catalog identifiers only. Feedback and comments are not interpolated.

Without config, `validation` is `not_run` and `gateway_admitted` is false. With config, this crate first runs `guard_probe_sql` through `query-pg`. The probe must exit 6, write empty stdout, and leave no admitted candidate. Any other probe result refuses the command, admits nothing, and does not set a verified flag. Only after that refusal does this crate validate up to `max_validated` examples, each in its own request file and process. Exit 0 with summary `status` `succeeded` and `returned_rows` at least 1 sets `gateway_admitted` true and `validation` `admitted`. Exit 6 is `guard_refused`. Exit 0 with zero rows is `empty`. Anything else is `error`. Those three do not admit. `knowledge_verified` stays false in every case. Empty, refused, and error examples remain in the outcome with `gateway_admitted` false so the refusal is visible; they are not described as verified.

## S9

Queue channels are `entity`, `metric_hypothesis`, `example`, `untrusted_feedback`, `stale_override`, and `negative_entity`. `trusted_metric` is false on every item. Score is `impact * uncertainty_millis`. Sort is score descending, then `queue_id` ascending. At most 128 items.

| channel | impact | uncertainty_millis |
|---|---|---|
| stale_override | 9 | 800 |
| metric_hypothesis | 6 | 700 |
| example | 4 | 900 when not admitted, 400 when admitted |
| negative_entity | 5 | 600 |
| untrusted_feedback | 2 | 1000 |
| entity with an edge | 8 | 200 |
| isolated entity | 3 | 500 |

## Candidate ids

```text
oo.selfbuild-suggestions.candidate/v1\n
<project>\n
<kind>\n
<key>\n
```

The id is `cand_` plus the first 32 hexadecimal characters of SHA-256 of that UTF-8 preimage. Metric and entity fact hashes use the same canonical JSON of the fact body with `candidate_id` removed.

## Gateway config

Unknown fields are rejected.

```json
{
  "schema": "oo.selfbuild-suggestions.gateway-config/v1",
  "query_pg_bin": "/path/query-pg",
  "guard_probe_sql": "UPDATE shop.account SET label = label",
  "max_validated": 4,
  "request": {
    "version": "oo.gateway.query-pg/v1"
  }
}
```

`request` is a `query-pg` document. This crate replaces `request.statement.sql` and `request.statement.source` (`request`) for each spawn and writes a new audit file path under a private temporary directory. `max_validated` is 1 to 4. The probe SQL must be non-empty, at most 400 characters, and must not contain a semicolon. The binary must be a regular file. The child is started with a cleared environment and `PATH=/usr/bin:/bin`. The wait is 40 seconds, then the child is killed. Stdout and stderr are capped at 65_536 bytes. A password string is not logged because this crate never reads the password file.

## Outcome

`schema` is `oo.selfbuild-suggestions.outcome/v1`. The body contains `scope`, `effects`, `gateway`, `dedup`, `scope_drops`, `entities`, `domains`, `metrics`, `examples`, `feedback`, and `queue`. `effects.model_called`, `effects.source_queried`, and `effects.samples_retained` are false. `effects.knowledge_verified_count` is 0.
