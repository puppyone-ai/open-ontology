# Self-build rules

Contract `oo.selfbuild-rules/v1`.

This unit reads three sealed documents and writes review-only S2, S3, and S4 cards. It does not query a source, call a model, publish, grant, certify a metric, or merge entities. It does not emit a knowledge graph or a join path. SP07 is not accepted by a successful run.

The catalog pin is commit `149944271ef1c2cb555954d65248413df2166f1c`. The card pin is commit `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. Those commits are stamped on every review document. This unit does not edit them.

## Input

Three JSON documents, each at most 1_048_576 bytes. Duplicate keys, trailing bytes, depth above 32, and more than 100_000 JSON nodes are `json`. More than 64 tables or 256 columns on one table is `bound.tables` or `bound.columns`.

| Document | Protocol |
|---|---|
| Catalog | `oo.source.pgsql/v1` |
| Profile | `oo.pg-builder.profile/v1` with `sample_policy` `none` |
| Cards | `oo.pg-builder.cards/v1` with `sample_policy` `none` and `schema_version` `oo.pg-builder.card/v1` |

`auto_accept` is `rule_high_confidence` or `none`. Any other token is `schema`.

Identity is stale, and no review document is written, when any of these differ:

- catalog `schema_hash` and cards `schema_hash`
- a card `card_sha256` and the sorted-key SHA-256 of that card with `card_sha256` removed
- catalog and card column name, attnum, type name, or `not_null`
- catalog and card `foreign_keys`
- `schema_ident` or `name_ident` and the catalog name
- a profile column attnum and the catalog attnum
- a profile table that is not in the catalog
- profile and card `reltuples` when both are JSON strings

An empty profile `tables` array is zero profile. It is not stale. Stats are then absent.

`most_common_vals` and `histogram_bounds` must be the string `withheld`. Any other value is `profile.values`. A sample policy other than `none` is `profile.samples`.

Comment objects and `untrusted_comment` strings are not read into a decision and are not copied. Names are ASCII identifiers of 1 to 63 characters. `schema_hash` and `evidence_hash` are 64 lowercase hex characters.

Sorted-key JSON uses this writer: object keys are sorted, strings escape `"`, `\`, and the JSON controls, and other characters are copied. SHA-256 is FIPS 180-4. The known object `{"a":1,"b":2}` hashes to `43258cff783fe7036d8a43033f830adfc60ec037382473548ac742b888292777`.

## Column rules

The first match wins. Personal and sensitive names are never turned into `pii` `none`.

| Order | Condition | semantic_type | pii | confidence | uncertainty |
|---|---|---|---|---|---|
| 1 | name is `national_id`, `id_card`, `sfzh`, `ssn`, `passport`, `password`, `secret`, or ends with `_national_id`, `_id_card`, `_sfzh`, `_ssn`, `_passport`, `_password`, `_secret` | `unknown` | `sensitive` | `1` | `1` |
| 2 | phone names `phone`, `mobile`, `tel`, `lxdh`, `telephone`, or those suffixes | `phone` | `personal` | `1` | `0` |
| 2 | `email` or suffix `_email` | `email` | `personal` | `1` | `0` |
| 2 | `display_name`, `person_name`, `full_name`, `first_name`, `last_name` | `person_name` | `personal` | `1` | `0` |
| 2 | `address`, `addr`, or suffix `_address` | `address` | `personal` | `1` | `0` |
| 3 | exact name `dh` | `unknown` | `unknown` | `0` | `1` |
| 4 | type `bool` | `boolean_flag` | `none` | `1` | `0` |
| 5 | type `date`, `timestamp`, or `timestamptz`. Audit names `created_at`, `cjsj`, `create_time`, `updated_at`, `xgsj`, `update_time` select `audit_time`. A `date` otherwise selects `date`. A timestamp otherwise selects `event_time` | that type | `none` | `1` | `0` |
| 6 | name `je`, `amount`, suffix `_je` or `_amount`, and type `numeric`, `int2`, `int4`, or `int8` | `money` | `none` | `1` | `0` |
| 6 | name `sl`, `qty`, suffix `_sl` or `_qty`, and type `numeric`, `int2`, `int4`, or `int8` | `quantity` | `none` | `1` | `0` |
| 6 | name `ratio` or suffix `_ratio`, and type `numeric` | `ratio` | `none` | `1` | `0` |
| 7 | name `zt`, `status`, suffix `_zt` or `_status`, type `text` or integer, and proven low cardinality | `status_code` | `none` | `1` | `0` |
| 7 | name `code` or suffix `_code`, type `text` or integer, and proven low cardinality | `code` | `none` | `1` | `0` |
| 8 | declared foreign key and equal type names | `foreign_key` | `none` | `1` | `0` |
| 9 | declared foreign key and unequal type names | `unknown` | `unknown` | `0.5` | `1` |
| 10 | sole primary key, name is `id` or ends with `_id` or `_key`, or the type is an integer | `identifier` | `none` | `1` | `0` |
| 11 | one naming parent | `foreign_key` | `none` | `0.5` | `0.5` |
| 12 | two or more naming parents | `unknown` | `none` | `0.5` | `0.5` |
| 13 | otherwise | `unknown` | `unknown` | `0` | `1` |

