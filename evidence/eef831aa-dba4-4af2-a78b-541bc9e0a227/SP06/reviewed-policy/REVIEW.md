# Review

Same session as the implementation. Frozen scope only: `crates/oo-gateway/src/reviewed_policy.rs`, the `lib.rs` export, `crates/oo-gateway/tests/reviewed_policy.rs`, and `contracts/gateway/reviewed-policy-v1.md`. This is not an independent review and it does not accept M1-D14B.

## Checked against the green run

`compile_reviewed_policies` takes the command stdout, the artifact JSON, the decisions JSON, and one `ReviewedBinding`. Budget is 1048576 bytes per document, then `parse_json_value`, then canonical command bytes, then the forbidden-key walk, then shape and seals. Duplicate pairs are refused before `decisions_sha256`. Identity is the exact display sentence in the contract. `lib.rs` adds the module and the public re-export and does not change crate docs or `policy`.

`verify_seals` recomputes wrapper, envelope, draft, manifest preimage, generated, and curated digests with the module's canonical writer and `oo_core::api::payload_sha256`. The trust root is the caller's `schema_hash` and `artifact_sha256`. Command non-claims are `not_claimed`. The green `seal_tamper_refuses` and the two-role compile both passed on bins built in the container from the fixed archive plus this adapter. There is no `oo-builder` dependency and no call to `compile_structure_draft`.

`admit_one` returns `unknown` or `rejected` for a missing column, `missing` when no decision is present, then `stale_schema`, `unbound`, `stale_fact`, and effective-status `rejected`, then `mismatch`, then `seal` if `hidden` lists the bound column. `column_spec` reads semantic type from the typed effective column, checks the sealed catalog type before RLS, and maps PII from the decision plus `explicit_allow`. `group_policies` keeps first-seen table order and allow order, sets `row_filter: None`, and `compile_column_acl_set` wraps `PolicyError`. The empty allowlist test passed.

The 21 tests cover two tables and two roles, stale schema, stale fact, rename, unknown, hidden-after-bind, rejected, missing decision, explicit PII, duplicates, mixed identity, forbidden keys, RLS, unsupported semantic types, protocol-only JSON, and seal tamper. The source-guard test does not scan `lib.rs`, where older install exports remain.

## Left as specified

Gateway contract step 7 says `provenance.draft_hash` on the draft. The implementation follows the a217 command: `wrapper.draft_hash` and `command.provenance.draft_hash`. Real draft provenance has no `draft_hash`. That reading is in `FINDINGS.md`. The contract was left frozen.

The decision reason word `hidden` is listed and is not returned. A hidden column after a successful bind is `seal`, which is step 14.

Clippy `-D warnings` on `--lib --test reviewed_policy` exited 0 in `linux/retry2`. That command lints the whole `oo-gateway` library plus this test. It does not lint the other gateway integration tests. The pre-edit smoke covered `policy::tests` only.

No PostgreSQL was used. No dependency file changed. Authentication, install, grants, and publication stay outside this adapter. M1-D14A, M1-D14B, and SP06-W01 stay open.
