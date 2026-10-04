# Findings

Verdict: **APPROVE LIMITED** commit `34498e75d5876d74ddb5343b1b0f5b9b1adaf1df`. No product change is requested. The fresh proof is `linux/probe-3/out/outcome.txt`.

## Command boundary

`crates/oo-pipeline/src/bin/oo-pipeline.rs` lines 7–16 accept `catalog-pg` only when no further argument is present. Any other command word, a missing command, or an extra argument prints `oo-pipeline: usage` and returns exit 2 before the runtime starts. Lines 27–32 call `catalog_cli::catalog_pg`.

`crates/oo-pipeline/src/catalog_cli.rs` lines 33–50 read one bounded document, reject a bad protocol as `invalid version`, reject an empty table list as `invalid input`, and build the source with the import command's helpers. Lines 94–99 deny unknown fields, so a `target` object is `invalid input` and is not opened. Lines 53–63 call `inspect_access` and return `registration refused` before `catalog` when `registration_allowed` is false. The session is local to `read_catalog`, so it is dropped before `encode` (lines 66–81) and `write_stdout` (lines 84–92). Encode refuses a protocol other than `oo.source.pgsql/v1`, a document over the scanner ceiling, or a value that does not parse back to the same `SourceCatalog`.

`linux/probe-3/out/narrow/outcome.txt` records exit 2 for no arguments, an unknown command, and an extra argument on both `catalog-pg` and `import-pg`, and exit 1 for empty input, malformed JSON, non-UTF-8, a duplicate key, a bad version, a strict unknown field, an empty table list, and a missing password file. The marker and the absent path are not echoed.

## What the process showed

The reader stdout is `oo.source.pgsql/v1` for source `a166src`, database `oo_a166_src`, 5244 bytes, no trailing newline. An admin query of `pg_attribute`, `pg_index`, `pg_constraint`, `obj_description`, and `col_description` matched columns, primary-key order, `mooring_berth_fk`, and the comment hashes. `oracle.json` stores `byte_len` and `sha256` only. A scan of `linux/probe-3/out/` found none of the fixture comment bodies and no `PRIVATE KEY`.

The writer principal exited 1 with empty stdout and stderr `oo-pipeline: registration refused`. The same test then inserted as that principal and recorded SQLSTATE `42501` for the reader insert. `pg_stat_activity` for `application_name = oo-pg-source` was 0 after both processes. The database list did not change. `_oo` was absent. `pg_publication` was 0.

`oo-builder captured-pg-draft`, run twice on those exact bytes, and `oo-builder review-pg-draft`, given `{"catalog": <object>}`, wrote one 20832-byte document. `wrapper.row_import` is `not_proven`, `wrapper.publication` is `unavailable`, and `wrapper.grants` is `not_authorized`. Five generated columns have `pii` null, with five pending `pii_unknown` items. The foreign key is one `declared_fk` with null confidence and null review, plus one pending `candidate_relationship` whose detail is `join_hint_left_unverified`. Provenance keeps `publication` `unavailable` and `pg_catalog_integration` `not_accepted`.

## Import surface left in place

`crates/oo-pipeline/src/import_cli.rs` line 165 and the following helpers are `pub(crate)` so `catalog_cli` can call them. `read_stdin` at lines 438–440 still calls `read_bounded_stdin("import input")`. The import command's usage line is unchanged. `full_import.rs` blob `dfaf2470419dc5e07fbe92ae60b099ab96ab788c` is the parent object. The non-ignored `import_pg_cli_live` tests passed on the smoke container and on `linux/probe-3` (3 passed, 1 ignored). The ignored import live test was not run. No target was installed.

`crates/oo-pipeline/src/lib.rs` lines 13–18 export `catalog_cli` under `forbid(unsafe_code)`. The crate does not depend on `oo-builder`. The builder binary in this commit is blob `c2f1fe4a4ba11eb36263b4eeb20e17fd9a93c09d`, the same object the parent already had.

## What stayed outside this approval

Parent SP03, parent SP07, a full policy, and publication stay open. The builder's draft says publication is unavailable and grants are not authorized. PII stays unknown. The foreign key stays a tentative candidate. Row import stays `not_proven`. a164 (`a173c58a` on source `3fba40dd`) remains the proof for `import-pg`. This review ran the new command, the narrow exit checks, and the existing non-ignored import CLI tests.

## Harness failures kept

`linux/probe/` inside exit 101. The oracle map needed a type on `BTreeMap::entry`, at `catalog_pg_a166_review.rs` as it stood then. The live test was not started. Import and catalog package tests in that container passed.

`linux/probe-2/` inside exit 101. Clippy rejected a manual `Default` for `HashOnly` (`clippy::derivable_impls`). The live test in that same run passed and wrote `linux/probe-2/out/`. Those files agree with the green run on schema hash, comment hashes, refusal, and the builder seals. The draft digest differs because `captured_at` changed. They are not the deciding oracle.

`linux/probe-3/` is the green rerun. The same private PostgreSQL container was reused. Its data directory was not replaced between attempts. The product tree was not edited.
