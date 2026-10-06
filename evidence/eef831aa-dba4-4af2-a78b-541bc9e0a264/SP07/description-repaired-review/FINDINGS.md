# Findings

## Verdicts

S5-D01: **APPROVE_LIMITED** for `567d621e2e52b7e0011bdb025dc9337e92ecc5a6`.

S5-D02: **APPROVE_LIMITED** for the same commit.

`linux/proof-2` is the decisive run. `linux/proof` is kept because its oracle failed. Neither verdict accepts full S5 or the provider.

## Fixed bytes

`56b791368d7b5ec050aa36ac91d33bc2234bc4d2` is the producer in the fixed tree. Blob `f783ae95f1f7398da18b6c47bcc87941db49f7a3`. Content SHA-256 `c392dd9ef7dfff45ea9f6298b257e8fc4bdeca04784acbfbb696ef644f884954`, the a262 patched producer.

`827e768b9f94af6134a5734bd16e066ab0d47528` is the CLI in the fixed tree. Blob `273de1e7cba3835150766545ba320320f56e65eb`. Content SHA-256 `c28297d86349507fff5bc67e5aa796a0f58836b8974cc9b1d7f08ade7d31798d`. The same blob is at `56b79136`, so the sort commit did not edit the CLI. `lib.rs`, `Cargo.lock`, the CLI test, and the CLI contract likewise match both commits. `oo-builder/Cargo.toml` matches `56b79136`.

## S5-D01

Contract line 81 orders cards by `(schema, name)`. Line 150 defines `selected_datasets` as the admitted visible datasets, sorted. Line 154 orders `candidates` by `dataset`. Line 149 allows `request_sha256` to be null when no request was issued.

Physical `A` becomes ident `q_41` and dataset `public.q_41`. Physical `b` stays `public.b`. Card order puts `A` first. Dataset order puts `public.b` first. The caller listed `b` then `A`. Those three orders disagree, which is the case a257 used.

On proof-2, every outcome's `selected_datasets` is `public.b` then `public.q_41`: early table budget, request-byte budget, success, truncated, incomplete, provider failure, duplicate-key format rejection, and response-byte budget. The full request and the tight request list cards `A` / `public.q_41` then `b` / `public.b`. Success candidates are `public.b` then `public.q_41`, with the supplied labels, `effect=review_only`, and confidence `0` then `1`. Empty-candidate outcomes keep the sealed structural draft and the compiler statements. Provider failure and the truncated and incomplete statuses drop the body. The response-byte body is absent. The duplicate-key sentinel is absent.

Early table budget and request-byte budget return `request_sha256` null. Response-byte `request_sha256` equals the prepare hash of the same `max_response_bytes=1` command and differs from the full-budget request hash `5801d0ba79262f00c00902f8d92ac91f3de67959647a7145ac45dd92bfbc52b4`. The outcome hash observed on the failing run was `4d0ad807d8869cc6cd93f70424510780e26453514aaf8ac07e2b6e8a0f640bb1`.

The author shop tests still use names whose idents equal the physical names. They passed (10). The a262 selection-order test also passed (1). This review's fixture is a separate sealed catalog, with its own source id, owner, and secrets, and it passed on proof-2.

## S5-D02

The proof-2 test spawns `CARGO_BIN_EXE_description-candidate`. The filename is `description-candidate`. For each of the ten library documents, stdout equals the library bytes, stderr is empty, and the exit is 0. No newline is added.

Closed boundaries in that test:

- no arguments: exit 2, the six usage lines
- unknown phase, `prepare_description_request`, and `complete_description_candidates`: exit 2, `description-candidate: unknown phase\n`, and the rejected text is not printed
- an extra argument on either phase: exit 2, `description-candidate: the phase takes no arguments\n`
- 1048577-byte stdin: `oo.builder.description-request-command/v1/json.budget.bytes`, `document exceeds the byte budget`
- bytes `ff fe`: `json.syntax`, `stdin is not UTF-8`
- a lone `{`: `json.syntax`, `description command JSON is not valid`
- a duplicate key: `json.duplicate_key`, `duplicate JSON object key`
- prepare with `captured_response`: `field.unknown`, `prepare does not accept a captured response`
- complete without `captured_response`: `field.missing`, `complete requires a captured response`
- a table name absent from the sealed draft: `oo.builder.description-candidate/v1/selection.unknown`, `selected table is not in the sealed captured draft`

Stdout is empty on those exits. The owner and the planted argv, stdin, duplicate-key, prepare, and response secrets are absent from stderr. The existing CLI suite, 5 tests, passed on smoke, proof, and proof-2.

## a257 still stands

a257 FINDINGS F1 on `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a` is a product failure of the unfixed tree. Success and the other ready-copied outcomes emitted `["public.q_41","public.b"]`. The early table budget already emitted `["public.b","public.q_41"]`. This review did not edit that evidence and did not rerun it. A green proof-2 shows the `56b79136` sort is present in `567d621e` and holds for an independent `A`/`b` catalog. It does not make a257 a false positive.

## Preserved oracle failure

`linux/proof` `test-a264.exit` is 101. The assertion compared a tight-budget outcome hash to the full-budget request hash. Sort, draft, statement, and outcome checks had already run for the earlier documents. The other focused tests and Clippy in that container exited 0. The product sources compared equal to the fixed blobs. This is a review mistake. It is not a regression of the repaired library.

## Limits, not findings

Column-level candidates, provider transport, residency, cost, a model call, and PostgreSQL are outside this proof. a263 is the provider task and was not reviewed. The shop fixtures still do not split ident from physical name. Request-byte and early-table `request_sha256` stay null, which matches contract line 149 and the a257 record of `budget_document`. Smoke offline fetch exited 101 on a cold private volume; the following locked fetch exited 0. No manifest or lock file was edited.
