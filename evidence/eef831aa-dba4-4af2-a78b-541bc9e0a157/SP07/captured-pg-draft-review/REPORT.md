# a157 captured PostgreSQL draft review

Verdict: **APPROVE LIMITED**.

Session `eef831aa-dba4-4af2-a78b-541bc9e0a157`. Reviewed commit `63466471a6d403e0a2a29c8d1ae064179da65bd8`, tree `1a6c65addb3897fe93159560b8d295949eb2542c`, parent `377707e88b13b2b77baa6f619c2c1b364f0dab28`. Author evidence commit `7539fe8f`. Fixed base cited by that delivery is `e6bd4c7aa8c3a43cc0516a21e325765c5eecb2f6`. This review archived `63466471` itself.

The approval covers `oo_builder::compile_captured_pg_catalog`: one `oo.source.pgsql/v1` catalog document, optional curated overlay, and optional reviews, producing an evidence envelope and a structure draft. Protocol `oo.builder.captured-pg-draft/v1`. Compiler `oo-builder/captured-pg-draft/v1`. The builder does not open a connection and does not depend on `oo-connectors`.

This approval leaves row import, grants, publication, certification, and SP07 open. `live_catalog_review` stays `pending`. Historical `pg_catalog_integration` stays `not_accepted`.

## Source

Six paths, `+1054/-8`. No `Cargo.toml`, `Cargo.lock`, connector, pipeline, spec, or ledger path is in the commit.

| Path | Blob |
|---|---|
| `contracts/builder/captured-pg-draft-v1.md` | `916fe4c053b91d072fbc16fe000e7b41206d940c` |
| `crates/oo-builder/src/captured_pg.rs` | `373bcad7968d88d9be331e4622b63d1ca60b1449` |
| `crates/oo-builder/src/draft.rs` | `9505a8fd63e5c2517622c695d9ffeda4a639b562` |
| `crates/oo-builder/src/lib.rs` | `5849b4884e08ce2bdb8a553d56733748ef1715cc` |
| `crates/oo-builder/src/pgsql.rs` | `c7f399aa919c18858ae9d29da22deabd01ebe115` |
| `crates/oo-builder/tests/captured_pg.rs` | `e2218b0dc82ecaec39f67dd00a580cd5468e2f7c` |

`Cargo.lock` blob `ef30377cade0ad07ba4192125f434b01452a5622` is the same at the parent, at `63466471`, and at the workspace HEAD read during this review. Root `Cargo.toml` `c78ee5d0575a11cd6078cf0055cdd2c673edf583`. `crates/oo-builder/Cargo.toml` `d3cf3f73ba7884e34fd503ccd13a7d010161b78a`. `crates/oo-connectors/Cargo.toml` `e8e3254da87a29ce90c485e8ad6566dbd4654f43`.

Author `MANIFEST.sha256` under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a156/SP07/captured-pg-draft/` checked with `shasum -a 256 -c`: exit 0, 224 OK, 0 FAILED. Recorded in `source/author-manifest-check.txt`. That tree was not rewritten.

## What the diff does

`source/hash-helper.diff` is `git diff 377707e8..63466471` for `lib.rs`, `draft.rs`, and `pgsql.rs`.

`config_hash` (`draft.rs:483`) and `type_hash` (`draft.rs:488`) stay the existing functions and become `pub(crate)`. `evidence_hash` stays private at `draft.rs:515` and is called from `catalog_bindings` (`draft.rs:2438`) and `evidence_envelope_bytes` (`draft.rs:2458`). `captured_pg.rs` calls those helpers. It does not contain a second hash implementation. `schema_hash` is the digest `pgsql::load` recomputes after normalization. A mismatched captured `schema_hash` is `hash.schema`.

`selection_from_catalog` (`draft.rs:2408`) builds the column-name set and the key set `primary_key` plus unique and foreign-key constraint names, then sorts them. `check_selection` (`draft.rs:406-437`) compares those sets, so order is not significant and a duplicate fails `duplicate.selection`. `compile_structure_draft` runs that check on the envelope this wrapper builds. The two constructions can drift, and a drift fails closed as `selection.mismatch`.

`accept_captured_catalog` (`pgsql.rs:1361`) sorts tables by `(schema, name)`, columns by `attnum`, unique keys, foreign keys, and policies by name, and privileges by grantee, privilege, and grant option. Primary-key member order, foreign-key pair order, enum labels, and domain checks stay as captured. `PG_CONTRACT_COMMIT` is the same `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`, now `pub(crate)`.

`provenance_value` (`draft.rs:2381-2382`) still writes `pg_catalog_integration = not_accepted`. `require_historical` (`captured_pg.rs:177-207`) refuses a draft that does not echo that contract, the hashes, the source id, the database, and `captured_at`. `seal_wrapper` (`captured_pg.rs:135-175`) hardcodes `live_catalog_review=pending`, `row_import=not_proven`, `grants=not_authorized`, `publication=unavailable`, `certification=none`, `llm=not_used`, `source_connection=not_used`, and `connector_protocol=oo.source.pgsql/v1`. `type_support.converted: true` is rejected at `pgsql.rs:986-991` with `type.converted` before a draft exists. `canonical_sha256` calls `oo_core::api::payload_sha256` and cannot fail open.

`apply_reviews` (`draft.rs:1232-1256`) and `relationship_gap` (`draft.rs:2081-2101`) are outside the diff hunks. A schema-hash mismatch still queues `stale_decision` / `stale_schema_hash` and does not apply the decision. A missing effective endpoint column still yields `to_column_not_effective`. `nfc.rs` is not in the commit.

a086 approved only findings 3 and 4 on `eebc0ea3b835655fdb3ab088e6f7351d0d316a43`, 118 cases, 0 failure. a094 approved only findings 1, 2, and 5 on `3ed558a9b62ab3f290ce9674b9afc10e693a5730`. This review cites those limited facts and does not rerun the closure matrix or the Unicode campaign. The library smoke below includes the unit tests that already live in this crate.

## Linux 1.94

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8. `RUSTUP_TOOLCHAIN=1.94.0`. Container `oo-a157-builder-smoke`, private network `oo-a157-eef831aa-net`, `CapDrop=ALL`, `CapAdd` empty, memory 8589934592, privileged false, no port bindings, `pull=never`. Archive of `63466471` included `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `.cargo`, `examples/ontology-draft`, and `contracts/builder`.

