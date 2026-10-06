# Findings

Review is limited to `tools/answer-citations` and this evidence directory. The fixed producer is `contracts/gateway/query-pg-cli-v1.md` and `crates/oo-gateway/src/bin/query-pg.rs` at `567d621e2e52b7e0011bdb025dc9337e92ecc5a6`. Blobs are in `source/blobs/`.

## Producer fields that exist

Admit and complete lines carry `protocol`, both SHA-256 hashes, `fingerprint_hex`, `role`, `project`, `policy_id`, `backend_pid`, and `backend_start`. Complete also carries `status`, `truncated`, `returned_rows`, `result_bytes`, `rollback_confirmed`, `socket_dropped`, `stop`, and `sqlstate`. There is no `query_id`, session, turn, principal, authentication, or warning string. The tool does not invent those links. Session and turn are echoed from the caller. Role, project, and protocol are matched to fields the producer writes.

## Admission

A pair admits only when it is a clean success and its role and project equal the binding. The derived id is `q_` plus a 6 to 40 character lowercase fingerprint. The desk fixture admits `q_836041cccc8287ad` for role `oo_r_desk` and project `tickets`. Tests also cover a changed hash, an extra `session` field, a refused completion (`failed`, `database`, `42501`), a missing complete line, a duplicate body, a cross-scope role, a project mismatch between admit and complete, a truncation disclosure, and a truncation conflict. `42501` and `oo_r_peek` are absent from those reports.

`unknown`, `incomplete`, `rejected`, and `crossdeclaredscope` invalidate a cited id. `duplicate` and `querytruncated` do not, except when clean successes disagree on truncation. That conflict is `rejected` and is not admitted.

## Proof failures that stayed on disk

Attempt 1, `linux/postedit`, test 101 and clippy 101. `env!("CARGO_BIN_EXE_answer_citations")` was unset while compiling `tests/cli.rs`. Clippy also rejected unread `returned_rows` and `result_bytes`. Those fields stay on `CompleteLine` so serde enforces the producer types. `check_complete` reads them and the report still omits them.

Attempt 2, test 101. `Result::unwrap_err` required `Debug` on `Outcome`. The report types now derive `Debug`.

Attempt 3, clippy 0, test 101. `serde_json::Value` stores object keys in sorted order, so a reparse does not show the serializer's field order. The order assert now scans the raw `outcome.json()` bytes. Top-level order is `version`, `trust`, `authenticated_audit`, `entailment`, `warning_comprehension`, `session_id`, `turn_id`, `role`, `project`, `protocol`, `citation_status`, `declares_data`, `missing_citation`, `receipts`, `admitted`, `citations`, `warnings`, `disclosures`.

Attempt 4 passed. The CLI test runs the built `answer-citations` binary. If Cargo does not export `CARGO_BIN_EXE`, the test takes the sibling of `current_exe`'s `deps` directory. Help, usage exit 2, empty stdin exit 3, a matching citation exit 0, a missing citation exit 1, and a flipped receipt hash exit 4 were exercised. Stderr on the hash failure is `answer-citations: hash`.

## Boundaries that held

No root manifest or root lock edit. No new crate beyond the renamed serde subset. No `cargo update`. No regex crate. No Python oracle. The SHA-256 used by the tool is `src/sha256.rs`, the same bytes as `tools/pg-onboarding/src/sha256.rs` (`475e4e06083ea5ab38373332cade4ee0b4d6d40f58b779607981368f367ced89` in attempt 4 `package-sha256.txt`). Unsafe code is forbidden. The archive never gained the overlay.

## Not claimed

Entailment, authenticated audit, natural-language warning comprehension, parent acceptance, and release are outside this result.