`state` is `auto_accepted` only when `auto_accept` is `rule_high_confidence`, `pii` is `none`, confidence is `1`, uncertainty is `0`, and `semantic_type` is one of `boolean_flag`, `money`, `quantity`, `ratio`, `event_time`, `date`, `audit_time`, `identifier`, `foreign_key`, `status_code`, `code`. Every other column is `pending`. `auto_accept` `none` forces `pending`. A personal or sensitive column is never `auto_accepted`.

Low cardinality is a positive `n_distinct` from 1 to 32, or a negative fraction times `reltuples` in that same range. `n_distinct` `-1` is unique and is not low. Absent stats do not prove low cardinality. `bool` is not an enum.

## Relations

A declared foreign key is provenance `declared_fk`, confidence `1`, status `candidate`. Cardinality is `one_to_one`, `many_to_one`, `one_to_many`, or `unknown` from sole-primary-key and `n_distinct` `-1` uniqueness. Missing uniqueness yields `unknown` and uncertainty `1`. Unequal type names set `type_compatible` false and raise uncertainty to at least `0.5`. The candidate is kept.

A naming candidate exists when the column is not in a declared key and is not a sole primary key, the other table has a sole primary key, and either the names are equal or the column is `{table}_id` for a primary key named `id`. The types must be equal. Provenance is `naming`, confidence `0.5`, uncertainty `0.5`.

Two or more candidates for the same child columns set `conflict` true on each. `selected` on the conflict record is null. No candidate is dropped and no entity is merged.

A bridge is a table whose primary key is exactly two declared single-column foreign keys, each aimed at a sole primary key. The unit adds one `inferred` candidate of cardinality `many_to_many`, confidence `0.5`, uncertainty `0.5`, between those parents through that bridge. The two foreign keys remain `many_to_one`. `entity_merge` stays `not_performed`.

No containment statistic is invented. No join path is emitted.

## Enums and dictionaries

A non-personal, non-ambiguous `text` or integer column with proven low cardinality gets an enum signature. `status` is `undetermined`. `approved` is false. `class` is `undetermined`. Values are `withheld`. `time_window` is `unobserved`. Jaccard is null.

A dictionary candidate is a table named `sys_dict` or `t_code`, or a name ending in `_dict`, with `reltuples` from 0 to 5000 and both a `code` or `dm` column and a `name`, `label`, or `mc` column. The link is `uncompared`. It is not an approved mapping.

## Expert cards

Every pending column, every enum, every relation whose uncertainty is not `0` or whose `conflict` is true, and every conflict has one expert card. Required fields are `stable_id`, `question`, `evidence`, `uncertainty`, `anchors`, and `provenance`. `provenance` is `rule`. `anchors` is non-empty. Questions are these sentences, with the names filled in:

