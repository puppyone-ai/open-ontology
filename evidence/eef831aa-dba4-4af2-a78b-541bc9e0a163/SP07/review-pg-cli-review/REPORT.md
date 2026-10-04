# a163 review-pg-draft command review

Verdict: **APPROVE LIMITED**.

Session `eef831aa-dba4-4af2-a78b-541bc9e0a163`. Reviewed commit `2283f904f71c9f04738a554f9c67439fa1db9bfc`, tree `8e093dd327b6ba73035b1a2d0b2ea88c4b25ac42`, parent `476d21c7a031c53fb8b582fb49c6a5dafd2a0b4c`. Author evidence `aa9a083d9630bddf51d5f5fb2004b3be64a6cf6a`. That manifest checked 93 OK, 0 FAILED.

The approval covers `oo-builder review-pg-draft` on that commit: no arguments, one JSON document on stdin, calling the existing `compile_captured_pg_catalog`. The document is a required `catalog` object, an optional `curated` object, and an optional `reviews` array. Stdout is protocol `oo.builder.captured-pg-draft-command/v1` with `wrapper`, `evidence_envelope`, and `draft` as JSON objects. Those objects equal the public API strings. The approved compiler remains `oo_builder::compile_captured_pg_catalog` on `63466471a6d403e0a2a29c8d1ae064179da65bd8`, reviewed by a157. The captured command remains the a160 approval of `3353380b03a5585a349fddd0038d0a79877c51e1`.

This approval leaves row import, grants, publication, certification, live catalog review, owner authority, authentication, persistence, and SP07 open. `wrapper.live_catalog_review` stays `pending`. Historical `pg_catalog_integration` stays `not_accepted`. A fresh accept, reject, or override binds one structural item. It does not approve semantics or publication.

## Source

`git diff --name-status 476d21c7 2283f904` is five paths. Against fixed base `3353380b`, the builder library, crate manifest, root manifest, and lock are unchanged (`git diff --exit-code` exit 0).

| Path | Blob | Content SHA-256 |
|---|---|---|
| `contracts/builder/captured-pg-draft-v1.md` | `0015ec4083bbbaf3287ca2baa1b887546b409955` | `abfe41ef9a614c81d6bbaeb1b4da7abde931e825b0a17b7ccf1a8555f2f65c41` |
| `crates/oo-builder/src/bin/oo-builder.rs` | `c2f1fe4a4ba11eb36263b4eeb20e17fd9a93c09d` | `a579c71ca56076f74fadb9b88a125cd452967dc43c7474af76429899d137d274` |
| `crates/oo-builder/tests/captured_pg_cli.rs` | `9d481b1f0dd93cd7db364c32d41fadece5680fe7` | `ff7d9e4ce25cdde61c897e53cb4755f85b89fe93c4b8571fe0f4230807ab5b84` |
| `crates/oo-builder/tests/review_pg_cli.rs` | `74fbe07922ab21bba4e8ab839e7b92802539acf7` | `8550eaf0e1e946a850c97c71036ee6be08835d1de3a029d5eeb62c19a9737dd0` |
| `crates/oo-builder/tests/structure_draft_cli.rs` | `1fe5e06f1fb078749cca306e8f28a9822ada957d` | `e174e3d84df017327de7a8f4cd13077504730e5ce19cb514315056f61e476dd8` |

Unchanged library blobs, same as a157 and a160:

| Path | Blob |
|---|---|
| `Cargo.lock` | `ef30377cade0ad07ba4192125f434b01452a5622` |
| `Cargo.toml` | `c78ee5d0575a11cd6078cf0055cdd2c673edf583` |
| `crates/oo-builder/Cargo.toml` | `d3cf3f73ba7884e34fd503ccd13a7d010161b78a` |
| `crates/oo-builder/src/captured_pg.rs` | `373bcad7968d88d9be331e4622b63d1ca60b1449` |
| `crates/oo-builder/src/draft.rs` | `9505a8fd63e5c2517622c695d9ffeda4a639b562` |
| `crates/oo-builder/src/lib.rs` | `5849b4884e08ce2bdb8a553d56733748ef1715cc` |
| `crates/oo-builder/src/pgsql.rs` | `c7f399aa919c18858ae9d29da22deabd01ebe115` |

