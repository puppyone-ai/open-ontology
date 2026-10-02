# SP07 structure closure — a081 report

This slice repairs two a071 findings on the structure-draft compiler. It does not complete the structure compiler, SP07, or the vertical loop.

Reviewed input: commit `4c989bce58407cf19d3d93dc600162d7322d92f8`, decision `request_changes`, session `eef831aa-dba4-4af2-a78b-541bc9e0a071`.

Closed here:

- `effective-fk-names-rejected-column` (finding 3). A fresh rejection of a source column, a referenced column, any member of a composite foreign key, or either side of a self-reference removes that relationship from `effective` and queues `relationship_endpoint_not_effective`. The detail is `from_column_not_effective`, `to_column_not_effective`, `from_and_to_column_not_effective`, or `endpoint_dataset_not_effective`. The item is pending and `publishable` false. Generated evidence, the curated echo, and the historical decision stay. An accept is not rewritten. A stale column rejection removes nothing. An unrelated relationship stays. A human rejection of the relationship itself does not gain this queue item.
- `duplicate-key-member-accepted` (finding 4). A repeated member inside one primary key, unique key, or foreign-key source or referenced list is `oo.builder.catalog-evidence/v1/duplicate.key_member` at the key path, message `repeated key member`. Attnum, name, and NFC-ident aliases are covered. Separate foreign keys may share a column. Member order is the input order. Hash checks run first: a wrong catalog hash stays `hash.schema`, and a wrong evidence binding stays `hash.evidence`.

Still open, for a later dependency correction after SDK handoff:

- `nfc-composition-equal-class` (finding 1)
- `unicode-data-notice-missing` (finding 2)
- `nfc-unicode-17-combining-classes` (finding 5)

Still open residuals from the same review, plus the slices named in the task: unsigned numeric scale, physical target ident length, unverified foreign keys, the UCD generator, the unused `oo-jobs` dependency, PostgreSQL integration, RBAC, PII, LLM, publishing, and query. `oo-core` was not changed. `nfc.rs` and `nfc_tables.rs` were not changed. No manifest, lock, or root dependency edit is in this delivery. a073 remains the mainroot dependency writer.

## Baseline

Immutable `git archive` of `a45c13c0e231dcbf6c7b97050b9bfbe8a11bb571` (parent `0b544cb066292221016899c9b5eceef84e984227`, tree `52a2125ce0e07452cc59692e486146c93b3427b0`). Builder, structure-draft contract, and ontology-draft goldens had an empty diff against `4c989bce`.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Run `3bf0c1b7-a66f-46ec-ac34-526cf13231f0`. Container `oo-a081-msrv-3bf0c1b7-a66f-46ec-ac34-526cf13231f0`. Bridge network, `--pull never`, `--cap-drop ALL`, no new privileges, memory 8g, cpus 4, tmpfs `/tmp:rw,nosuid,nodev,size=1g`.

Commands and integer exits, from the phase files:

```
cargo fetch --manifest-path Cargo.toml --locked
fetch.exit=0
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder
test.exit=0
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings
clippy.exit=0
clippy-component.exit=0
docker-run.exit=0
```

Lib tests on that tree: 18 passed. No earlier exit file was rewritten.

## Fixed tree

Linux archive tree `93c6cc57f1ad3dc6e7dcc67ba8761316405ced91`. It is base `911f317cde6d87ec49c247afc35681c7c64d5c91` (tree `a8a642b13e40f084bc884b6ba656ec09e62f1d86`) plus the owned builder sources, the structure-draft contract, and `contracts/builder/structure-draft/fixtures`. The worktree blobs of those sources match this tree. Dirty workspace `Cargo.toml` and `crates/oo-connectors/Cargo.toml` were left out of the archive and out of the commit.

Same image, toolchain, and command list. Run `daa20988-8e6b-47b0-858d-0e29a7f7b6cc`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `aarch64-unknown-linux-gnu`.

```
fetch.exit=0
test.exit=0
clippy.exit=0
clippy-component.exit=0
docker-run.exit=0
```

Results: lib 27 passed, `structure_draft_cli` 4 passed (including `repeated_key_member_exits_nonzero_without_echoing_the_name` and `rejected_fk_endpoint_closes_the_relationship_on_the_cli`), doc tests 0, clippy clean. `checked_in_goldens_match_compiler_bytes` passed. Goldens were not regenerated.

Host checks before that run used Homebrew `rustc 1.98.1` and are not the MSRV result. They also passed: `cargo test --locked --offline -p oo-builder` and the same Clippy command.

## Fixture conclusions

`counterexamples/conclusions.json` is the machine summary. Before is the baseline Linux binary (`sha256 7aaeb0d24056c540fd6db0770794d9695bf1de75f65f25568a0790a55abb19e8`). After matches the fixed Linux binary (`sha256 de688f114cb7c8819eb51e040d6f392d8168ae20cd5d1279468bbc18b67456f0`) byte for byte, including stderr and exits.

| Fixture | Before | After |
|---|---|---|
| `duplicate-pk-member` | exit 0, effective primary key repeats the member | exit 1, empty stdout, `duplicate.key_member`, name absent from stderr |
| `duplicate-pk-attnum-alias` | exit 1, `key.unknown_column` | exit 1, `duplicate.key_member` |
| `duplicate-pk-name-alias` | exit 1, `key.unknown_column` | exit 1, `duplicate.key_member` |
| `duplicate-pk-wrong-schema-hash` | exit 1, `hash.schema` | exit 1, `hash.schema` |
| `fk-from-endpoint-rejected` | exit 0, effective `bill_to_fkey`, `ship_to_fkey`, `warehouse_fkey`; `bill_to` already absent | exit 0, effective `ship_to_fkey` and `warehouse_fkey`; closure `from_column_not_effective` |
| `fk-to-endpoint-rejected` | exit 0, both customer foreign keys remain; customer primary key already empty | exit 0, only `warehouse_fkey` remains; both customer foreign keys `to_column_not_effective` |
| `fk-composite-local-rejected` | exit 0, `invoice_account_fkey` effective | exit 0, relationship removed, `from_column_not_effective` |
| `fk-self-from-rejected` | exit 0, `employee_manager_fkey` effective | exit 0, relationship removed, `from_column_not_effective` |
| `fk-stale-column-rejected` | exit 0, `bill_to` and `bill_to_fkey` remain | exit 0, same facts remain, no closure item |

a071 historical probes, exact `phase1.jsonl` lines 27 and 58, are in `counterexamples/historical/`. Line 27 keeps effective `bill_to_fkey` after `bill_to` is gone. Line 58 emits primary key `["id", "id"]`. Both library and CLI exits are 0. Those probe inputs were not stored; the table above is the reconstructed pair.

Publication stays `unavailable` on every successful draft in this set. Stderr for the duplicate-member errors does not contain `leak_pk_9f3a`, `alias_pk_9f3a`, or `DO_NOT_SURFACE_INSTRUCTION_9f3a`.
