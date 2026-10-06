# Findings

Independent review of frozen commit `13566602dc36ce6ebd03fef25a9bd17aae7a281c` for `install-column-acl`. No product, library, manifest, or lock edit. The author a235 proof did not invoke `query-pg`. This review did, on a new database, after fixture SQL that the installer does not run.

## Result

APPROVE_LIMITED. No product defect showed up in the frozen install scenarios, the separate mapping-read grants, the separate `ANALYZE`, the compiled `query-pg` read, or the compiled producer paste.

The three harness failures are preserved under `attempt-1/`, `attempt-2/`, and `attempt-3/`. They were recursion in the review catalog JSON, an owner check applied to the captured-pg-draft evidence envelope, and `clippy::collapsible_if` in the review test. The green Clippy log has no warning.

## What the green run shows

- Generation 3 against stored maximum 7 exits 7 `install-column-acl: policy stale`. `oo_r_stale_gen` is absent.
- A second table `missing_table` exits 7 `install-column-acl: policy mapping`. `oo_r_bad_map` is absent and has 0 column grants.
- Role `clerk` on `trade.party` (`party_id`, `display_name`, `note`) and `trade.ledger` (`entry_id`, `party_id`, `memo`) returns `installed`, role `oo_r_clerk`, grantor `oo_fixture_admin`. `ratio` is absent from the party column list. The success document matches the physical names from `lookup_physical_mapping`.
- The same clerk document returns `replay` with the same `tables` array.
- Role `peek` returns `installed`, role `oo_r_peek`, with 4 column grants. Clerk has 6. Table privileges for both roles are 0. Both roles are NOLOGIN with superuser, createdb, createrole, inherit, bypassrls, and replication false.
- Before any mapping grant, column plus table privileges on `_oo.physical_mappings` for `oo_r_clerk`, `oo_r_peek`, and `oo_a241_login` sum to 0.
- The login, with `SET LOCAL ROLE oo_r_clerk`, reads party id 41, display name `ada`, and note `kept-local`. `INSERT` of `_oo_pos` and `UPDATE` of the ledger memo both return SQLSTATE 42501.
- The fixture then grants schema USAGE, `SELECT` on `_oo.physical_mappings`, and `EXECUTE` on `_oo.lookup_physical_mapping(text, text, bigint, text, text, text, text)` to `oo_a241_login`. `has_schema_privilege`, `has_table_privilege`, and `has_function_privilege` are true. `reltuples` is still below 0.
- Compiled `query-pg` with `unknown_rows=refuse` exits 5 `query-pg: policy` and does not create the audit file.
- After `ANALYZE` on both physical tables, `reltuples` is no longer the unknown sentinel. The same `query-pg` request exits 0, returns the two typed rows, and writes a local JSONL receipt with `end_audit_ok` true. The query document omits `row_filter`. Install documents still send `row_filter` null.
- `oo-builder captured-pg-draft`, `reviewed-effective-oos`, and `reviewed-policy` from this commit produced a policy array. Pasting that array into `install-column-acl` against the trade database exits 7 `install-column-acl: policy stale`. The `oo_r_` role count and the clerk column-grant count stay unchanged. The reviewed-policy stdout does not contain `owner_role_do_not_leak` and does not end with a newline.
- The `install-column-acl.rs` source in the archive contains no `query-pg`, no `ANALYZE`, no `GRANT`, and no `provision_mapping_reader`.

## Boundary

Mapping-read grants and `ANALYZE` are SQL in the review fixture. They are not behavior of `install-column-acl`. The local query receipt is not an audit WAL. Authentication, publication, and row-level security were not measured. The grantor in this proof is superuser `oo_fixture_admin`.

The committed `install_column_acl_cli` suite was compiled and its 12 tests passed with the live environment unset. That does not repeat the a235 live oracle. The live evidence for this review is `install_column_acl_a241_review`.

## Open

M1-D14D stays open. A non-superuser grantor, authentication, an audit WAL, publication, and row-level security stay out of this verdict.
