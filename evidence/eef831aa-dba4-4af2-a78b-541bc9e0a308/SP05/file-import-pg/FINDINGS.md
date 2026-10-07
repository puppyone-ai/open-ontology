# file-import-pg findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a308`. Observations from the passing `linux/proof` run and the preserved failed attempts. Scope is this replace child only.

## Import behavior that the proof showed

- Caller state `awaiting_review` and stored `_oo.full_import_run.state` `awaiting_publish` are different strings. Seal grants nothing. `published` and `role_grant` stay false. No live pointer is moved.
- `full_import_write_batch` stores position codec `snapshot-zero/v1` and lineage `full-import.snapshot`. Those names are hardcoded by the existing writer. This child does not emit `batch-cursor-us/v1`.
- One sqlx connection holds `BEGIN` across COPY, `write_batch`, `SET CONSTRAINTS ALL IMMEDIATE`, `note_table`, and `seal`. The shadow constraint trigger `full_import_guard_shadow` is deferred. Attempt 3 fired it at COMMIT after seal had set `awaiting_publish`, SQLSTATE `OO031`, code `run_closed`. The passing run fires the constraint while the run is still `open`, then notes and seals. COPY failures stay driver tokens `copy_start`, `copy_send`, or `copy_finish` and map to `database`.
- PostgreSQL `bool` text for the fixture is `true` and `false`. Attempt 4 expected `t` and `f` after the row was already committed. Numeric text was `1.50` and `0.00`.
- Delimited unquoted empty under policy `empty` is SQL NULL. Under policy `token`, unquoted empty is an empty string. Quoted fields are values, including a quoted null token and a quoted empty field. JSONL missing and JSON null both have a null Arrow slot. Presence distinguishes them. A missing required field is rejected after the worker finishes and before connect.
- A finished JSON object with no trailing newline is a clean EOF in `crates/oo-connectors/src/jsonl.rs`. Attempt 6 imported `{"id":"t1"}` as one row. The passing fixture is `{"id":"t1"`, cut off before the closing brace, and the worker returns protocol fact `truncated`.
- Worker stderr keeps the tokens `truncated` and `header_required`. Other protocol facts collapse to `protocol`. The int8 overflow fixture and the extra-field CSV fixture therefore surface as `malformed` / `protocol`, not as a distinct `typed_overflow` or width token.
- `pg_terminate_backend` asks the server to end that backend. The test waits until `pg_stat_activity` no longer lists the pid, observes that the shadow regclass is gone, and then imports the same file once. This is not a claim that SIGKILL arrived inside a COPY packet.
- `/proc` scan after success and refusal sees no `oo-file-parser-worker`. Processes that left the group with `setsid` would not be covered. `kernel_restriction` is `unsupported`.
- Bootstrap `0000_bootstrap_roles.sql` uses bare `CREATE ROLE` for `oo_target_owner` and `oo_pipeline_runtime`. `DROP DATABASE` leaves those cluster roles. Attempt 5 failed the next install at `CREATE ROLE`. A clean retry drops those two roles after revoking `oo_pipeline_runtime` from `oo_a308_runtime`.
- Fixture `max_fields` is 64. The worker field cap is 256, so the admitted budget stays inside that cap.
- Parsing mode on the request and the result is `trusted_public_fixture`. Production-untrusted parsing and runsc are unsupported. No certificate was produced for them.
- Append and upsert are unsupported. Parquet, Excel, sqlite, and database dumps are unsupported and are not parsed by this crate.
- The first offline refuse test resolves the tool lock. `lock_subset` then requires every checksum to exist in the committed root lock. The passing lock has 250 checksums and no git source. It was copied into `tools/file-import-pg/Cargo.lock` only after docker exit 0.

## Limits

Admin and operator review, ACL publication, and moving a live read pointer are separate work. This proof stops at `awaiting_review` with the shadow readable by the installing admin and denied to `oo_a308_runtime` and `oo_a308_agent` with SQLSTATE `42501`.

The passing live test is one Tokio test. It does not by itself close the parent SP05-W05 goal.
