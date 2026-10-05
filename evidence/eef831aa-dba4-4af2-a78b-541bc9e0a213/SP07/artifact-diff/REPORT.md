# Report

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a213`. Fixed source `a661dcdf72ff91561ae6b7cb79916e582884d08a`, tree `0a8f46576d5f4180ca44ce5f33a414e00c923437`. Repo HEAD when `linux/overlay-3` launched was `ca2e248d38e90e6e12743bf607a5793ce6798ac5`. Date: 2026-10-06.

This verdict covers `ontology-artifact-diff` for two explicit `oo.builder.captured-pg-draft-command/v1` files. O-D01 stays unaccepted. SP07-W05, owner authority, authentication, and publication stay open. `auto_accept` is false. `publication` is `unavailable`. `certification` is `none`. `grants` is `not_authorized`.

The compiler under test is `compile_captured_pg_catalog` from the fixed archive. The diff command does not open a database and does not call a model. PostgreSQL was not started.

## What was read

- `contracts/builder/captured-pg-draft-v1.md`, `contracts/builder/structure-draft/README.md`, and the frozen `contracts/builder/artifact-diff-v1.md`
- `crates/oo-builder/src/captured_pg.rs`, `draft.rs`, `pgsql.rs`, and `src/bin/oo-builder.rs` at `a661dcdf` for the public artifact shape
- `source/blobs/ontology-artifact-diff.rs`, `source/blobs/ontology_artifact_diff.rs`, and `source/blobs/artifact-diff-v1.md`
- a204 launch recipe `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh`, commit `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, sha256 `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`
- a199 manifest tool `tools/sha256_manifest.rs`, sha256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`
- a200 was read as the PostgreSQL recipe and was not executed

## Pre-edit smoke

`linux/preoverlay` is an archive of `a661dcdf` before the diff command existed. The path list omitted `examples/`. `draft::tests::checked_in_goldens_match_compiler_bytes` panicked at `crates/oo-builder/src/draft_tests.rs:1792` because `failures/adapter.code` was absent. `lib.stdout` shows 27 passed and that 1 failure. `lib.exit` 101. `docker-run.exit` 101.

`captured-pg.exit` 0. `captured-pg.stdout`: `ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s`, including `parser_locks_the_shop_schema_hash`. `clippy.exit` 0 on `--lib --test captured_pg -- -D warnings`. `clippy-component.exit` 0. `fetch-offline.exit` 101 is the cold private cargo volume. The same script then fetched online and recorded `fetch.exit` 0.

Rust `1.94.0` (`4a4ef493e 2026-03-02`), `aarch64-unknown-linux-gnu`, Debian 13. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. Container `723377b98c4ee906e92f5342eebbe2b7456030fbfa764b0825dbe9e6321c04a3`. Memory `6442450944`. `NanoCpus=1000000000`. `CapDrop=["ALL"]`. `OOMKilled=false`. `Privileged=false`. Network `oo-a213-eef831aa-net`.

## Decisive run

`linux/overlay-3` is a fresh `git archive` of `a661dcdf` including `examples/`, plus the two owned Rust files. `overlay-sha256.txt` matches the worktree blobs:

- `ontology-artifact-diff.rs` `ce0ff74acdd8672f6826e33da2d4cb2a4f53f0fcff4d9c57a16d96333548fb4e`
- `ontology_artifact_diff.rs` `857205c2251e837be44b0b9ce517f218fcebcd96804295182e816b0842677fc0`
- contract `a139649d0c19f83e040046bc76ecc7a110b5aed873b96113dcc0e26bbb8f9d6e`

`capacity.txt`: `own=0 global=1 host_cargo=0 cap_global=10`. One other rust-builder was already running. This task started one container. `fetch-offline.exit` 0. `fetch.exit` 0. `overlay-present.exit` 0. `test.exit` 0. `clippy-component.exit` 0. `clippy-bin.exit` 0. `clippy-test.exit` 0. `clippy.exit` 0. `docker-run.exit` 0.

`test.stdout`: `ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.52s`.

Container `5b58e88db76a98dfd7275e95b0e3ef6778a43c51fc4e27c3111535904cff829a`. Same image, 6GiB, 1 CPU, cap drop, and private network. `/src` was read-only. `tmpfs /tmp` is `rw,nosuid,nodev,noexec,size=1g`. The container was removed.

Clippy is two commands, because the earlier combined `--bin --test` invocation did not define `CARGO_BIN_EXE`:

```text
cargo clippy --locked --offline -p oo-builder --bin ontology-artifact-diff -- -D warnings
cargo clippy --locked --offline -p oo-builder --test ontology_artifact_diff -- -D warnings
```

`clippy.stderr` ends with `Finished dev profile` for each. No warning text.

The harness calls `compile_captured_pg_catalog` on two shop catalogs. A local preimage is locked to `SHOP_SCHEMA` `bcb019d92270bf12092a3f47697b25719cc087a1ac4b7c56a10cf6dd00294c4c` and `STALE_SCHEMA` `31bbede923a28e58c8fef54240de5458533fa881e4d5898cdd31504b4aa0bd7e` before the compiler runs. The compiler output is the acceptance oracle. The harness hashes with `oo_core::api::payload_sha256` and its own canonical writer. `unchanged_catalogs_emit_a_deterministic_sealed_diff` requires that writer to equal `wrapper.draft_hash` and the diff seal.

Observed on the compiled binary:

| Case | Result |
|---|---|
| Identical shop catalogs | exit 0, `facts` empty, `unchanged` 33, `schema_hash_equal` true, two runs byte-identical |
| Flag order reversed, pretty JSON, key reorder | same stdout bytes |
| Column `email` renamed to `contact_email` | `unchanged` 10, physical and generated column removed and added, source `changed` |
| `email` `type_oid` 25 to 1043 | `unchanged` 12, physical and generated column `changed`, `type_reason` `known_builtin_not_converted` |
| FK `purchase_customer_fk` loses the `tenant_id` pair | `unchanged` 12, same foreign-key id, physical and generated `changed` |
| `not_null` move plus curated classification and a decision bound to the before schema hash | `unchanged` 12, physical column `changed`, generated column `stale`, curated classification `added`, decision and queue `stale`, `auto_accept` false |
| Duplicate physical column, resealed | exit 1, `duplicate.fact` |
| Unsealed duplicate | exit 1, `hash.mismatch` |
| Usage, missing file, directory, symlink, oversize, bad UTF-8, syntax, duplicate key, bad protocol, bad historical commit, bad publication, zero draft hash, extra key | the documented exit and static line |

Stdout and stderr of those runs omit `a156-comment-body-do-not-carry`, `owner_role_do_not_leak`, `secret-owner-name-a213`, `do-not-echo-label-a213`, and the refusal sentinels.

## Preserved failures

`linux/overlay` container `7cb4b63ae517297db1751d3dd9857f4326faf5c409b947180fc6638e5e819770`. `test.exit` 101, `clippy.exit` 101, `docker-run.exit` 101. `format!("\\u{ch:04x}")` does not implement `LowerHex` for `char`. Clippy also reported `CARGO_BIN_EXE_ontology_artifact_diff` undefined under combined `--bin --test`.

`linux/overlay-2` container `36a682cd8996b0d251faeecbcaaab1cb033a75aec2e412eb8d2222ecb7d734ca`. Compile succeeded. `preimage_locks_the_checked_in_shop_hashes` passed. The other 8 tests panicked at `tests/ontology_artifact_diff.rs:908` because `std::env::var("CARGO_BIN_EXE_ontology_artifact_diff")` was `NotPresent`. Clippy rejected four nested `if`s, one redundant closure, and `diff_map` with 8 arguments. `test.exit` 101, `clippy.exit` 101, `docker-run.exit` 101.

Those directories were not overwritten. `linux/overlay-3` uses `env!("CARGO_BIN_EXE_ontology-artifact-diff")`, let-chains, `is_some_and(is_hex)`, and a `MapSpec` argument.
