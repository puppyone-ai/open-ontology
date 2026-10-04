# a158 captured PostgreSQL draft command

`oo-builder captured-pg-draft` reads one `oo.source.pgsql/v1` catalog from stdin and writes a command result. Protocol `oo.builder.captured-pg-draft-command/v1`. The command calls `compile_captured_pg_catalog(input, None, None)`. `structure-draft` still writes the same draft bytes.

Source commit `3353380b03a5585a349fddd0038d0a79877c51e1`. Tree `5d6767362bedea26d673b43b932a7776f2ecfc25`. Parent `8adb9c3680b0816195fead6a698bc6a704ac751b`. Fixed archive `63466471a6d403e0a2a29c8d1ae064179da65bd8`, tree `1a6c65addb3897fe93159560b8d295949eb2542c`. The library, crate manifest, root manifest, and lock at that archive are the bytes in the source commit.

## Command

No arguments. Stdin is one catalog document, read up to 1048577 bytes. A longer stream is not read further. The parser rejects a document above 1048576 bytes. Stdout is one JSON object, sorted keys, no trailing newline:

| Key | Value |
|---|---|
| `protocol` | `oo.builder.captured-pg-draft-command/v1` |
| `wrapper` | Parsed `CapturedPgDraft.wrapper` |
| `evidence_envelope` | Parsed `CapturedPgDraft.evidence_envelope` |
| `draft` | Parsed `CapturedPgDraft.draft` |

The three objects are the compiler strings embedded as JSON values. They are checked to be objects before any stdout write. Exit 0 writes that document. Exit 1 is a read, UTF-8, catalog, compiler, or stdout failure. Exit 2 is a missing command, an unknown command, or an argument on `captured-pg-draft`. Stderr is a static diagnostic. It does not echo the catalog.

Curated overlays and review decisions stay on `compile_captured_pg_catalog`. This command does not open a connection, call a model, read source credentials, import rows, grant, certify, or publish.

With no arguments the process still prints `process entry is not implemented (SP00-W01 skeleton)` and names `structure-draft` and `captured-pg-draft`.

The captured catalog inside the envelope keeps owner and privilege records. The draft and the wrapper do not copy the owner. Comment bodies are absent from the result. `wrapper.row_import` is `not_proven`. `wrapper.grants` is `not_authorized`. `wrapper.live_catalog_review` stays `pending`. Historical `pg_catalog_integration` stays `not_accepted`.

## Source

Three paths. `Cargo.toml`, `Cargo.lock`, `crates/oo-builder/Cargo.toml`, `captured_pg.rs`, `draft.rs`, `lib.rs`, and `pgsql.rs` keep the `63466471` blobs.

| Path | Blob | Content SHA-256 |
|---|---|---|
| `crates/oo-builder/src/bin/oo-builder.rs` | `327eaa1519380da3b22c724617f3b76f0ff46736` | `d379f42cebfde58973a2b14ae1f6f5fbfb1267ec0ab549ceee102c6a02716f7f` |
| `crates/oo-builder/tests/captured_pg_cli.rs` | `d51f3baae9e98812e4a72cd29c395fc90bd25c93` | `38782d6d3895fb7bf00a3aef15797db1db49d562fb9b823af732501f1df9ba77` |
| `contracts/builder/captured-pg-draft-v1.md` | `d59febd6d8034983ce841f45b1bd8e27d6560cbd` | `e9a37c51cf5fec53dbb5d223086265349ba71faccbef84888ce99b02c9f199c4` |

The content hashes equal `linux/focused/overlay.sha256`, recorded before the focused container ran. The committed bytes are the tested bytes.

## Fixture

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out/catalog-before.json`. SHA-256 `de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9`. 5644 bytes. The a157 review commit is `92829c3b1b80abc10324535051ca5c84faa0d2d8`. The file is the `PgSession::catalog` document for `ledger.party` and `ledger.invoice` in database `oo_a157`, source `ledger_src`, schema hash `a0efbfc8b5bc7536a121208a1c8e3ff41b16cdda4018b5618b047f40d415271c`. The email comment is `{byte_len: 37, sha256: 5b4c8687a0a47b150796d520ec9c5253d23eeb33e80cfd685831a7f29384df81}`. The sentence `a157-review-comment-body-do-not-carry` is absent from the file. The focused snapshot copied this file and checked the hash before the tests.

## Linux 1.94

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8. `RUSTUP_TOOLCHAIN=1.94.0`. Private network `oo-a158-eef831aa-net`, private cargo and target volumes, `HOME=/tmp`, `CapDrop=ALL`, memory 8589934592, privileged false, no port bindings, `pull=never`. No host cargo. `oo-a154-pg` stayed up.

| Phase | Exit | Result |
|---|---|---|
| pre-edit `cargo test -p oo-builder --lib` | 0 | 28 passed, 0 failed |
| pre-edit `cargo clippy -p oo-builder --lib --test captured_pg -- -D warnings` | 0 | finished, no warning diagnostic |
| pre-edit container | 0 | `OOMKilled=false` |
| attempt 1 `captured_pg_cli` | 0 | 3 passed, 0 failed |
| attempt 1 `structure_draft_cli` | 0 | 4 passed, 0 failed |
| attempt 1 clippy | 1 | `cargo-clippy` was not installed in that container |
| attempt 1 container | 1 | tests had already passed |
| focused `captured_pg_cli` | 0 | 3 passed, 0 failed |
| focused `structure_draft_cli` | 0 | 4 passed, 0 failed |
| focused `cargo clippy -p oo-builder --bin oo-builder --test captured_pg_cli -- -D warnings` | 0 | finished, no warning diagnostic |
| focused container | 0 | `OOMKilled=false` |

The pre-edit archive had no working-tree overlay. The focused archive was `63466471` plus the three paths above and the catalog fixture. Logs are `linux/smoke-before/`, `linux/focused/attempt-1/`, and `linux/focused/`.

The CLI test pins the fixture hash, checks stdout bytes against the public API strings, reparses the three objects, recomputes `draft_hash`, `evidence_envelope_hash`, `captured_catalog_hash`, `wrapper_hash`, and `manifest_hash`, and checks the review queue. On the a157 catalog the relationship `invoice_party_fk` is `many_to_one`, `declared_fk`, status `draft`, confidence null, join hint `left`. Email PII and semantic type are null. Queue items `pii/ledger.party/email` / `pii_unknown` and `semantic/ledger.party/email` / `semantic_unknown` are pending. `row_import` is `not_proven`. `grants` is `not_authorized`. The comment sentence is absent from stdout. `a157_owner` is absent from the draft and the wrapper and present on the catalog document inside the envelope. Bad JSON, invalid UTF-8, and a 1048577-byte stdin exit 1 with empty stdout and a short static stderr. No arguments, an unknown command, and an extra argument exit 2.

## Bounds

No PostgreSQL container was started. No library, connector, pipeline, spec, or ledger file was edited. The development ledger checkbox was left as it stood. a154 containers were not removed. Cleanup of the a158 builder, volumes, network, and temp directory is `cleanup.txt`.
