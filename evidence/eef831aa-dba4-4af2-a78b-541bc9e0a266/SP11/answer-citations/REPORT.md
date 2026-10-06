# a266 answer citations

Session `eef831aa-dba4-4af2-a78b-541bc9e0a266`. Fixed source `567d621e2e52b7e0011bdb025dc9337e92ecc5a6`. The proof compiled `git archive` of that commit plus an overlay of `tools/answer-citations`. It did not compile the moving worktree. Smoke recorded repo HEAD `47f062a022b27646469172fd307cb1aa81d3961c`. This report does not accept a parent plan item, does not close SP11, and does not release.

## Owned paths

- `tools/answer-citations/**` — standalone workspace, not a root workspace member
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a266/SP11/answer-citations/**`

Root `Cargo.toml` and root `Cargo.lock` were not edited. a207 remains the connector manifest and root-lock writer. No PostgreSQL was started.

## What the tool does

`answer-citations` reads one `oo.answer-citations/v1` document from stdin. The caller supplies `session_id`, `turn_id`, `role`, `project`, `protocol`, each receipt body, and the SHA-256 of that body. `trust` must be `local_operator_declared`. The report sets `authenticated_audit` false and `entailment` and `warning_comprehension` to `not_judged`.

The producer audit is `oo.gateway.query-pg-audit/v1` JSONL. Those lines have no `query_id`, session, turn, principal, or warning text. A citation id is `q_` plus the admit `fingerprint_hex` when that fingerprint is 6 to 40 lowercase hex digits. The compiled fixture fingerprint `836041cccc8287ad` is `q_836041cccc8287ad`. The scan is byte-wise and does not use a regex crate.

Only an in-scope clean success admits a citation. Clean means `succeeded`, `rollback_confirmed`, `socket_dropped` false, `stop` null, and `sqlstate` null. `truncated` may be true. Scope is the pair's role and project against the binding. A protocol mismatch on the binding is exit 4 `binding`. A role or project mismatch on a pair is `crossdeclaredscope`. An identity mismatch between admit and complete is one incomplete pair, using the admit fingerprint. Two in-scope clean successes that disagree on `truncated` are not admitted and the citation class is `rejected`.

`declares_data` true with no admitted citation sets `missing_citation` and `citation_status` `invalid`. `declares_data` false with no citations is valid. The tool does not judge whether the answer's numbers follow from query rows.

Stdout on exit 0 or 1 is one JSON object and a newline. Other failures write no stdout. Stderr is `answer-citations: ` plus one static token. The report carries ids, statuses, and receipt SHA-256 values.

## Contract and fixture

`tools/answer-citations/CONTRACT.md` was written before the implementation. `tests/fixtures/desk-explicit.audit.jsonl` is the compiled query-pg receipt from `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a242/SP12/benchmark-tools-review/linux/proof/live/desk-explicit.audit.jsonl`. Attempt 4 `fixture-cmp.exit` is 0. The Rust test checks length 1041 and SHA-256 `8efbfb0748ad65073a2204909d8b79310c7006161e924893d11b15399d624783`.

The lock is the pg-onboarding serde subset with the package name `answer-citations`. `serde` is `=1.0.229` and `serde_json` is `=1.0.151` with `arbitrary_precision`. `cargo-update-invoked.txt` is `not-invoked` on smoke and on attempt 4.

## Toolchain

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. Attempt 4 recorded `rustc 1.94.0 (4a4ef493e 2026-03-02)` and `cargo 1.94.0 (85eff7c80 2026-01-15)`.

Each builder was 6442450944 bytes, `NanoCpus` 1000000000, `CapDrop` `["ALL"]`, `Privileged` false, network `oo-a266-eef831aa-net`, `OOMKilled` false. `/tmp` was `rw,nosuid,nodev,noexec,size=1g`. `HOME` was `/tmp`. One builder at a time. No host cargo. PostgreSQL image was not started.

Recipe origins are in `origin/recipes.txt`.

## Runs

| Run | Top exit | Docker exit | Container |
|---|---|---|---|
| `linux/smoke` | 0 | 0 | `f0fc9b1394a2af490b1321b164d6bfd635a40bf89bec8d44c4558bb26d82e545` |
| `linux/postedit` | 101 | 101 | `e64d11319d46559f007a34af772412a55fdea6e63351096c019445982cc70c7e` |
| `linux/postedit-attempt-2` | 101 | 101 | preserved in that directory's `hostconfig.txt` |
| `linux/postedit-attempt-3` | 101 | 101 | clippy 0, test 101 |
| `linux/postedit-attempt-4` | 0 | 0 | `23feeb65f8f3a77d2bd0eb0baf508d57113f470c55c087ba1be1885fcc678e44` |

Attempt 4 test stdout: 9 tests in `tests/api.rs` passed, and `help_usage_success_and_citation_failure` passed. Clippy `--all-targets -- -D warnings` exit 0. Fetch exit 0. The fetch command had the bridge network. The crate cache was already filled by attempt 1, and fetch stdout and stderr are empty. This is not an air-gap claim. Test and clippy used `--offline`.

The fixed archive stayed without `tools/answer-citations`. Attempt 4 `overlay-count.txt` is `archive=46111 proof_without_overlay=46111 overlay=14`.

## Limits

The command does not open a database, does not read SQL, and does not score numeric entailment or natural-language warning text. A truncated clean success still admits, with `querytruncated` and `comprehended` `not_judged`. Diagnostics do not include the answer, the receipt body, SQL, cells, or credentials.
