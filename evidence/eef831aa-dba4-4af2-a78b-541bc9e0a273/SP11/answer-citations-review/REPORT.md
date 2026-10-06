# a273 answer-citations review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a273`. Reviewed commit `7e404dd3d36cdeecb553ab0c707dbfe82ba94c62`. Fixed archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`, tree `7e83a381c8a90c5f61f3ce73947eb7ac441381e9`, 47949 files. The product tree `tools/answer-citations` is `f3b20319347cdc119dbc97d832974f5dcedb4362` at the reviewed commit, at the fixed archive, and at workspace HEAD when this report was written. Builds used `git archive` of the fixed commit. They did not compile the moving worktree.

`parent_acceptance` stays false. This review does not mark RT-D02, SP11-W04A, or docs/08 section 7 done. It does not judge factual or numeric entailment. `local_operator_declared` stays an operator binding. `authenticated_audit` is false. `entailment` and `warning_comprehension` stay `not_judged`.

## Owned path

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a273/SP11/answer-citations-review/**`

No product write. No root `Cargo.toml` or root `Cargo.lock` edit. a207 remains the connector manifest and root-lock writer. No dependency edit. No PostgreSQL. The pinned Postgres image was not started.

## What the review ran

Smoke, before any harness overlay, checked the archive tool, the desk receipt hash, the producer field set, and the child lock as a root-lock subset. It ran no `cargo test`, no `cargo fetch`, and no Clippy.

Proof built a second archive and added only `tools/answer-citations/tests/a273_review.rs`. That harness drives the compiled `answer-citations` binary. Library use is limited to `sha256_hex` for the fixture digest. The cases are the desk receipt, a fresh in-scope admit, role and project crosses, an uppercase citation id, fingerprint lengths 5, 6, 40, and 41, admit/complete identity mismatches, refused and partial completions, a sqlstate on a succeeded line, an in-scope failure followed by an out-of-scope success, truncation admit and truncation conflict, a missing complete line, two queries, a repeated fingerprint, strict JSON and SQLSTATE shape, budget, usage, and secret-bearing request fields.

## Contract surface that the runs exercised

The caller supplies `session_id`, `turn_id`, `role`, `project`, `protocol`, the receipt bytes, and the SHA-256 of those bytes. `trust` must be `local_operator_declared`. Protocol must be `oo.gateway.query-pg-audit/v1`.

The producer audit lines have no `query_id`, session, turn, or principal. `linux/smoke/audit-fields.txt` records the admit fields and the complete fields from `crates/oo-gateway/src/bin/query-pg.rs`, and records `summary_turn=stdout_only`. The stdout summary line's `turn` is a process index. It is not an audit session turn and it is not a citation input.

A citation id is `q_` plus a 6 to 40 character lowercase fingerprint. The desk fixture, length 1041, SHA-256 `8efbfb0748ad65073a2204909d8b79310c7006161e924893d11b15399d624783`, admits `q_836041cccc8287ad` for role `oo_r_desk` and project `tickets`. The fresh fingerprint `a273a273a273` admits `q_a273a273a273` for role `oo_r_rev` and project `cite273`.

An identity mismatch between admit and complete stays one incomplete pair and is not admitted. An out-of-scope clean success does not admit over an in-scope refusal. A later in-scope clean success after an in-scope failure admits the later pair. Truncation disagreement among in-scope clean successes is `rejected` and is not admitted. A truncated clean success still admits, with `querytruncated` and `comprehended` `not_judged`. Stderr tokens observed include `citations`, `hash`, `request`, `binding`, `receipt`, `budget`, and `usage`. The password field, the answer canary, and the cell canary are absent from stdout and stderr in the sealed cases.

Child lock registry triples are a root-lock subset: `registry=11 path=1 missing=0 mismatch=0` on smoke and on both proofs. `cargo-update-invoked.txt` is `not-invoked`.

## Toolchain

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. Recorded compiler `rustc 1.94.0 (4a4ef493e 2026-03-02)` and `cargo 1.94.0 (85eff7c80 2026-01-15)`. `RUSTUP_TOOLCHAIN=1.94.0`.

Each builder was 6442450944 bytes, `NanoCpus` 1000000000, `CapDrop` `["ALL"]`, `Privileged` false, `OOMKilled` false. `/tmp` was `rw,nosuid,nodev,noexec`. `HOME` was `/tmp`. Cargo home and the target directory were the task volumes, because that `/tmp` cannot exec a `rustc` output. One builder at a time. Host cargo count was 0 at each launch. PostgreSQL count is 0.

Recipe origins and the small script deltas are in `origin/recipes.txt`.

## Runs

| Run | Top exit | Docker exit | Test | Clippy | Container |
|---|---|---|---|---|---|
| `linux/smoke` | 0 | 0 | not run | not run | `3f39b27c03c815575d9af18afa1b4efc4bd46248c351992395280e10b7115ec9` |
| `linux/proof` | 101 | 101 | 0 | 101 | `9805569470f6bddc997021d6ea361a153c29995f012b7e206892643503f50c39` |
| `linux/proof-attempt-2` | 0 | 0 | 0 | 0 | `914414b99623b2aba6fba2913056e519d52ac72fc8426d343a0d7f0e644bf392` |

Smoke network mode is `none`. Proof network is `oo-a273-eef831aa-net` (`bridge`). Fetch and the Clippy component use that bridge. Test and Clippy use `--locked --offline`. Fetch stdout on attempt 2 is empty because the crate cache was filled by the first proof. This is not an air-gap claim.

Smoke repo HEAD was `eb9fdf46313d8a1d3cc4e25eb1fa09be28e3842e`. Proof attempt 2 repo HEAD was `8a6395aef5b1d3b16d1b3d72833d2df2ebaa536e`. Both archives are the fixed commit. Overlay count on both proofs is `only_in=1 differ=0` and `extra=1`.

Attempt 2 test stdout: `a273_review` 9 passed, `tests/api.rs` 9 passed, `tests/cli.rs` 1 passed. Clippy `--all-targets -- -D warnings` finished `answer-citations` in 0.30s with exit 0.

## Limits

The command does not open a database and does not score whether answer numbers follow from query rows. The absence of the desk answer text in a report is redaction, not a numeric-entailment result. The harness did not place an uppercase fingerprint inside a receipt body. Producer fingerprint acceptance and the citation lowercase rule were read from source. That case was not a failing run. Diagnostics in the sealed cases do not include the answer canary, the cell canary, the password, SQL text, or the SQL hashes.
