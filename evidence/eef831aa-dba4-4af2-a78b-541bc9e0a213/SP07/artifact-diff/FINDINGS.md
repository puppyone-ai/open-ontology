# Findings

Verdict: **APPROVE_LIMITED** for the artifact-diff slice on fixed `a661dcdf72ff91561ae6b7cb79916e582884d08a`. Line numbers are `source/blobs/ontology-artifact-diff.rs` unless another file is named. The decisive run is `linux/overlay-3`: 9 passed, 0 failed, `test.exit` 0, `clippy.exit` 0, `docker-run.exit` 0.

Two review passes were done by the same author after that run. No product defect required another archive. The preserved `linux/overlay` and `linux/overlay-2` failures are compile and harness failures. Their logs stay in those directories.

## Pass 1: seals, identities, and statuses

`main` (`147:158`) writes stdout only from `run`'s document. `run` (`203:216`) parses arguments, reads both files, projects both artifacts, renders the diff, and parses that text with `parse_json_value` before returning it. A parse failure there is `field.invalid` with no side.

`project_text` (`280:296`) allowlists the command keys, checks the protocol, then checks wrapper, envelope, and draft shape before `check_seals`. Version and status checks run inside `check_wrapper` and `check_draft_top`. The overlay refusal for a replaced publication status returned `status.refused` while the draft hash was still the compiler's seal. The zeroed `draft_hash` returned `hash.mismatch`. A replaced protocol and a replaced historical commit returned `version.unsupported`.

`check_seals` compares `draft_hash`, `evidence_envelope_hash`, `wrapper_hash`, and both `manifest_hash` fields to `payload_sha256` of the canonical bytes, and requires the wrapper source fields to equal `draft.provenance` and the envelope bindings. The unchanged test requires the summary `draft_hash` to equal the harness digest of the compiler's draft. That digest also equals the compiler's own `wrapper.draft_hash`, so the duplicated canonical writer matches the compiler on the shop document. `assert_sealed` removes `diff_hash`, hashes the remainder, and requires the stdout bytes to be that canonical object. Flag order, pretty JSON, and a reordered command object produced the same stdout.

Identities in the passing tests:

- Source `source/shop_src/shop` is `changed` when `schema_hash` moves, because the source payload includes that hash (`stale_when_equal: false` on the source `MapSpec`).
- Rename of `email` to `contact_email` removes `column/public.customer/email` and adds `column/public.customer/contact_email` on both the physical and generated layers.
- Type `oid` 25 to 1043 changes the physical column (`type_oid`, `type_name`, `type_reason`) and the generated column (`data_type`).
- `foreign_key/public.purchase/purchase_customer_fk` stays one id when the column pair list shrinks from 2 to 1. Physical and generated payloads are `changed`.
- Equal table, column, primary-key, and foreign-key payloads become `stale` when the schema hash moves. The identical catalog emits no facts and `unchanged` 33.
- Unknown versus unknown generated classification and review increments `unchanged` and is omitted, including when the schema hash moves. A one-sided unknown classification is omitted and is not counted. The rename test's `unchanged` 10 is the five surviving columns' generated classification and review pairs. The rename assertion requires no classification fact and no curated fact.
- The decision whose `schema_hash` is the before shop hash is `stale` on the after document. The after `stale_decision` queue item is `stale`. `action` `accept` is copied as a token. `auto_accept` stays false.

Facts sort by status, kind, id, layer. The tests require that order. Alphabetical status order is `added`, `changed`, `removed`, `stale`.

## Pass 2: bounds, refusals, and separation

`parse_args` (`226:250`) accepts `--before` and `--after` once each, in either order. A missing value, a repeated flag, a value that starts with `-`, an empty value, a NUL, or an extra positional is usage exit 2. Usage stderr is the four contract lines.

`read_file` (`253:278`) uses `symlink_metadata` and rejects anything that is not a regular file, then opens and checks the fd metadata again. Length above `DOCUMENT_MAX_BYTES` (1048576) is `json.budget.bytes` before a full read. The read uses `take(DOCUMENT_MAX_BYTES + 1)`. Non-UTF-8 is `json.utf8`. The overlay saw directory and symlink as `io.regular`, a missing path as `io.open`, 1048577 bytes as `json.budget.bytes`, bytes `ff fe` as `json.utf8`, truncated JSON as `json.syntax`, and a repeated `draft` key as `json.duplicate_key`. Stderr is one static line and does not contain the path or the sentinel strings.

`map_scan` (`298:311`) maps the core diagnostic suffix to `json.duplicate_key`, `json.budget.bytes`, `json.budget.depth`, `json.budget.nodes`, or `json.syntax`. The diagnostic message is not printed.

Generated classification goes through `check_record` (`1529:1542`), which allowlists `at`, `by`, and `state`. Curated classification uses `allow_only` on the curated object for `datasets` and `relationships`, then `class_payload` (`1516:1527`). `class_payload` stores `unknown: false`, `value_sha256`, and `state` only when the state matches `^[a-z][a-z0-9_]{0,31}$`. The curated `by` value `secret-owner-name-a213` and the label `do-not-echo-label-a213` are absent from stdout. The comment body and the catalog owner are absent from the compiler draft, the wrapper, the envelope, and the diff.

Physical `generated` and `identity` use `require_marker` (`1735:1739`), so the shop's empty markers pass. An empty string means unset. That is the contract sentence added when the shop artifact was compared with the first parser.

`type_support.converted` true is `field.invalid`. Queue `publishable` true is `status.refused`. Other queue kinds are shape-checked and are not emitted. Unique keys and policy expressions are walked and are not fact identities. A schema move that only those objects caused would still change the source binding and mark equal table, column, primary-key, and foreign-key facts stale.

`push_string` (`1870:1887`) matches the compiler escape: quote, backslash, `\b`, `\f`, `\n`, `\r`, `\t`, and `\u00xx` via `format!("\\u{:04x}", u32::from(ch))`. Digests call `oo_core::api::payload_sha256`.

No `unsafe`, no model call, and no connection API appear in the command. The crate lint forbids `unsafe_code`. Clippy `-D warnings` on the binary and on the test both exited 0.

Reviewed limits, left as written:

- One-sided unknown classification is omitted and is not counted. Unknown versus unknown is counted in `unchanged`.
- Queue `detail` is copied. The compiler's shop detail is the static token `stale_schema_hash`.
- The path is a trusted local file. `symlink_metadata` rejects a symlink that is present at the check. This command does not claim a jail.
- Policy text and unique-key membership are not separate diff identities.
