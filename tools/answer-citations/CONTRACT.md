# answer-citations command

`answer-citations` checks bracket citations in one answer against ordered local `query-pg` audit receipts. It reads one JSON document from stdin. It does not open a database, does not read SQL, and does not decide whether the answer's numbers follow from a query result.

The producer is `query-pg` as specified by `contracts/gateway/query-pg-cli-v1.md`. Its audit file is `oo.gateway.query-pg-audit/v1` JSONL. Each admitted statement is one `admit` line, then one `complete` line. Those lines carry `protocol`, `role`, `project`, `policy_id`, both SHA-256 hashes, `fingerprint_hex`, `backend_pid`, and `backend_start`. The completion line also carries `status`, `truncated`, `returned_rows`, `result_bytes`, `rollback_confirmed`, `socket_dropped`, `stop`, and `sqlstate`.

The producer file has no `query_id`, no session, no turn, no principal, and no authentication field. This command does not add those fields to a receipt and does not treat the caller binding as an authenticated audit. Trust is only `local_operator_declared`: the caller asserts which session and turn the receipts belong to, and asserts the exact SHA-256 of each receipt body.

`docs/08-agent-interface.md` section 7 requires each cited `query_id` to occur in that session's Gateway audit. `docs/13-evaluation-and-benchmarks.md` section 6.4 also compares cited numbers with retained rows. This command checks only the first existence rule, and only against the local `query-pg` receipt the caller supplied. It does not judge entailment. It does not read warning prose.

## Usage

```text
answer-citations
answer-citations --help
```

No arguments reads one request from stdin. `--help` alone prints the operator summary and exits 0. Any other argument exits 2. Stderr is `answer-citations: ` plus one static token. The token is never the answer, the receipt, SQL, a cell, a credential, or a path.

## Request

Stdin is at most 1_048_576 bytes. The document is UTF-8 JSON. Duplicate object keys, trailing commas, trailing bytes, and nesting deeper than 8 are refused. More than 4_096 JSON nodes is a budget failure. `serde` rejects unknown fields. JSON null is refused in the request. `serde_json` is built with `arbitrary_precision`.

`version` is `oo.answer-citations/v1`.

`trust` is `local_operator_declared`. Any other string is a binding failure.

`binding.session_id` and `binding.turn_id` are caller labels. Each is 1 to 64 bytes. The first byte is an ASCII letter or digit. The rest are ASCII letters, digits, `_`, or `-`. They are not read from the receipt.

`binding.role` is the producer role: `oo_r_` plus a slug of 1 to 32 bytes. The slug starts with an ASCII lowercase letter or `_`. The rest are lowercase letters, digits, or `_`.

`binding.project` is 1 to 63 bytes, with the same slug grammar.

`binding.protocol` is `oo.gateway.query-pg-audit/v1`. That is the producer version field. A different protocol is a binding failure, not a citation class.

`receipts` has 0 to 8 objects, in caller order. Each object is `receipt_sha256` and `body`. `receipt_sha256` is 64 lowercase hex digits and is the SHA-256 of the UTF-8 bytes of `body` after JSON string decoding. A mismatch is a hash failure and produces no citation report. `body` is 1 to 32_768 bytes.

`answer.declares_data` is a JSON boolean. `true` means the caller says the answer carries query data, so at least one admitted citation is required. `false` allows an answer with no citations. `answer.text` is 0 to 65_536 UTF-8 bytes. The text is not copied to the report.

There is no `principal` field and no field that marks the binding as authenticated.

## Receipt grammar

A body is JSONL. Lines are split on `\n`. One trailing empty line is allowed. Any other empty line is refused. A line is at most 4_096 bytes and contains no `\r`. Each line is one JSON object with no duplicate keys. A receipt has at most 8 pairs.

Lines must be `admit`, `complete`, repeating. A final `admit` with no `complete` is one incomplete pair. Any other sequence is a receipt failure.

An `admit` object has exactly these fields: `protocol`, `record` (`admit`), `original_sha256`, `executed_sha256`, `fingerprint_hex`, `role`, `project`, `policy_id`, `backend_pid`, `backend_start`.

A `complete` object has those identity fields plus `record` (`complete`), `status`, `truncated`, `returned_rows`, `result_bytes`, `rollback_confirmed`, `socket_dropped`, `stop`, and `sqlstate`.

`protocol` is `oo.gateway.query-pg-audit/v1`. `original_sha256` and `executed_sha256` are 64 lowercase hex digits. `fingerprint_hex` is 1 to 128 lowercase hex digits. `policy_id` is 64 lowercase hex digits. `role` and `project` use the binding grammar. `backend_pid` is an integer from 1 through 2147483647. `backend_start` is 1 to 64 ASCII digits. `status` is `succeeded`, `partial`, or `failed`. `truncated`, `rollback_confirmed`, and `socket_dropped` are booleans. `returned_rows` fits `u32`. `result_bytes` fits `u64`. `stop` is null or one of `timeout`, `canceled`, `type_refused`, `database`, `protocol`. `sqlstate` is null or five ASCII uppercase letters or digits.

The `complete` line must repeat the `admit` line's protocol, hashes, fingerprint, role, project, policy id, backend pid, and backend start. A mismatch is an incomplete pair, not two queries.