`review-pg-draft` is `oo-builder.rs:60-73`. It calls `read_and_compile_review` at `oo-builder.rs:123-140`. That function scans with `oo_core::parse_json_value`, deserializes `ReviewPgDocument` (`oo-builder.rs:179-187`, `deny_unknown_fields`), rejects a non-object catalog, writes key-sorted JSON, and calls `compile_captured_pg_catalog`. `read_stdin` takes `DOCUMENT_MAX_BYTES + 1` at `oo-builder.rs:219-230`. `command_document` parses the three compiler strings and the spliced result before stdout is written (`oo-builder.rs:235-248`). Failures print `err.code` and `err.message` (`oo-builder.rs:70`). The message is a static string. The document is not printed.

`captured-pg-draft` still calls `compile_captured_pg_catalog(&read_stdin()?, None, None)` at `oo-builder.rs:118-120`. `structure-draft` still calls `compile_structure_draft`.

The a161 write card named `oo-builder.rs`, `review_pg_cli.rs`, `captured_pg_cli.rs`, and `contracts/builder/captured-pg-draft-v1.md`. It did not name `tests/structure_draft_cli.rs`. That file's only change is the no-argument assertion at lines 21-23. The coordinator accepts that narrow obsolete-usage assertion for this fixed review. The diff is `source/structure-draft-cli.diff`. The structure-draft command bytes are unchanged, below.

## Fixture

Both catalogs are the a157 files from commit `2283f904`, not a working-tree copy.

| File | SHA-256 | Bytes |
|---|---|---|
| `catalog-before.json` | `de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9` | 5644 |
| `catalog-after.json` | `53e2fe7e68197f8698a2a9e2683dc2131426a0faea6bfb277594f9ba9ce52903` | 6149 |

The sentence `a157-review-comment-body-do-not-carry` is absent from the bare command stdout.

## Linux 1.94

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8. `cargo 1.94.0`. `RUSTUP_TOOLCHAIN=1.94.0`. Container `oo-a163-builder`, private network `oo-a163-eef831aa-net`, private cargo and target volumes, `HOME=/tmp`, `CapDrop=ALL`, memory 8589934592, privileged false, no port bindings, `pull=never`. `HostConfig.Tmpfs` is null. The snapshot was `git archive` of `2283f904` plus the review oracle `tests/a163_oracle.rs`. No PostgreSQL container was started. No product source was edited.

Capacity immediately before `docker run` was `running_rust_image_or_label=0`, `own_rust_builder_running=0`, `host_cargo_rustc_rustdoc_rustup=0`. `containers-before.txt` is empty. This builder was the only owned Rust builder.

| Phase | Exit | Result |
|---|---|---|
| `cargo fetch --locked` | 0 | download completed, then offline |
| `cargo test --locked --offline -p oo-builder --lib` | 0 | 28 passed, 0 failed |
| `cargo test --locked --offline -p oo-builder --test a163_oracle` | 0 | 1 passed, 0 failed, 0.36s |
| `cargo test --locked --offline -p oo-builder --test structure_draft_cli` | 0 | 4 passed, 0 failed |
| `cargo test --locked --offline -p oo-builder --test captured_pg_cli` | 0 | 3 passed, 0 failed |
| `cargo test --locked --offline -p oo-builder --test review_pg_cli` | 0 | 4 passed, 0 failed |
| `cargo clippy --locked --offline -p oo-builder --lib --bin oo-builder --test a163_oracle --test review_pg_cli --test captured_pg_cli --test structure_draft_cli -- -D warnings` | 0 | finished, 0 warning lines |
| container | 0 | `OOMKilled=false` |