| Phase | Exit | Result |
|---|---|---|
| `cargo test -p oo-builder --lib` | 0 | 28 passed, 0 failed |
| `cargo clippy -p oo-builder --lib --test captured_pg -- -D warnings` | 0 | finished, no warning diagnostic |
| `cargo test -p oo-builder --test structure_draft_cli` | 0 | 4 passed, 0 failed |
| `cargo test -p oo-builder --test captured_pg` | 0 | 6 passed, 0 failed |
| container | 0 | `OOMKilled=false` |

Clippy for 1.94 was installed inside that container (`rustup-clippy.exit` 0). This run did not rustfmt `draft.rs`. Raw logs are `linux/smoke/`.

## Live PostgreSQL 17.10

Image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `os=linux`, `arch=arm64`. Server `17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a157-pg`, alias `oo-a157-pg`, database `oo_a157`. `up.exit` 0, `init.exit` 0. Privileged false. `CapDrop=ALL`. Added caps `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. Published ports `{}` (`5432/tcp` is null). Certificate SAN `DNS:oo-a157-pg`. `log_statement=all` after init, then the log was truncated. Reader `oo_a157_reader` is `SELECT` only. Tables are owned by `a157_owner`.

The fixture is schema `ledger`, tables `party` (`tenant_id`, `id`, `email`) and `invoice` (`tenant_id`, `id`, `party_id`), composite primary key `(tenant_id, id)` on both, foreign key `invoice_party_fk` `(tenant_id, party_id)` to `party` `(tenant_id, id)`. This is a different database from the author shop fixture.

The probe called `PgSourceConfig::try_new`, `PgConnector::new`, `connect`, and `PgSession::catalog`, then `compile_captured_pg_catalog`. Passwords and the CA stayed in mode `600` files under `/tmp`. They were not process arguments.

| Check | Observed |
|---|---|
| Catalog | `oo.source.pgsql/v1`, `type_conversion=unsupported`, source `ledger_src`, database `oo_a157` |
| Party columns | `tenant_id`, `id`, `email` in `attnum` order |
| Invoice columns | `tenant_id`, `id`, `party_id` |
| Primary keys | `tenant_id`, `id` on both |
| Relationship | `invoice_party_fk`, `many_to_one`, `declared_fk`, status `draft`, confidence null, join hint `left`, from `ledger.invoice` `(tenant_id, party_id)` to `ledger.party` `(tenant_id, id)` |
| Email | `text`, `known_builtin_not_converted`, `converted=false`, PII null, semantic type null, queue `pii_unknown` and `semantic_unknown` pending |
| Collation | email oid `100` name `default`; `tenant_id` oid `0` name null. The probe required a nonzero named text collation and did not embed oid 100 |
| Comment | byte length 37, sha256 `5b4c8687a0a47b150796d520ec9c5253d23eeb33e80cfd685831a7f29384df81`, no `body` field |
| Selection | party columns and `primary_key`; invoice columns, `primary_key`, and `invoice_party_fk` |
| Empty reviews | `None` and `[]` produced the same envelope, draft, and wrapper |
| Round trip | `compile_structure_draft(envelope)` equals the sealed draft bytes |
| Reorder | reversed tables, columns, and privileges kept the sealed bytes and hashes |
| Swapped PK members | `hash.schema` |
| `converted: true` | `type.converted` |
| Comment `body` field | `field.unknown` |
| Duplicate `protocol` key | `json.duplicate_key` |
| Fresh accept of `column/ledger.party/email` | structural item accepted, still not publishable, PII and semantic type stay null, wrapper stays pending |
| Fresh reject of `column/ledger.party/id` | `id` stays generated, leaves effective, curated echo keeps status `rejected` and label `Kept label`, generated relationship stays, effective relationship is removed, queue `relationship_endpoint_not_effective` detail `to_column_not_effective`, pending, not publishable |
| Strict core | generated and effective datasets and relationships deserialized as `DatasetFile` and `RelationshipsFile` and `validate_dataset` / `validate_relationships` returned no diagnostics |
| Schema hashes | `a0efbfc8b5bc7536a121208a1c8e3ff41b16cdda4018b5618b047f40d415271c` then, after `ALTER TABLE ledger.party ADD COLUMN note text`, `c123519256e1c88ab063a41f250ac02483822983d3cf95c364d25983e86fde4d` |
| Stale decision | `column/ledger.party/email` `stale_decision` detail `stale_schema_hash`, pending, not publishable. Effective label stays `email`, PII null. Curated echo keeps label `Correspondence` and PII `sensitive`. `curated_unbound` is pending. `semantic_field_not_applied` on `pii/ledger.party/email` has detail `pii` |
| Seals | `draft_hash` `d6980c138fa4939195bd673582938c94e9adca72a9acf710ddcebae407b138ef`. `wrapper_hash` `e525520f010677050108f4aed9d899ecca28e2c56cbcb91e10d9485d20e3267e`. Independent sorted-key SHA-256 matched `draft_hash`, `evidence_envelope_hash`, `captured_catalog_hash`, `manifest_hash`, and `wrapper_hash` |
| Owner | `a157_owner` is on the catalog document and absent from both drafts and both wrappers |

`capture.exit` 0, `stale.exit` 0, `alter.exit` 0, probe container exit 0. Artifacts: `probe/out/capture-oracle.json`, `stale-oracle.json`, `catalog-before.json`, `catalog-after.json`, `draft-before.json`, `draft-after.json`, `wrapper-before.json`, `wrapper-after.json`.

The snapshot `Cargo.lock` gained only package `a157-captured-pg-review` 0.1.0, depending on `oo-builder`, `oo-connectors`, `oo-core`, `serde_json`, `sha2 0.10.9`, and `tokio`. The repository lock was not written. The probe was a member of that snapshot only.

## Reader SQL

Harness `probe/src/bin/sql_audit.rs`, `#![forbid(unsafe_code)]`, current workspace dependencies. It classifies both `statement:` and `execute <name>:` lines, including `execute <unnamed>:`. Self-check exit 0: one extended `SELECT`, two simple statements, one `INSERT` counted as a write, one admin `ALTER`, and a disconnect line ignored.

