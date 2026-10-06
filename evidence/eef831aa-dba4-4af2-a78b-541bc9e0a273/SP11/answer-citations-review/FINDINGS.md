# Findings

Review scope is `tools/answer-citations` at `7e404dd3d36cdeecb553ab0c707dbfe82ba94c62`, built from the fixed archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`. Those two product trees are the same object, `f3b20319347cdc119dbc97d832974f5dcedb4362`. Producer blobs are `source/blobs/query-pg.rs` and `source/blobs/query-pg-cli-v1.md`. The contract blob is `source/blobs/CONTRACT.md`. No product file was edited.

## Producer fields

`linux/smoke/audit-fields.txt`, from the archive copy of `query-pg.rs`:

- Admit: `protocol`, `record`, `original_sha256`, `executed_sha256`, `fingerprint_hex`, `role`, `project`, `policy_id`, `backend_pid`, `backend_start`.
- Complete adds `status`, `truncated`, `returned_rows`, `result_bytes`, `rollback_confirmed`, `socket_dropped`, `stop`, `sqlstate`.
- The forbidden set checked by `tools/audit_fields.rs` (`query_id`, `session`, `session_id`, `turn`, `turn_id`, `principal`, `authenticated`) is absent from those two structs.
- `SummaryLine` includes `turn`. The oracle records `summary_turn=stdout_only`. That line is the command's stdout summary, not the JSONL audit the citation tool reads.

The derived citation id is `q_` plus the fingerprint. Session, turn, and the operator role and project come from the request binding. They are not fields the producer audit writes. `authenticated_audit` is false on every successful report the harness parsed. That binding is not a full production citation and it is not an authenticated audit.

## Admission observed on the compiled CLI

Attempt 2 `test.exit` is 0. The nine harness tests call the `answer-citations` binary.

- Desk receipt hash and length match, exit 0, admitted `q_836041cccc8287ad`, raw top-level key order matches the contract serializer, and the report omits the answer text, SQL, the SQL hashes, the policy id, and `backend_start`.
- Fresh in-scope pair admits `q_a273a273a273`. A role cross is `crossdeclaredscope` and does not print `oo_r_rev`. A project cross does not print `cite273`.
- An uppercase citation id stays `unknown` and is not the admitted id. Fingerprint length 5 is not a citation. Lengths 6 and 40 admit. Length 41 does not admit, and that fingerprint string is absent from the report.
- Complete fingerprint, policy, pid, and role mismatches are incomplete and are not admitted.
- `failed` / `database` / `42501`, a partial stop, and `succeeded` with sqlstate `42501` are rejected and `42501` is not printed. An in-scope failure followed only by an out-of-scope clean success does not admit. A later in-scope clean success after an in-scope failure admits the later pair.
- `truncated: true` on a clean success admits and discloses `querytruncated` with `comprehended` `not_judged`. The same id with `truncated` false and then true is `rejected`, with empty `admitted` and empty `disclosures`.
- A missing complete line is incomplete. Two fingerprints keep answer citation order and receipt admission order. Two identical receipts warn `duplicate` and admit ordinal 0.
- A flipped receipt hash is exit 4 and stderr is exactly `answer-citations: hash\n` with empty stdout. A duplicate version key, a password field, and an empty stdin are exit 3. `trust` other than `local_operator_declared`, and a wrong protocol, are exit 4. An injected `query_id`, a `session` field, a duplicate record key, and sqlstate `4250` or `42p01` are exit 5 with empty stdout. Thirty-three citations are exit 3 `budget`. `--nope` and an extra `--help` argument are exit 2.
- `SECRET_a273_ANSWER`, `SECRET_a273_CELL`, and `SECRET_a273_PASSWORD` are absent from the sealed stdout and stderr.

No product defect was opened by these cases. The mismatched pair does not become an admission. The secret values do not appear in the report.

## Lock and archive

`linux/smoke/lock-subset.txt` and both proof copies say `registry=11 path=1 missing=0 mismatch=0`, with empty stderr. The path package allow-list is only `answer-citations`. No lock file was edited.

Smoke `overlay-absent.txt` is `harness_absent`. Both proofs record `only_in=1 differ=0` and `extra=1`, and the extra path is `a273_review.rs`.

## Preserved harness failure

`linux/proof` test exit is 0 and clippy exit is 101. The six Clippy errors are all in `tests/a273_review.rs`: one `collapsible_if` on the `CARGO_BIN_EXE_answer_citations` lookup, and five `cloned_ref_to_slice_refs` on `&[body.clone()]`. The log line is `could not compile answer-citations (test "a273_review") due to 6 previous errors`. Product sources are not named in those errors. `linux/proof/expect-review.rs` is that harness, SHA-256 `5555fc620e690475c593da32ac580fc4f96eb1ff7fdbbb36ab859ceb0023e0f3`.

The overlay used by attempt 2 replaces that lookup with `if let Some(path) = std::env::var_os(...).filter(|path| !path.is_empty())` and replaces the five clones with `std::slice::from_ref`. Its SHA-256 is `954bd51d76fc750d659b9acedf24d3627162ee0410f31811755fb08bf1d18b6d`, matching `linux/proof-attempt-2/expect-review.rs`. Attempt 2 clippy exit is 0 and test exit is 0. No product patch.

## Not claimed

Entailment, warning-prose comprehension, authenticated audit, parent acceptance of RT-D02, and release are outside this result. An uppercase fingerprint inside a receipt body was not executed. Docs/13 §6.4 numeric entailment was not run as a campaign.
