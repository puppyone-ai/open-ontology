# PostgreSQL builder

Contract `oo.pg-builder/v1`. Compiler id `oo.pg-builder/v1`.

This unit reads one PostgreSQL catalog through the existing `oo-pipeline catalog-pg` command, reads bounded `pg_class.reltuples` and `pg_stats` null fraction, distinct count, and correlation, and builds per-table evidence cards. A caller approval admits those cards. Generation then makes one loopback chat completion per table. A table with more than eight columns is split into column batches of eight, in column order. Validated output is a review-only S5 draft: dataset candidates through `complete_description_candidates`, and field candidates staged in this unit. Explicit operator decisions then call `apply_description_review` and `apply_description_pack` on one validated table call. An aggregate of several tables has no single complete command and is not a review.

A model response does not grant, certify, classify PII, choose a semantic type, or publish. `authenticated` stays false. `parent_auth` stays `required`. This is not full SP07 acceptance.

The production description provider stays HTTPS-only with its 16-byte bearer rule. This unit does not call it and does not change it. The loopback adapter below is a separate local-dev client.

## Config

`protocol` is `oo.pg-builder.config/v1`. Unknown keys are rejected. The bearer is not a field, not an argument, and not an environment variable.

| Field | Rule |
|---|---|
| `adapter` | `loopback_http_local_dev` |
| `namespace` | `pg-builder` |
| `purpose` | `generate` or `fault_injection` |
| `endpoint.origin` | `generate`: exactly `http://127.0.0.1:8317`. `fault_injection`: `http://127.0.0.1` and a port from 1 to 65535. No other host, scheme, user, path, query, or fragment |
| `endpoint.path` | `/v1/chat/completions` |
| `endpoint.model_id` | `grok-4.7` |
| `sample_policy` | `none` |
| `temperature` | JSON number `0` |
| `secret.bearer_file` | regular file, at most 512 bytes. The token is the bytes without one trailing newline |
| `declared_price.status` | `unknown` or `declared_non_billing` |
| `budget` | every field below |
| `postgres` | present for `profile` and `catalog` |

`declared_non_billing` also requires `currency` `USD` and `per_total_token`, a non-negative decimal with at most 8 fractional digits and no exponent. `unknown` forbids those fields. Neither status is an invoice. `billing_certificate` is not a config field.

The token is 1 to 256 bytes of visible ASCII (`0x21` through `0x7e`) with no whitespace. A shorter local token is accepted only by this loopback adapter. If the config file, the prompt, or the model input already contains the token bytes, the call is `config.secret` and no socket is opened.

### Budget

| Field | Range |
|---|---|
| `deadline_ms` | 1 to 30000, or 1 to 180000 only for the local private model below |
| `max_request_bytes` | 1 to 262144 |
| `max_response_bytes` | 1 to 262144 |
| `max_prompt_tokens` | 1 to 100000 |
| `max_completion_tokens` | 1 to 100000 |
| `max_total_tokens` | 1 to 200000 |
| `max_tables` | 1 to 16 |
| `max_in_flight` | 1 |

`deadline_ms` bounds one HTTP exchange. The operator-written value is 30000 unless a config explicitly sets another allowed value. A value from 30001 to 180000 is accepted only when `purpose` is `generate` and `endpoint.origin` is `http://127.0.0.1:8317`. Fault injection and every other origin keep the ceiling 30000. A value above 180000 is refused. This ceiling is the Builder LLM HTTP deadline only. It does not change PostgreSQL source budgets: profile SQL keeps `statement_timeout` 10 seconds, and the catalog request keeps `connect_ms`, `statement_ms`, and `lock_ms` at 30000. Parent source query, lock, and snapshot budgets are not retuned. There is one attempt per table batch and no retry. A timeout is a structural draft, is not stored, and is not rewritten into a success.

### PostgreSQL

