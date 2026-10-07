# file-import-pg review findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a330`. Observations from the passing `linux/proof` run on commit `15558917e2c5be12b74dd1d64fb542b7d05a03af`, plus the four preserved review attempts. Scope is child FILE-D02 / E39.

Verdict for this child: **APPROVE_LIMITED**.

## What the passing run showed

- Caller protocol `oo.pipeline.file-import-pg-result/v1` returns state `awaiting_review`, `published` false, and `role_grant` false. Stored `_oo.full_import_run.state` is `awaiting_publish`. Both strings are intentional. Seal grants nothing.
- `full_import_write_batch` stores position codec `snapshot-zero/v1` and lineage `full-import.snapshot`. The receipt query on the CSV source returned that pair and batch id `b0`. This child emits those names.
- Shadow `relpersistence` is `p`. PUBLIC grantee 0 count is 0. `pg_roles` stayed 20. Runtime and agent `SELECT` return SQLSTATE `42501`.
- The shadow constraint trigger `full_import_shadow_commit` is `DEFERRABLE INITIALLY DEFERRED` and calls `_oo.full_import_guard_shadow`. The function source requires `state = 'open'`. The CSV import, the complete JSON object, and the post-terminate retry all committed and stored `awaiting_publish`. A seal that ran before `SET CONSTRAINTS ALL IMMEDIATE` would abort that trigger with SQLSTATE `OO031`, which is the author attempt 3 failure on the earlier tree. Fixed `tools/file-import-pg/src/apply.rs` issues `SET CONSTRAINTS ALL IMMEDIATE` before `note_table` and seal. The passing commits are the behavioral check of that order.
- Delimited policy `empty`: unquoted empty is SQL NULL and quoted empty is an empty string. Policy `token` with token `\N`: unquoted token is SQL NULL, quoted token is the text `\N`, and both empty forms are empty strings. Delimited cell text `null` stays the text `null`. JSONL JSON `null` is SQL NULL. JSONL object text stays JSON text. A missing JSONL field returns `missing_field` / `missing_field`, exit 2, and adds no `full_import_run`.
- A complete JSON object `{"id":"done"}` with no trailing newline imports one row. A cut object `{"id":"cut"` returns `truncated` / `truncated`, exit 4, and adds no run. The author passing fixture was only the cut object. This review is the separate success proof for the finished object.
- Extra CSV field and int8 `9223372036854775808` both surface as `malformed` / `protocol`, exit 4. The worker allowlist collapses those protocol facts to `protocol`.
- Replay returns no `run_id`, keeps one run and three rows, and does not adopt a new generation. A changed object on the same receipt returns `hash_conflict` / `receipt`, `child_started` true, exit 3, and leaves the rows. Rewriting the file after the request hash, before `ParentConfig::start`, returns `hash_drift` / `object` with `child_started` false and exit 2.
- `pg_terminate_backend` on the staged COPY backend removed pid 198. The shadow relation was absent and the run count stayed 5. The same bytes then imported once as `k,kept`. This is a server terminate of that backend. It is not a SIGKILL inside a COPY packet.
- `/proc` after each case shows no `oo-file-parser-worker`. The scan is the builder mount namespace. A process that left the group with `setsid` is outside that scan. `kernel_restriction` on the result is `unsupported`.
- Parsing mode is `trusted_public_fixture`. Production-untrusted parsing and runsc were not exercised. Append, upsert, Parquet, Excel, sqlite, and database dumps were not exercised.
- Bootstrap `0000_bootstrap_roles.sql` creates `oo_target_owner` and `oo_pipeline_runtime` for the whole cluster. `fresh_catalog` is false while those roles exist, even on an empty new database, and `install` then stops. Attempt 4 panicked there after the author live. The passing oracle drops the author database and those roles, then installs `oo_a330_file`. The author live itself is not idempotent on a database it already filled: attempt 3 failed at `live.rs:584` with `replay=true`.

## Preserved attempts

Attempts 1 through 4 are harness or cluster-state failures. Attempt 2 and attempt 4 already reproduced the author refuse, Clippy, and live results on this fixed commit. Attempt 1 never reached those tests because cargo 1.94 `fetch` rejects `-p`. The passing run uses `cargo fetch --locked` with no package flag.

## Limits of this approval

This approval covers the replace path exercised above: compiled `oo-file-parser-worker`, canonical projection, binary COPY into one LOGGED unpublished shadow, fence generation, and full-import receipt, on PostgreSQL 17.10 with the pinned Rust 1.94 builder.

Parent SP05-W05 and AT-FILE-06 remain open. Admin review, ACL publication, and moving a live read pointer are outside this child. Append and upsert are outside this child. Other file formats are outside this child. Production runsc is outside this child. The position codec in the receipt is `snapshot-zero/v1`. The terminate proof is `pg_terminate_backend` on the open COPY backend.
