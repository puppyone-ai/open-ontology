# Findings

Author check of the role-pack command on `af3308c32fa9e90bb29fab970b691d57dbc35279`. This session wrote the command, the tests, and this record. It is not a separate independent review. SP07-W06 is not approved.

## Closed in the harness, preserved

F-H1. `linux/focused` exited 101. `two_roles_match_the_library_pack_and_drop_hidden_metadata` panicked at `tests/role_pack_cli.rs:138` with `oo.builder.role-pack/v1/oos.invalid` path `/metrics/0`, message `a metric failed OOS validation`. The certified fixture metric had no `default_anchor`. The accepted compiler requires one. The CLI was forwarding that compiler error. The fixture now sets `default_anchor` to `month_start` on both metrics. `role_pack.rs` was not edited. That run also failed clippy because the fresh container had not installed the component. Later runs install `clippy` for toolchain 1.94.0 before `cargo clippy`.

F-H2. `linux/focused-2` exited 101 at `tests/role_pack_cli.rs:473`: `missing SELECT mass_kg AS total FROM wharf.crate`. Clippy was already green, and the command stdout had already matched the library pack. `metric_kept` omits a metric when a bare identifier matches a column on an ungranted dataset. The legal SQL named `mass_kg`, and the hidden `wharf.scale` dataset had a column of the same name. The fixture renamed that hidden column to `reading_kg` and asserts the clerk stdout omits `reading_kg` and `scale_id`. The legal SQL is present in `linux/commit/out/clerk.stdout.json`. `role_pack.rs` hash is still `e9349ccd9b0e4576e0b4dd712e28aba0f5e7562dba72413d431aac3d14a7d544`.

## Checked on the deciding run

The command document is scanned with `oo_core::parse_json_value` and `deny_unknown_fields` before `compile_role_pack` (`role_pack_cli.rs`). Scanner budget and duplicate-key codes are kept. Their messages are static sentences. Serde's `unknown field` and `missing field` text is classified and is not printed. Compiler errors copy `code` and `message` and drop `path`, so a path that repeats input is not written to stderr.

`command_document` splices `RolePack.artifact` and `manifest.json` bytes. Before that, it requires the six manifest statuses and requires each artifact file's path, text, and byte length to match `pack.file`. It does not recompute `pack_digest`.

Usage lists `structure-draft`, `captured-pg-draft`, `review-pg-draft`, and `role-pack`, and it states that visibility is caller-supplied public data and is not a live permission proof. Extra arguments on all four commands exit 2 without echoing the argument. Unknown command `publish` exits 2.

The two-role test compares the binary with `compile_role_pack` on both canonical and unsorted JSON, then checks an independent `sha2` 0.10.9 digest of the file records. No dependency was added. `Cargo.lock` was not edited by this commit.

The temp directory passed to the binary stayed empty. No publication, mount, or grant call exists on the command path.

## Limits that stay open

The visibility object is trusted public data. This command does not prove it against a live permission system. Manifest statuses stay `not_claimed`, `not_performed`, `omitted`, and `row_security_reproven: false`.

UTF-8 and stdin I/O failures still use `oo.builder.catalog-evidence/v1/json.syntax` and `oo.builder.catalog-evidence/v1/io.read`, because the shared reader was left in place. The contract records those codes.

Serde error classification matches on the English phrases `unknown field` and `missing field`. A change in serde's wording would fall through to `schema.invalid` with a static sentence. The review command uses the same classification style.

Domains, the tokenizer, the read API, search, mount, authentication, the policy adapter, row-security re-proof, and publication gates G1–G13 are outside this command. A successful exit is not SQL execution and not a grant.

The pre-edit offline `cargo fetch` on an empty private volume exited 101, then the online fetch exited 0. Focused and deciding runs fetched offline from that volume and exited 0. That first 101 is recorded under `linux/preedit/fetch-offline.exit`.

Slot B's uncommitted gateway files and dirty `Cargo.lock` were present in the worktree and were not part of `af3308c3` or of the deciding archive.