- personal: `Confirm the PII class of {dataset}.{column}. The rule marked it personal and did not grant access.`
- sensitive: `Confirm the PII class of {dataset}.{column}. The rule marked it sensitive and did not grant access.`
- other pending column: `What is the semantic type and PII class of {dataset}.{column}?`
- enum: `What do the distinct values of {dataset}.{column} mean? Values are withheld. The signature is undetermined and not approved.`
- relation: `Is {from} a foreign key to {to}? Cardinality is {cardinality}. Type compatibility is {true|false}.`
- bridge: `Do {left} and {right} form a many-to-many relation through {bridge}? This candidate does not merge entities.`
- conflict: `Column {dataset}.{column} has more than one relation candidate. No candidate was selected.`

## Output

Protocol `oo.selfbuild-rules.review-cards/v1`. `effect` is `review_only`. `authenticated` is false. `parent_auth` is `required`. `publication` is `unavailable`. `grants` is `not_authorized`. `certification` is `none`. `entity_merge` is `not_performed`. `metric_certification` is `not_performed`. `knowledge_graph` is `not_emitted`. `comment_disclosure` is `withheld`. `sample_policy` is `none`.

`pii_leak_count` counts columns whose `pii` is `personal` or `sensitive` and whose `state` is `auto_accepted`. A non-zero count is `pii.leak` and yields no document.

Stable ids:

| Kind | Id |
|---|---|
| column | `col/{schema}.{table}.{column}#{attnum}` |
| relation | `rel/{schema}.{table}.{columns}/to/{ref_schema}.{ref_table}.{ref_columns}/{provenance}` |
| bridge | `rel/{bridge}/{left_dataset}.{left_column}/to/{right_dataset}.{right_column}/inferred` |
| enum | `enum/{schema}.{table}.{column}#{attnum}` |
| dictionary | `dict/{schema}.{table}` |
| conflict | `conflict/{schema}.{table}.{column}#{attnum}` |
| expert | `ask/{kind}/{anchor}` |

Column lists inside a relation id are joined by `+` in constraint order. Bridge endpoints are ordered by dataset, then column.

Arrays are sorted by `stable_id`.

## Scorer

Protocol `oo.selfbuild-rules.score/v1`. The scorer reads a review document and a hand-written `oo.selfbuild-rules.truth/v1` document. It does not call the producer. It compares column `semantic_type`, `pii`, and `state`; relation provenance, cardinality, and conflict; enum `status` and `approved`; dictionary `status`, `approved`, and `match`; and a null conflict selection. Sets must be equal both ways.

`pii_leak_count` is recomputed from the truth's personal and sensitive ids. A truth id whose actual `pii` is `none`, or whose `state` is `auto_accepted`, counts as one leak. Any actual personal or sensitive column that is `auto_accepted` also counts. The public fixture's expected leak is 0. Unknown ids must stay `semantic_type` `unknown`, `pii` `unknown`, and `pending`.

The canonical review text must not contain any truth `forbidden_substrings` entry. Expert cards must have a non-empty question, an evidence object, uncertainty `0`, `0.5`, or `1`, a non-empty anchors array, and provenance `rule`.

`matched` is true only when there is no mismatch and the leak count is 0.

## CLI

`selfbuild-rules produce --catalog PATH --profile PATH --cards PATH --auto-accept TOKEN`

`selfbuild-rules score --truth PATH --actual PATH`

Exit 0 is a review document or a matching score. Exit 1 is a refusal or a score mismatch. Exit 2 is usage. Stderr is `selfbuild-rules:` and a static code. Paths and comment text are not written. A refusal writes no stdout.

## Out of scope

No source query, no model, no sample, no raw comment, no automatic PII grant, no entity merge, no metric certification, no publication, and no parent SP07 acceptance.
