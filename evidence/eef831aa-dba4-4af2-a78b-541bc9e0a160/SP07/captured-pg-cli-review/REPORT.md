# a160 captured PostgreSQL draft command review

Verdict: **APPROVE LIMITED**.

Session `eef831aa-dba4-4af2-a78b-541bc9e0a160`. Reviewed commit `3353380b03a5585a349fddd0038d0a79877c51e1`, tree `5d6767362bedea26d673b43b932a7776f2ecfc25`, parent `8adb9c3680b0816195fead6a698bc6a704ac751b`. Author evidence `2996562ceb332cc0ce505c703e775615fa321e5b`. That manifest checked 72 OK, 0 FAILED.

The approval covers `oo-builder captured-pg-draft`: one `oo.source.pgsql/v1` catalog on stdin, no arguments, calling `compile_captured_pg_catalog(input, None, None)`. Protocol `oo.builder.captured-pg-draft-command/v1`. Stdout is one canonical JSON object whose `wrapper`, `evidence_envelope`, and `draft` values are the three strings on `CapturedPgDraft`. The approved API remains `oo_builder::compile_captured_pg_catalog` on `63466471a6d403e0a2a29c8d1ae064179da65bd8`.

This approval leaves row import, grants, publication, certification, live catalog review, and SP07 open. `wrapper.live_catalog_review` stays `pending`. Historical `pg_catalog_integration` stays `not_accepted`. With no arguments the process still prints the SP00-W01 skeleton line. That line is a documentation residual, recorded below with its source line.

## Source

`git diff --name-status 63466471 3353380b` for the builder, lock, manifests, and this contract:

| Path | Status |
|---|---|
| `contracts/builder/captured-pg-draft-v1.md` | modified, blob `d59febd6d8034983ce841f45b1bd8e27d6560cbd` |
| `crates/oo-builder/src/bin/oo-builder.rs` | modified, blob `327eaa1519380da3b22c724617f3b76f0ff46736` |
| `crates/oo-builder/tests/captured_pg_cli.rs` | added, blob `d51f3baae9e98812e4a72cd29c395fc90bd25c93` |

`git diff --exit-code` for `Cargo.toml`, `Cargo.lock`, `crates/oo-builder/Cargo.toml`, `captured_pg.rs`, `draft.rs`, `lib.rs`, `pgsql.rs`, `crates/oo-core`, and `crates/oo-connectors/Cargo.toml` exited 0. Those blobs match the a157 accepted commit:

| Path | Blob |
|---|---|
| `Cargo.lock` | `ef30377cade0ad07ba4192125f434b01452a5622` |
| `Cargo.toml` | `c78ee5d0575a11cd6078cf0055cdd2c673edf583` |
| `crates/oo-builder/Cargo.toml` | `d3cf3f73ba7884e34fd503ccd13a7d010161b78a` |
| `crates/oo-builder/src/captured_pg.rs` | `373bcad7968d88d9be331e4622b63d1ca60b1449` |
| `crates/oo-builder/src/draft.rs` | `9505a8fd63e5c2517622c695d9ffeda4a639b562` |
| `crates/oo-builder/src/lib.rs` | `5849b4884e08ce2bdb8a553d56733748ef1715cc` |
| `crates/oo-builder/src/pgsql.rs` | `c7f399aa919c18858ae9d29da22deabd01ebe115` |

The command reads stdin with `take(DOCUMENT_MAX_BYTES + 1)` at `oo-builder.rs:93-97`. `DOCUMENT_MAX_BYTES` is 1048576. It calls `compile_captured_pg_catalog(&read_stdin()?, None, None)` at `oo-builder.rs:86`. `command_document` (`oo-builder.rs:105-118`) parses the wrapper, the envelope, the draft, and the spliced result before `main` writes stdout (`oo-builder.rs:57-66`). Failures print `err.code` and `err.message` (`oo-builder.rs:69`). The message is a static string. The path is not printed.