There was no failed builder attempt. Fetch used the network. The test and Clippy commands were `--offline`. The lib smoke is the crate's existing 28 unit tests. This review did not add a Unicode, FIFO, security, or closure matrix.

## Process oracle

The oracle spawns `CARGO_BIN_EXE_oo-builder`. Expected objects come from `compile_captured_pg_catalog` on the fixture values. Raw files are `linux/smoke/oracle-out/`. `oracle-result.json` records `failure_count` 0.

Catalog-only `review-pg-draft` and `captured-pg-draft` wrote the same 23329 stdout bytes. SHA-256 `45f35cf6208e59c70d4556051cf2ac1d2096c43eafd950396414180f4e0ceb08`. That is the stdout digest recorded for this fixture by the a160 review. Zero newline bytes. The last byte is `0x7d`. An empty reviews array produced the same bytes. Key-sorting the catalog did not change the API seals.

| Check | Observed |
|---|---|
| Protocol | `oo.builder.captured-pg-draft-command/v1` |
| Keys | `draft`, `evidence_envelope`, `protocol`, `wrapper` |
| `schema_hash` | `a0efbfc8b5bc7536a121208a1c8e3ff41b16cdda4018b5618b047f40d415271c` |
| `draft_hash` | `d6980c138fa4939195bd673582938c94e9adca72a9acf710ddcebae407b138ef` |
| `wrapper_hash` | `e525520f010677050108f4aed9d899ecca28e2c56cbcb91e10d9485d20e3267e` |
| `captured_catalog_hash` | `5c17683d74bcc7916954dfb23d22004e5ccbd4058f54bb1d3699ca679a8b90b0` |
| `evidence_envelope_hash` | `415da260e8e229f9b0a9b07e03a6a106d2cda4cf8ebb4b38c6444b627fea8eae` |
| `manifest_hash` | `25b3533d37badc024ea46e393bfc33cc6711616590b4f0888c1202197eb04f03` |
| Round trip | `compile_structure_draft(envelope)` equals the draft string |
| Wrapper | `row_import=not_proven`, `grants=not_authorized`, `publication=unavailable`, `certification=none`, `llm=not_used`, `source_connection=not_used`, `live_catalog_review=pending` |
| Historical | `pg_catalog_integration=not_accepted`, commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` |
| Owner | `a157_owner` occurs 18 times in stdout, all inside the envelope; 0 in the draft object and 0 in the wrapper object |

`draft_hash` and `wrapper_hash` are the a157 seals for this fixture. The envelope, catalog, and manifest digests match the values recomputed by a160 for the same fixture. The oracle recomputed SHA-256 of the draft bytes, of the envelope bytes, and of the wrapper with `wrapper_hash` removed. Each matched the wrapper field.

Fresh decisions use the queue fingerprints from the bare draft: email `d86ee784370f0f33f8649793e73a9b33a1f61c439dfcaf7ced42e3318bfc2ca3`, id `58e598b6c782846518bbafbbe710e49e5c76fe02cad19b5773843b2766330c0e`, relationship `ef8eb7c5bd99d5cad287b91a78c35c93932ab50c7be80516de0ef9abaf9d2f3e`. Each process stdout equaled the public API for the same catalog, curated value, and reviews.

| Case | Draft hash | What the stdout showed |
|---|---|---|
| Fresh accept of `column/ledger.party/email` | `0258d01cd91ce5de8d6cce6ad0b40ef71220658f61d9f98c55dae3fbe7dd0cfc` | structural item `accepted`, `publishable` false; effective PII and semantic type null; `pii_unknown` and `semantic_unknown` pending and not publishable; `live_catalog_review` still `pending` |
| Fresh accept of `invoice_party_fk` plus fresh reject of `column/ledger.party/id` | `ff1999302a74fa0f6c140de4042a4665b14ffd5d8f45d18af6e84f6874dc87f1` | generated `id` and `invoice_party_fk` remain; effective copies are gone; curated id is `rejected` with label `Kept label`; queue `relationship_endpoint_not_effective` detail `to_column_not_effective`, pending, not publishable |
| Fresh override of email | `73bd1527da5add4e43d8c89d340ea135a345e90369e49780715d76bd635aa834` | effective label `Correspondence`; effective PII absent; curated PII `sensitive` and label `Correspondence`; `semantic_field_not_applied` detail `pii` |
| Same email accept on `catalog-after.json` with the pre-alter schema hash and fingerprint | `8e00ba219f9e99044a739acb3f96eff04f7ce44784a42bec1609e32ee7ae688f` | pending `stale_decision` detail `stale_schema_hash`; effective label stays `email`; curated echo keeps `Correspondence` and `sensitive`; schema hash `c123519256e1c88ab063a41f250ac02483822983d3cf95c364d25983e86fde4d`; captured catalog hash `6f4fca46a3144f9e5c0073fec0945af70717cde52a18c257ec74b0d60109c92c` |
| Matching schema hash, fact fingerprint 64 zero digits | `c5ecebcf480337dcdb1604664420807ce57b8fde2213047528731dab74513f5e` | pending `stale_decision` detail `stale_fact_fingerprint`; structural state stays `pending` |

The stale-schema draft hash is the a157 stale seal. The post-alter schema hash and captured-catalog hash are the seals the author recorded and this process recomputed through the public API.

Failure cases wrote 0 stdout bytes. Stderr did not contain `a163-do-not-echo`.

| Case | Exit | Stderr |
|---|---|---|
| No arguments, stdin left open with the marker | 2 in 22 ms | the eight usage lines, 664 bytes; skeleton sentence absent |
| Unknown command `publish` | 2 | `unknown command`, 28 bytes |
| `review-pg-draft` plus the marker as an argument | 2 | `takes no arguments`, 86 bytes, marker absent |
| `captured-pg-draft --out` | 2 | captured-pg-draft takes no arguments |
| `structure-draft --publish` | 2 | structure-draft takes no arguments |
| Duplicate `catalog` key whose value contains the marker | 1 | `json.duplicate_key`, 105 bytes, marker absent |
| Unknown top-level field named with the marker | 1 | `field.unknown`, 111 bytes, marker absent |
| `catalog` array, `catalog` null | 1 | `catalog is not an object` |
| `curated` string containing the marker, `curated` null, `reviews` object, `reviews` null | 1 | `schema.invalid`, marker absent |
| Missing `catalog` | 1 | `field.missing` |
| Bytes `ff fe` | 1 | `stdin is not UTF-8` |
| Malformed JSON and trailing data, each containing the marker | 1 | `json.syntax`, marker absent |
| Unknown review item | 1 | `review.unknown_item`, static message, 115 bytes |
| 1048576 spaces, then EOF | 1 | `json.syntax`, not `json.budget.bytes` |
| 1048577 spaces plus the marker, stdin left open | 1 in 21 ms | `json.budget.bytes`, `document exceeds the byte budget`, 111 bytes, marker absent |

`structure-draft` on `examples/ontology-draft/goldens/customer.input.json` exited 0. Stdout SHA-256 `486a22960ec2c759455bf26c1d970e44dc486a5e7225a38ec4d6c08b876cb106` equals `customer.draft.json` and the digest recorded by a160. A duplicate `protocol` key exited 1 with empty stdout and `json.duplicate_key`.

## Bounds

No library, connector, pipeline, manifest, lock, spec, or ledger file was edited. The catalog compiler matrix and the Unicode campaign were not rerun. No output-fault, FIFO, or security campaign was added. a162 containers were not removed. Cleanup of the a163 builder, volumes, network, and temp directory is `cleanup.txt`.
