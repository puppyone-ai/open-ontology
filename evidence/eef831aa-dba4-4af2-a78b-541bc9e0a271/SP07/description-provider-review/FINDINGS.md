# Findings

## Verdict

S5-D03: **APPROVE_LIMITED** for provider commit `aba067efe4b19bae868e15e23157ebbcf0a4969f` as stored in fixed archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`.

`linux/smoke-2` and `linux/proof` are the decisive runs. `linux/smoke` is kept because its fetch command was rejected. The verdict does not accept full S5, the parent goal, or a release. a264 APPROVE_LIMITED for S5-D01 and S5-D02 stands. This review does not replace it.

## Fixed bytes

`git diff --stat aba067ef 9951cda7 -- tools/description-provider` is empty. Provider `lib.rs` blob `f4fccebcb31471e028079c22f7faaef3046159e4` matches at both commits. `description_candidate.rs` blob `f783ae95f1f7398da18b6c47bcc87941db49f7a3` matches `56b79136`. Content SHA-256 of that producer is `c392dd9ef7dfff45ea9f6298b257e8fc4bdeca04784acbfbb696ef644f884954`.

The provider lock content hash stayed `d6b28e14bceb66512cc44acbbdce9af84754c860bef199384eb0b1662f9258cb`. The root lock stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Neither file was rewritten.

## Transport against the contract

Contract lines 124 through 137 were read on the outcome, not on a certificate file.

On the valid mock, `transport.status` is `complete` with HTTP 200 and usage 18, while the candidate statements stay `llm_transport=not_used`, `residency=not_approved`, and `cost=not_metered`. Those statements are the compiler document. The performed call is the sibling `transport` object. `billing_certificate` and `residency_certificate` stay false. `caller_residency_approval` is `explicit`. `declared_cost.amount` is the configured price `0.000002` times 18, in integer units (`0.000036`).

On the token cap, `transport.budget_reason` is `tokens` and `candidate.budget_reason` is `caller`. Usage and the declared amount are still present. Certificates stay false. Statements stay `not_metered` and `not_used`.

Redirect is HTTP 302 and refusal `redirect`, with no second hit. A partial close is `incomplete` / `partial`. A non-JSON 200 is `provider_failure` / `malformed_envelope`. A held connection past the deadline is `provider_failure` / `timeout`. A bearer echo on HTTP 200 reports usage and a declared amount, admits no candidate, and keeps both certificates false.

The request body uses the JSON lexeme `0` for temperature. The response format name is `oo_builder_description_response_v1` with `strict` true and protocol const `oo.builder.description-response/v1`. One POST, path `/v1/chat/completions`. Proxy environment and an ambient API-key variable are set. The Authorization header matches the bearer file. The comparison is length-only. The token is absent from argv and from the outcome text.

The provider CLI stdout equals the library outcome. `complete_candidates` on the compiled `description-candidate` equals `candidate`. `OO_A263_DESCRIPTION_CANDIDATE` and `OO_A271_DESCRIPTION_CANDIDATE` both point at that binary for the proof.

## Registry triples

`linux/proof/lock-subset.txt` is `registry=248 path=4 missing=0 mismatch=0`. `linux/proof/registry-vs-direct.txt` records 14 direct exact version pins. The 248 count is every registry package in the provider lock, compared as name, version, source, and checksum to the root lock. Path packages are the four allowed workspace members.

`linux/smoke-2` zeroed the first checksum in a temporary copy. The tool exited non-zero with `mismatch ahash 0.8.12` and `mismatch=1`. The archive lock was not modified.

## Preserved harness failure

`linux/smoke` `fetch.exit` is 1 because the command included `-p`. `cargo fetch` rejected it before any test. Identity and the absent-overlay checks had passed. smoke-2 dropped `-p` and fetched the root manifest the way a264 did. That failure is not a provider defect.

## Limits, not findings

The shop names `public.customer` then `public.purchase` match `selected_datasets` and the prepared request table order on this fixture. That order is also alphabetical. This run does not re-prove the repaired sort against an insertion order that differs from the sorted order. a264 already approved that sort on a separate catalog.

`linux/smoke-2` offline fetch exited 101 with `no matching package named chrono-tz` because the private volume was still cold after the failed smoke. The following locked fetch exited 0. No manifest was edited.

Column-level candidates, a real provider, a production network, an invoice, and a residency certificate are outside this proof. The contract leaves them there. Full S5 is not accepted here. a263's own green run is not this review.
