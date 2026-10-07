# Findings

`parent_acceptance` is false. Independent review has not been run. This followup does not accept SP07, the full plan, or a release.

## Proven

- Rust 1.94 focused-6 on the current source: 11 `pg-builder` tests passed, 2 `pg-fixture` tests passed, clippy `-D warnings` exit 0. `docker-run.exit` 0. Builder 6GiB, 1 CPU, pids 256, `oom=false`, `priv=false`. Preserved failures: `attempt-test-101` and `attempt-redirect-102`, both docker 101, same redirect assertion, clippy 0 on the second attempt.
- Focused-5 remains the green proof from before the JSON-string prompt sentence. Its earlier failures stay: mount 125, stopped prompt 143, clippy 101.
- The catalog chain and the 100000-row private load are unchanged. Pass 1 evidence `992617a1ff52ba9509a884e63977d1256bcaae231850bcd170c2f65dd10c9b50`. Pass 2 evidence `2c66f95d485a9df43d08edd0c8f097c7a283ed15b4511735b6482438a764369b`. Schema hash `eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74`. Sample policy `none`. Histogram and most-common values stay withheld.
- Profile `statement_timeout` is still 10 seconds. Catalog `connect_ms`, `statement_ms`, and `lock_ms` are still 30000. The 180000 ms ceiling applies only to a generate call whose origin is `http://127.0.0.1:8317`.
- Pass 1 wrote three validated per-table drafts at HTTP 200, `finish_reason=stop`, model id `grok-4.7`. Aggregate usage 7516 / 12616 / 20132. Declared cost status `unknown`, amount null, invoice false, billing certificate false. `candidate_sha256` `558466ffa534633d9e4aac732a59532b38dd74ff294ae0a28461b57aa7b5d221`.
- A new process cache hit used those drafts. Socket `not_opened`, attempt 0, same `candidate_sha256`. The 1000 ms timeouts share those cache keys and were not stored. Fail-cache file count is 0.
- Pass 2 opened sockets on different `input_sha256` values. Account and entry validated and stored. Party table outcome `format_rejected`, refusal `evidence_identity`, `cache_store=not_stored`, `repair=not_performed`.
- Review of the unavailable draft exited 1 with `review.blocked`. No review file and no pack file. Curated SHA-256 stayed `c7e75f0cdf6dc5c7c8634e1dd9bb34797479e035d6e31330623ecb85b8ad6a23`. `live/llm/success.json` and `live/per-table/timeout.json` stayed at `4106d4da79ec0f45e2faece12c907861bbab3c6dfcedf93ef4d03ac7fc36b816` and `f3d9d8e36fe3ddaed4d62fa33b3785526a5c81ff1a2105867680ec949d2086dd`. `bearer_scan=clean`.

## Withheld

- S9 reviewer decisions, S10 PII and semantic classification, and a two-role compiler pack. a339 has no fixed delivered commit. Column text is staged on the generation document and was not applied.
- A validated pass 2 aggregate. `shop.party` under the untrusted comment fixture failed `evidence_identity`. That response was kept as a structural draft.
- A live miss on a second model id. Config accepts only `grok-4.7`. The live miss is the changed pass 2 comment.
- `shop.entry.amount` numeric oid 1700. That query-executor seam is outside this unit.
- Parent acceptance, SP07 acceptance, and a release.

## Historical proxy metadata

Unauthenticated `GET /v1/models` returned HTTP 401, body length 27, SHA-256 `627b24a9c73f61f6f684ec9f16f7cef785a89b776737c233e098e46f0eafa965`.

The preserved per-table run recorded HTTP 426, body SHA-256 `b250c8bc7e27fa8052c6d62dc7bf47b0c35e744bbfdf3ccebaf41ad71159e2ee`, then HTTP 503, body SHA-256 `77fbab9ee88170e785090b3ece546ae691216eceaf953059455a78fc6ac2d7ec`. Those files were not rewritten.

The later readiness probe in `live/ready-probe.txt` returned HTTP 200 for request model `grok-4.7`. The envelope model was `grok-4.7-build`. That probe is not an S5 candidate.

## Seams

- `description-candidate.dataset_only`. Required extension, owned by a339 and not delivered as a fixed commit: admit column candidates and apply them only through explicit review. Observed a339 proof-5 clippy-review exit 101 and proof-6 clippy-pack exit 101. This unit did not compile that API.
- `description-review.single-request`. One review command carries one request hash, so the three-table aggregate is not a review.
- Numeric oid 1700 for `shop.entry.amount` remains outside this unit.
- Production provider HTTPS and 16-byte bearer rules were not weakened. The 6-byte token is accepted only by `loopback_http_local_dev`.