## Fixture

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out/catalog-before.json`. SHA-256 `de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9`. 5644 bytes. a157 review commit `92829c3b1b80abc10324535051ca5c84faa0d2d8`. Protocol `oo.source.pgsql/v1`, source `ledger_src`, database `oo_a157`, schema hash `a0efbfc8b5bc7536a121208a1c8e3ff41b16cdda4018b5618b047f40d415271c`. The email comment is `{byte_len: 37, sha256: 5b4c8687a0a47b150796d520ec9c5253d23eeb33e80cfd685831a7f29384df81}` and has no `body` field. The sentence `a157-review-comment-body-do-not-carry` is absent from the file.

## Linux 1.94

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8. `cargo 1.94.0`. `RUSTUP_TOOLCHAIN=1.94.0`. Container `oo-a160-builder`, private network `oo-a160-eef831aa-net`, private cargo and target volumes, `HOME=/tmp`, `CapDrop=ALL`, memory 8589934592, privileged false, no port bindings, `pull=never`. The snapshot was `git archive` of `3353380b` plus the review oracle `tests/a160_oracle.rs` and the fixture. No PostgreSQL container was started.

| Phase | Exit | Result |
|---|---|---|
| `cargo test --locked -p oo-builder --lib` | 0 | 28 passed, 0 failed |
| `cargo test --locked -p oo-builder --test a160_oracle` | 0 | 1 passed, 0 failed |
| `cargo test --locked -p oo-builder --test structure_draft_cli` | 0 | 4 passed, 0 failed |
| `cargo test --locked -p oo-builder --test captured_pg_cli` | 0 | 3 passed, 0 failed |
| `cargo clippy --locked -p oo-builder --lib --bin oo-builder --test captured_pg_cli --test a160_oracle -- -D warnings` | 0 | finished, 0 warning lines |
| container | 0 | `OOMKilled=false` |

Attempt 1 is preserved under `linux/smoke/attempt-1/`. The oracle crate did not compile: `serde_json::json!` reached the recursion limit, so `a160_oracle` and Clippy exited 101. Lib, `structure_draft_cli`, and `captured_pg_cli` had already exited 0. The summary writer was replaced with `serde_json::to_value`. Product sources were not edited. The green run is `linux/smoke/`.

## Process oracle

The oracle spawns `CARGO_BIN_EXE_oo-builder`. Expected objects come from `compile_captured_pg_catalog` on the fixture text. Raw files are `linux/smoke/oracle-out/`.

| Check | Observed |
|---|---|
| Success exit | 0, stderr empty |
| Stdout | 23329 bytes, SHA-256 `45f35cf6208e59c70d4556051cf2ac1d2096c43eafd950396414180f4e0ceb08`, 0 newline bytes, last byte `0x7d` |
| Public API | stdout bytes equal the spliced `draft`, `evidence_envelope`, and `wrapper` strings |
| Canonical | re-serializing the parsed object with sorted keys reproduces those bytes |
| Keys | `draft`, `evidence_envelope`, `protocol`, `wrapper` |
| Protocol | `oo.builder.captured-pg-draft-command/v1` |
| Round trip | `compile_structure_draft(envelope)` equals the draft string |
| `draft_hash` | `d6980c138fa4939195bd673582938c94e9adca72a9acf710ddcebae407b138ef` |
| `evidence_envelope_hash` | `415da260e8e229f9b0a9b07e03a6a106d2cda4cf8ebb4b38c6444b627fea8eae` |
| `captured_catalog_hash` | `5c17683d74bcc7916954dfb23d22004e5ccbd4058f54bb1d3699ca679a8b90b0` |
| `wrapper_hash` | `e525520f010677050108f4aed9d899ecca28e2c56cbcb91e10d9485d20e3267e` |
| `manifest_hash` | `25b3533d37badc024ea46e393bfc33cc6711616590b4f0888c1202197eb04f03` |
| Relationship | `invoice_party_fk`, `many_to_one`, `declared_fk`, status `draft`, confidence null, join hint `left` |
| Email | PII null, semantic type null |
| Queue | `pii/ledger.party/email` / `pii_unknown` and `semantic/ledger.party/email` / `semantic_unknown`, both `pending`, `publishable` false |
| Wrapper | `row_import=not_proven`, `grants=not_authorized`, `live_catalog_review=pending` |
| Historical | `pg_catalog_integration=not_accepted` on the wrapper and the draft; commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` occurs twice in stdout |
| Owner | both envelope tables record `a157_owner`; the string occurs 18 times in stdout; it is absent from the draft object and the wrapper object |
| Privileges | 9 records on each envelope table; `grantable` is absent from the draft and the wrapper |
| Comment | envelope email comment is `{byte_len: 37, sha256: 5b4c8687...}` with no `body`; the review sentence is absent from stdout |

`draft_hash` and `wrapper_hash` are the seals recorded for this fixture by the a157 review. The other three seals were recomputed here from the command objects: SHA-256 of the draft bytes, of the envelope bytes, of the canonical catalog value, of the wrapper with `wrapper_hash` removed, and of the draft with `provenance.manifest_hash` removed.

Failure cases wrote 0 stdout bytes:

| Case | Exit | Stderr |
|---|---|---|
| No arguments, stdin left open with a marker | 2 in 20 ms | skeleton line, both command names, marker absent |
| Unknown command `publish` | 2 | `unknown command` |
| `captured-pg-draft --out` | 2 | `takes no arguments`, marker absent |
| Malformed JSON containing the marker | 1 | `json.syntax`, 109 bytes, marker absent |
| Bytes `ff fe` | 1 | `stdin is not UTF-8` |
| 1048576 spaces, then EOF | 1 | `json.syntax`, not `json.budget.bytes` |
| 1048577 spaces, stdin left open | 1 in 22 ms | `json.budget.bytes`, `document exceeds the byte budget`, 113 bytes |
| Fixture plus a planted comment `body` | 1 | `field.unknown`, planted sentence absent |

`structure-draft` on `examples/ontology-draft/goldens/customer.input.json` exited 0. Stdout SHA-256 `486a22960ec2c759455bf26c1d970e44dc486a5e7225a38ec4d6c08b876cb106` equals `customer.draft.json`. A duplicate `protocol` key exited 1 with empty stdout and `json.duplicate_key`. The existing `structure_draft_cli` tests passed, including the customer golden and the closure fixture.

## Residual

`crates/oo-builder/src/bin/oo-builder.rs:13`:

```rust
eprintln!("oo-builder: process entry is not implemented (SP00-W01 skeleton)");
```

With no arguments the first stderr line is `oo-builder: process entry is not implemented (SP00-W01 skeleton)`. The following lines name `structure-draft` and `captured-pg-draft`. Exit is 2 and stdout is empty. The command section of `contracts/builder/captured-pg-draft-v1.md` states that this skeleton sentence remains. The named commands run. This review does not treat the sentence as a functional failure of `captured-pg-draft`.

## Bounds

No library, connector, pipeline, spec, or ledger file was edited. The catalog compiler matrix and the Unicode campaign were not rerun. The lib test above is the crate's existing unit smoke. No output-fault, FIFO, or security campaign was added. a159 containers were not removed. Cleanup of the a160 builder, volumes, network, and temp directory is `cleanup.txt`.
