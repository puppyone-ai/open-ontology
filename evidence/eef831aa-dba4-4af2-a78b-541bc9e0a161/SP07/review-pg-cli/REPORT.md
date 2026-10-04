# a161 review command

`oo-builder review-pg-draft` reads one JSON document from stdin and calls `compile_captured_pg_catalog`. The document carries the captured `oo.source.pgsql/v1` catalog object and, when present, the curated object and the reviews array that function already accepts. Stdout is the same command result as `captured-pg-draft`. Protocol `oo.builder.captured-pg-draft-command/v1`.

Source commit `2283f904f71c9f04738a554f9c67439fa1db9bfc`. Tree `8e093dd327b6ba73035b1a2d0b2ea88c4b25ac42`. Parent `476d21c7a031c53fb8b582fb49c6a5dafd2a0b4c`. Fixed archive `3353380b03a5585a349fddd0038d0a79877c51e1`, tree `5d6767362bedea26d673b43b932a7776f2ecfc25`. The library, crate manifest, root manifest, and lock at that archive are unchanged. The diff from the fixed base is the five paths in `source/blobs.txt`.

## Command

No arguments. Stdin is one document, read up to 1048577 bytes. The core scanner rejects a longer document, a duplicate key, and trailing data. `serde` `deny_unknown_fields` rejects an unknown top-level field. `catalog` is an object. `curated`, when present, is an object. `reviews`, when present, is an array. The command writes those values as key-sorted JSON and calls `compile_captured_pg_catalog`. Absent curated or reviews is none.

Stdout is one JSON object, sorted keys, no trailing newline: `protocol`, `wrapper`, `evidence_envelope`, `draft`. The three compiler values are objects. Exit 0 writes that document. Exit 1 is a read, UTF-8, document, compiler, or stdout failure. Exit 2 is a missing command, an unknown command, or an argument. Stderr is a static diagnostic.

`captured-pg-draft` still calls `compile_captured_pg_catalog(input, None, None)`. `structure-draft` still writes the same draft bytes.

With no arguments the process exits 2 and prints usage for `structure-draft`, `captured-pg-draft`, and `review-pg-draft`. It no longer says the SP00-W01 entry skeleton is unimplemented. That sentence described the entry as unfinished after the draft commands existed. A missing command is still exit 2. The contract records the usage text and the reason.

## What the process checked

The catalogs are the a157 files. `catalog-before.json` SHA-256 `de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9`. `catalog-after.json` SHA-256 `53e2fe7e68197f8698a2a9e2683dc2131426a0faea6bfb277594f9ba9ce52903`. Schema hashes, fact fingerprints, and decision results come from `compile_captured_pg_catalog` on those files. `oracle/confirmed.json` lists the preserved hashes the green run matched.

A catalog-only document reproduces the a157 bare seals: schema `a0efbfc8b5bc7536a121208a1c8e3ff41b16cdda4018b5618b047f40d415271c`, draft `d6980c138fa4939195bd673582938c94e9adca72a9acf710ddcebae407b138ef`, wrapper `e525520f010677050108f4aed9d899ecca28e2c56cbcb91e10d9485d20e3267e`. Key-sorting the catalog does not change those bytes. An empty reviews array does not change them either. The post-alter catalog schema is `c123519256e1c88ab063a41f250ac02483822983d3cf95c364d25983e86fde4d`.

A fresh accept of `column/ledger.party/email` marks that structural item accepted and leaves it unpublished. Email PII and semantic type stay null. The PII and semantic queue items stay pending and not publishable. A fresh accept of `relationship/ledger.invoice/invoice_party_fk` together with a fresh reject of `column/ledger.party/id` removes `invoice_party_fk` from the effective layer, keeps it in the generated layer, and queues `relationship_endpoint_not_effective` with detail `to_column_not_effective`. The curated echo keeps status `rejected` and label `Kept label`. A fresh override applies the label `Correspondence` and leaves curated PII `sensitive` in the echo. Effective PII stays null, and `semantic_field_not_applied` names `pii`.

The same email accept, applied to `catalog-after.json` with the pre-alter schema hash and fact fingerprint, stays a pending `stale_decision` with detail `stale_schema_hash`. The effective label stays `email`. The curated echo keeps `Correspondence` and `sensitive`. The draft hash is the a157 stale seal `8e00ba219f9e99044a739acb3f96eff04f7ce44784a42bec1609e32ee7ae688f`. A matching schema hash with a zero fact fingerprint stays pending as `stale_fact_fingerprint`. `live_catalog_review` stays `pending`. Historical `pg_catalog_integration` stays `not_accepted` at commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`.

Every successful stdout object equals the public API strings for the same catalog, curated value, and reviews. Duplicate keys, unknown top-level fields, a non-object catalog, a non-object curated value, a non-array reviews value, a missing catalog, invalid UTF-8, and a 1048577-byte stdin exit 1 with empty stdout. An argument on `review-pg-draft` exits 2. Stderr does not contain the marker `a161-do-not-echo`.

## Linux 1.94

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Private network `oo-a161-eef831aa-net`, private cargo and target volumes, `HOME=/tmp`, `CapDrop=ALL`, memory 8589934592, privileged false, no port bindings, `pull=never`. No host cargo. No PostgreSQL container.

| Phase | Exit | Result |
|---|---|---|
| pre-edit `cargo test -p oo-builder --lib` | 0 | 28 passed, 0 failed |
| pre-edit `cargo clippy -p oo-builder --lib --test captured_pg -- -D warnings` | 0 | finished, no warning diagnostic |
| pre-edit container | 0 | `OOMKilled=false` |
| attempt 1 `review_pg_cli` | 101 | raw string `br"{}"#` did not compile |
| attempt 1 `captured_pg_cli` | 101 | usage assertion looked for `does not connect` |
| attempt 1 `structure_draft_cli` | 0 | 4 passed, 0 failed |
| attempt 1 clippy | 101 | same raw-string compile error |
| attempt 1 container | 1 | |
| attempt 2 `review_pg_cli` | 101 | review order and a non-hex fact fingerprint |
| attempt 2 `captured_pg_cli` | 0 | 3 passed, 0 failed |
| attempt 2 `structure_draft_cli` | 0 | 4 passed, 0 failed |
| attempt 2 clippy | 0 | finished, no warning diagnostic |
| attempt 2 container | 1 | |
| focused `review_pg_cli` | 0 | 4 passed, 0 failed |
| focused `captured_pg_cli` | 0 | 3 passed, 0 failed |
| focused `structure_draft_cli` | 0 | 4 passed, 0 failed |
| focused `cargo clippy -p oo-builder --bin oo-builder --test review_pg_cli --test captured_pg_cli -- -D warnings` | 0 | finished, no warning diagnostic |
| focused container | 0 | `OOMKilled=false` |

The pre-edit archive had no working-tree overlay. The focused archive was `3353380b` plus the five committed paths and the two catalog fixtures. Logs are `linux/smoke-before/`, `linux/focused/attempt-1/`, `linux/focused/attempt-2/`, and `linux/focused/`.

## Bounds

No library, connector, pipeline, manifest, lock, spec, or ledger file was edited. The development ledger checkbox was left as it stood. Owner authority, authentication, multitenant isolation, persistence, and an audited review workflow remain later work. Cleanup of the a161 builder, volumes, network, and temp directory is `cleanup.txt`.