`host` is a DNS label sequence or `127.0.0.1`. `port` is 1 to 65535. `database` and `username` are 1 to 63 characters of ASCII letters, digits, and underscore. `password_file` and `ca_file` are regular files. `catalog_bin` is the `oo-pipeline` binary. The child argv is that binary and `catalog-pg` only. The password and the bearer are not argv and are not child environment variables. `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `OPENAI_API_KEY`, and `OPENAI_BASE_URL` are removed from the child environment.

Profile SQL is fixed. It reads `reltuples`, `null_frac`, `n_distinct`, and `correlation` as text. It does not select `most_common_vals`, `most_common_freqs`, or `histogram_bounds`. Comment text is read separately and marked `untrusted_data`. Statement timeout is 10 seconds. Sample policy `none` sends no row samples.

## Cards

`oo.pg-builder.cards/v1` lists at most `max_tables` cards ordered by `(schema, name)`. Each card has `dataset`, `schema`, `name`, `schema_ident`, `name_ident`, the untrusted table comment or null, `reltuples` or null, column stats, foreign keys copied from the sealed compiler card, and `card_sha256`. `card_sha256` is the sorted-key SHA-256 of the card with that field removed.

`most_common_vals` and `histogram_bounds` are the string `withheld`. A profile document that carries those values is `profile.values`. Comment text is data. It is not appended to the prompt and it is not an instruction.

`oo.pg-builder.card-approval/v1` must set `approved` true, `authenticated` false, and `parent_auth` `required`. The evidence hash, schema hash, cards hash, and each card hash must match. Any other authentication claim is `approval.authentication`.

## Model input

The system message is exactly the frozen prompt and nothing from the catalog. `prompt_version` is `oo.pg-builder.prompt/v2`. Each user message is one `oo.pg-builder.model-input/v2` object for one column batch of one table: `role` `data`, the prompt hash, `model_id`, `sample_policy` `none`, `batch` (`index`, `count`, `column_from`, `column_to`), `echo`, and `card`. `card` contains that batch's columns only: name, attnum, type name, null fraction, distinct count, correlation, and the untrusted comment. It also contains the dataset, reltuples, the untrusted table comment, comment authority, and foreign keys. It does not contain row samples, most-common values, histogram bounds, other tables, the sealed artifact, or the description-candidate request body.

`echo` carries the hashes the model must copy: compiler `request_sha256`, `schema_hash`, `evidence_hash`, compiler `prompt_version`, compiler `prompt_sha256`, `model_id`, `source_id`, `budget_sha256`, `dataset`, `compiler_card_sha256`, and `evidence_card_sha256`. The prompt tells the model to copy those values unaltered and to emit one dataset candidate with `dataset`, `card_sha256`, `label`, `description`, `grain`, `confidence`, `caveats`, and `open_questions`. `compiler_card_sha256` is the description compiler's card hash. `evidence_card_sha256` is this unit's card hash. They are different. The description-candidate request is produced locally by `prepare_description_request` on a selection narrowed to that one table. Narrowing does not drop the sealed artifact from the command the compiler sees. The model call does not repeat the multi-table command. The prepared `model_id` must be `grok-4.7`. There is no fallback model and no fallback origin.

The chat body uses `temperature` 0, `stream` false, and `response_format.type` `json_object`. It does not set a reasoning-effort field. The loopback client connects only to the configured `127.0.0.1` port, writes one HTTP/1.1 POST, follows no redirect, and reads no proxy variable. Column batch size is 8. A table of 8 columns or fewer is one call.

## Response

A transport status other than `complete` is passed to `complete_description_candidates` without a body. Field text is not parsed. Invalid JSON is passed through as the captured body with status `complete` so the description compiler can reject it. The bytes are not repaired.

Batch index 0 must be protocol `oo.pg-builder.s5-response/v1` with exactly `protocol`, `complete` true, `table_response`, and `field_candidates`. `table_response` is re-encoded with sorted keys and passed to `complete_description_candidates`. Keys and values are not added, removed, or rewritten. A later batch of the same table must be protocol `oo.pg-builder.s5-field-batch/v1` with exactly `protocol`, `complete` true, and `field_candidates`. A later batch does not call `complete_description_candidates` and has no complete command. `field_candidates` are checked here against that batch's columns only. A response that is not the protocol required for its batch is not turned into a synthetic description response. Model bytes are not rewritten into a valid candidate.

Each field candidate has `dataset`, `column`, `attnum`, `card_sha256`, `label`, `description`, `confidence`, `caveats`, and `open_questions`. The set must equal the approved columns. `card_sha256` is the evidence card hash, not the compiler card hash. Text bounds match the description-candidate ceilings. `confidence`, `label`, `description`, and `grain` are JSON strings. `attnum` is a JSON number. `caveats` and `open_questions` are JSON arrays. `confidence` is `0`, `1`, `0` plus one to four decimal digits, or `1` followed by one to four zeros after the decimal point. `pii`, `semantic_type`, `classification`, `publication`, `grants`, `curated`, `effective`, and `auto_accept` reject the field set.

`outcome` is `validated` only when batch index 0 received description-compiler `candidates` and every field candidate in the batch validated, or when a later batch's field candidates validated. Anything else is `structural_draft`. One table with one batch returns that batch document. Several batches return `oo.pg-builder.generation/v1` with `calls` holding each batch document. The aggregate `outcome` is `validated` only when every call is `validated`. Its `complete_command` is null. Review and pack commands refuse a generation that is not `validated`, and they refuse a null complete command. Per-call transport status, usage, and response hash are authoritative. The aggregate status is `complete` only when every call is `complete`.

## Cache

The cache key protocol is `oo.pg-builder.cache-key/v2`. The key is the sorted-key SHA-256 of `card_schema`, `prompt_version`, `prompt_sha256`, `model_id`, `sample_policy`, adapter, origin, path, namespace `pg-builder`, the cards document `evidence_hash` and `schema_hash`, the table `dataset`, that table's evidence `card_sha256`, `batch_index`, `column_from`, `column_to`, and that table's description `request_sha256`. The directory is `{cache_dir}/pg-builder/`. A hit reads that file, opens no socket, sets this invocation's attempt to 0, and returns the same canonical candidate object. A changed table card, evidence hash, request hash, model, prompt, origin, or batch misses. A sibling table does not share the key. Only a `validated` batch is stored, by write to a temporary file in the same directory, `sync`, and rename. Failures, including timeouts, are not stored and do not replace a stored draft. The aggregate is not a cache entry. A cache hit does not open a socket even when a sibling batch misses. The aggregate cache is `hit` when every call hit, `miss` when none hit, and `partial` when the calls differ.

## Meter

Every opened exchange records `model_id`, request input hash, response hash unless the body contained the bearer, usage when the meter is consistent, latency, attempt 1, finish reason, HTTP status, and transport status. `declared_price.status` `unknown` records usage and sets `amount` to null. It does not record zero and it does not record an invoice. `declared_non_billing` records `total_tokens` times `per_total_token` with `billing_certificate` false. A cache hit copies the stored meter and sets this invocation's socket to `not_opened`.

## Review and pack

`review` builds `oo.builder.description-review-command/v1` from the stored complete command and caller decisions `approve`, `reject`, or `defer`. Hashes are computed from the compiler output and the curated documents. The reviewer label and date are the caller's. `apply_description_review` sets `authenticated` false.

`pack` calls `apply_description_pack` twice when asked: once with every decision forced to `defer` (the before pack) and once with the caller decisions (the after pack). Classification decisions are the caller's `pii` and `semantic_type` plus the draft fact fingerprint. A role column that has no classification decision is refused before the pack call. Unlisted columns stay out of the visibility snapshot.

## Field publication seam

`oo-builder/description-candidate/v1` admits dataset candidates only. A column object is an unknown key. `oo-builder/description-review/v1` writes dataset `label`, `description`, and `grain` only. `oo-builder/description-pack/v1` copies those three dataset fields into the captured overlay. This unit therefore stages field candidates and does not insert them into curated documents, the resealed artifact, or the role pack.

The expected column candidate, which this unit validates and stages, is one object with exactly `dataset`, `column`, `attnum`, `card_sha256`, `label`, `description`, `confidence`, `caveats`, and `open_questions`. `card_sha256` is the evidence card hash. Publishing that object requires a delivered compiler change that admits those keys and applies them only through an explicit review. That change is outside this unit. Until it is delivered, a role pack that contains field candidate text is not produced, and dropping the field text to claim pack acceptance is forbidden. `apply_description_review` embeds one description request. Distinct per-table request hashes are not concatenated into one captured body. A combined multi-table review is withheld at seam `description-review.single-request`. Per-table review uses that table's real complete command and an operator file whose decisions name only that dataset.

## Failures

Invalid schema, a prompt-injection shaped key, a truncated completion, a timeout, and an unavailable socket return `structural_draft` or a refused command. They do not write the candidate cache, do not call review, and do not call pack. Curated bytes, review files, role ACL, active publication, and compiled packs are unchanged by those paths. A historical timeout document stays a timeout. This unit does not edit it and does not store it.

## Out of scope

No customer endpoint, no raw row, no sample value, no second model endpoint, no retry, no proxy, no grant, no authenticated RBAC, no invoice, and no parent acceptance. The numeric query blocker recorded by the real PostgreSQL fixture remains a separate seam.

## CLI

`pg-builder` takes one command and paths. Exit 0 writes a structured document, including a handled model failure. Exit 1 is a config or seal refusal. Exit 2 is usage. The bearer is read from `secret.bearer_file` and is not an argument.

Commands: `catalog`, `profile`, `comments`, `seal`, `cards`, `curated`, `approve`, `generate`, `review`, `pack`, `scan`. `approve` writes `authenticated` false and `parent_auth` required. `generate` opens the loopback socket only after approval. `review` and `pack` refuse a generation whose `outcome` is not `validated`. `scan` compares file bytes with the bearer file and prints `bearer_scan=clean` or `bearer_scan=found` plus a path. It does not print the token.