Real log, `log_statement=all`, reader `oo_a157_reader`:

| Counter | Value |
|---|---|
| `reader_statements` | 34 |
| `reader_simple` | 2 |
| `reader_execute` | 32 |
| `reader_data_writes` | 0 |
| `reader_unclassified` | 0 |
| keywords | `SELECT` 30, `START` 2, `ROLLBACK` 2 |
| admin | `ALTER` 1, `SELECT` 3, `SHOW` 1 |

`classify.exit` 0. `START` is a read. The log has no reader `COMMIT`. A substring scan that looks for `CREATE` inside `SELECT` text hits catalog queries whose first keyword is still `SELECT`. The redacted log is `audit/postgresql.redacted.log`.

## Process deviation

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a156/SP07/captured-pg-draft/pg/audit.py` is a Python script, 158 lines. The author task asked for a safe-Rust test harness. This review did not delete or rewrite that file and does not describe it as Rust. `source/process-deviation.txt` records the file. The a157 classifier is the Rust binary above. Shell scripts only start containers and move files.

## Failures kept

| Record | Exit | Cause |
|---|---|---|
| `probe/attempt-1/build.exit` | 101 | Snapshot `/src` was mounted read-only, so Cargo could not write `Cargo.lock` for the evidence-only member. |
| `probe/attempt-1/launch.exit` | 1 | The container exited before `capture.ready`. |

The green probe is `probe/out/` with `launch.exit` 0. Product sources were not edited on either attempt.

## Bounds

The comment sentence is present in `pg/init.sh`, in the `comment.txt` writer in `pg/up.sh`, and as the expected constant in `probe/src/main.rs`. It is absent from the catalog JSON, both drafts, both wrappers, and the redacted log. Passwords and private keys are absent from this evidence directory. `sqlx-postgres` printed two warnings while the probe compiled the connector. Scoped Clippy on `oo-builder` was clean.

a154 containers were left in place. `oo-a154-pg` stayed up. This review did not prune, push, or edit the ledger, specs, or product manifests.
