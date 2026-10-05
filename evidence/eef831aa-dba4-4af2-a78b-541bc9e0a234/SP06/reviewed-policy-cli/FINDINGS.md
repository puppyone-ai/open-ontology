# Findings

Author check of commits `e21f20d2894d0ffc584fb68709c0e355d94201a6` and `bcc8673463e8191f2cb9e8efb34ccc2c73574c8d` against `contracts/gateway/reviewed-policy-cli-v1.md` and the adapter `compile_reviewed_policies`. This is the same session that wrote the command. It is not an independent review. M1-D14C stays unaccepted. No ledger line was edited.

## What the command does

`reviewed-policy` reads one operator document from stdin and calls `oo_gateway::reviewed_policy::compile_reviewed_policies` with the command string, the artifact string, the decisions string, and one `ReviewedBinding`. One call is one role. The shop proof uses two processes for `clerk` and `peek`. Stdout is compact JSON, struct field order, no trailing newline. Stderr on success is empty. On refusal stdout is empty and stderr is one `reviewed-policy:` line.

The adapter still checks canonical command bytes before forbidden keys. Forbidden keys `samples`, `credentials`, `privileges`, `password`, `secret`, and `row_filter` are the adapter's walk. A decisions digest mismatch is `seal` before a stale decision. The stale-schema fixture recompiles `reviewed-effective-oos` so `decisions_sha256` matches. Decision reasons that pass the command's allowlist are `unknown`, `missing`, `stale_schema`, `stale_fact`, `rejected`, `unbound`, and `mismatch`. Any other reason word is dropped and the line is `reviewed-policy: decision` with no extra text. `PolicyError`'s display, which can include a column name, is not printed.

## Exact remaining seam

`ColumnAclPolicy`, `LogicalTable`, `ColumnSpec`, `ClassStatus`, `PiiClass`, `RowFilter`, and `ReviewedBinding` do not implement `Serialize` or `Deserialize`. The command's `OutputDoc`, `PolicyOut`, `TableOut`, `ColumnOut`, `PiiOut`, and `RowFilterOut` are private to `crates/oo-gateway/src/bin/reviewed-policy.rs`. The integration test declares its own deserialize types and `to_policy`, then calls `oo_gateway::policy::compile_column_acl_set`. Those test types are not the bin types. Shared types were not given serde and `lib.rs` was not edited, so `compile_column_acl_set` stays at `oo_gateway::policy` and is not a crate-root export.

`query-pg` already deserializes the same field names in its private `PolicyDoc`. That type is not linked to this command. Pasting `policies` into a query-pg request is a later harness. This command does not run `query-pg` and does not install.

`row_filter` null is the adapter's success value. The DTO can also carry `{ "kind": "claims", "column", "claim" }`. The green test converts a hand-built claims object to `RowFilter::Claims` and `compile_column_acl_set` returns `PolicyError::RowFilterRefused`. The command itself emits null for this adapter.

`oo_core::json_scan::parse_json_value` accepts one value of at most 1048576 bytes. The operator document is that one value. `command`, `artifact`, and `decisions` are strings so the producer bytes pass through. Three adapter documents near that cap do not fit in this envelope. An adapter `Budget` from a decisions array over 4096 still maps to exit 3. The stdin cap of 1048577 bytes is the budget the green test hits.

A column that is in `hidden` after a successful bind is the adapter's `seal`, exit 8. The command does not emit a `hidden` decision reason. The adapter contract lists that word and step 14 assigns the case to `seal`.

## What stays open

The command does not authenticate the caller, the role, or the login. It does not install, grant, or publish. It does not write an audit WAL. It does not prove live row-level security or a non-superuser grantor. A missing executor completion does not apply: the process has no session.

M1-D14C and SP06-W01 stay open. No parent plan item was accepted. No dependency was added.