A pair is a clean success only when `status` is `succeeded`, `rollback_confirmed` is true, `socket_dropped` is false, `stop` is null, and `sqlstate` is null. `truncated` may be true. Any other completed pair is rejected. A clean success whose fingerprint length is outside 6 to 40 cannot form a citation id and is incomplete.

## Query ids

A citation id is derived only from a pair's `fingerprint_hex`. When that fingerprint is 6 to 40 lowercase hex digits, the id is `q_` joined with the fingerprint. The fixture fingerprint `836041cccc8287ad` is the id `q_836041cccc8287ad`. This is the bounded ASCII form used by `docs/08-agent-interface.md` section 7 (`q_` plus 6 to 40 ASCII alphanumerics). It is not a Gateway ULID and not an authenticated query id.

The answer scan is byte-wise and does not use a regex crate. A citation is `[`, then `q_`, then 6 to 40 ASCII letters or digits, then `]`. Anything else, including a short id or an id that contains `_` after the prefix, is not a citation. The same id is kept once, at its first closing bracket. At most 32 distinct citations are accepted.

## Admission

Pairs are read in receipt order, then line order. `pair_ordinal` starts at 0 across the whole request. A query id is admitted when at least one in-scope clean success has that fingerprint. Scope means the pair's `role` and `project` equal the binding. If any later pair derives the same id, the report includes one `duplicate` warning for that id and does not remove the first admission. If in-scope clean successes for the same id disagree on `truncated`, the id is not admitted and the citation class is `rejected`.

Only an admitted id satisfies a citation. Classes and warning codes:

| Code | When |
| --- | --- |
| `admitted` | The citation's id has an in-scope clean success. This class is not a warning. |
| `unknown` | No parsed pair derived that id. |
| `incomplete` | The only in-scope pairs for that id are unfinished or cannot form an id. A trailing `admit` is this class. |
| `rejected` | An in-scope pair completed and is not a clean success, or clean successes disagree on truncation. |
| `crossdeclaredscope` | Every pair for that id is outside the binding role or project. |
| `querytruncated` | An admitted id has `truncated` true. The citation stays `admitted`. |
| `duplicate` | A second pair derived an id that an earlier pair already derived. |

`unknown`, `incomplete`, `rejected`, and `crossdeclaredscope` make the report invalid. `querytruncated` and `duplicate` do not, unless the duplicate is a truncation conflict.

If `declares_data` is true and no citation is `admitted`, `missing_citation` is true and the report is invalid. If `declares_data` is false and the answer has no citations, that result is valid even when receipts admit queries. Citations that are present are still checked.

An admitted id with `truncated` true is also a disclosure: `code` `querytruncated`, `required` true, `comprehended` `not_judged`. The command does not decide whether the answer text explained the truncation. Receipts have no warning message field. `stop` and `sqlstate` are status fields, not natural-language warnings.

## Report

A valid or invalid citation check writes one JSON object to stdout and a trailing newline. A request, budget, binding, hash, or receipt failure writes no stdout. The report contains ids, statuses, and the receipt SHA-256 only. It does not contain the answer, the receipt body, SQL, cells, hashes of SQL, the policy id, the backend pid, or credentials.

Field order:

```json
{"version":"oo.answer-citations/v1","trust":"local_operator_declared","authenticated_audit":false,"entailment":"not_judged","warning_comprehension":"not_judged","session_id":"s1","turn_id":"t1","role":"oo_r_desk","project":"tickets","protocol":"oo.gateway.query-pg-audit/v1","citation_status":"ok","declares_data":true,"missing_citation":false,"receipts":[{"ordinal":0,"receipt_sha256":"<64 lowercase hex>","status":"bound"}],"admitted":[{"query_id":"q_836041cccc8287ad","receipt_ordinal":0,"pair_ordinal":0,"status":"succeeded","truncated":false}],"citations":[{"query_id":"q_836041cccc8287ad","ordinal":0,"classification":"admitted"}],"warnings":[],"disclosures":[]}
```

`authenticated_audit` is always false. `entailment` and `warning_comprehension` are always `not_judged`. `citation_status` is `ok` or `invalid`. `receipts[].status` is `bound`. `admitted[].status` is `succeeded`. `warnings[].code` is one of the warning codes above. `disclosures[].comprehended` is `not_judged`.

## Exits

| Code | Token | When |
| --- | --- | --- |
| 0 | | `citation_status` is `ok`. Stderr is empty. Warnings may be present. |
| 1 | `citations` | The report was produced and `citation_status` is `invalid`. |
| 2 | `usage` | The arguments are not the usage above. |
| 3 | `request` | The document, UTF-8, types, duplicate keys, or unknown fields are refused, or stdin could not be read. |
| 3 | `budget` | A size, depth, node, or citation count budget is exceeded. |
| 4 | `binding` | Trust, session, turn, role, project, or protocol is refused. |
| 4 | `hash` | A declared receipt SHA-256 does not match `body`. |
| 5 | `receipt` | A hash-matched body is not a producer audit. |

## Fixture

`tests/fixtures/desk-explicit.audit.jsonl` is the compiled `query-pg` receipt copied from `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a242/SP12/benchmark-tools-review/linux/proof/live/desk-explicit.audit.jsonl`. Its SHA-256 is `8efbfb0748ad65073a2204909d8b79310c7006161e924893d11b15399d624783` and its length is 1041 bytes. The derived id is `q_836041cccc8287ad`, role `oo_r_desk`, project `tickets`. Handcrafted tests change copies of those lines. They are not additional compiled receipts.
