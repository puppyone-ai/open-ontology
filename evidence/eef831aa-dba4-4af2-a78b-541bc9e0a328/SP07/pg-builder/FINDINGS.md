# Findings

`parent_acceptance` is false. Independent review of this consumer commit has not been run. a348 `APPROVE_LIMITED` covers producer `c23db28dc27f5cf20741dd36e8a8e88129ad1715` and says builder integration is outside that verdict. This followup does not accept E47, SP07, the full plan, or a release.

## Proven

- Rust 1.94 focused-10 on the current source: 12 `pg-builder` tests passed, 2 `pg-fixture` tests passed, clippy `-D warnings` exit 0. `docker-run.exit` 0. `s9.exit` 0. Builder 6GiB, 1 CPU, pids 256, `oom=false`, `priv=false`. Base `c23db28dc27f5cf20741dd36e8a8e88129ad1715` with the workspace `tools/pg-builder` overlay.
- Focused-7 is the pre-edit smoke of that same producer commit with `overlay=none`. 11 `pg-builder` tests, 2 fixture tests, clippy exit 0, `docker-run.exit` 0.
- Focused-8 stays a failure. `docker-run.exit` 101, `test-builder.exit` 101, `clippy-builder.exit` 101 (`provenance` had 10 arguments). Fixture exit 0. `s9.exit` is `skipped`.
- Focused-9 stays a failure. Clippy exit 0. Fixture exit 0. `test-builder.exit` 101 because two tests read a reused temp cache and saw `transport.calls` left 0 right 1. `s9.exit` is `skipped`. The calls-equal-1 assertions were kept. Focused-10 temp directories include a clock value and those two tests passed.
- The catalog chain and the 100000-row private load are unchanged. Pass 1 evidence `992617a1ff52ba9509a884e63977d1256bcaae231850bcd170c2f65dd10c9b50`. Pass 2 evidence `2c66f95d485a9df43d08edd0c8f097c7a283ed15b4511735b6482438a764369b`. Schema hash `eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74`. Sample policy `none`. Histogram and most-common values stay withheld.
- Profile `statement_timeout` is still 10 seconds. Catalog `connect_ms`, `statement_ms`, and `lock_ms` are still 30000. The 180000 ms ceiling applies only to a generate call whose origin is `http://127.0.0.1:8317`.
- Pass 1 wrote three validated per-table drafts at HTTP 200, `finish_reason=stop`, model id `grok-4.7`. Aggregate usage 7516 / 12616 / 20132. Declared cost status `unknown`, amount null, invoice false, billing certificate false. Aggregate `candidate_sha256` `558466ffa534633d9e4aac732a59532b38dd74ff294ae0a28461b57aa7b5d221`. Per-table candidate hashes: account `fad062220149123ab2f978fa7db4444f7710143a6fd34d0c8a0650662b88edeb`, entry `57c79505b23f4304169e9ff72c2d9410da5f1063371f8913296ef9b442fec9a1`, party `1064ccef6200c2838fee04ef7bcdbbc3e240d926a3768ab5002e1d19163aeb14`.
- A new process cache hit used those drafts. Socket `not_opened`, attempt 0, same aggregate `candidate_sha256`. The 1000 ms timeouts share those cache keys and were not stored.
- Pass 2 opened sockets on different `input_sha256` values. Account and entry validated and stored. Party table outcome `format_rejected`, refusal `evidence_identity`, `cache_store=not_stored`, `repair=not_performed`.
- The S9 bridge rebound those stored pass 1 bodies. `rebind.bound` is true for all three tables. `model_bytes` is `copied`. `stored_generation` is `unchanged`. Stored prompt `oo.builder.description-prompt/v1`. Rebound prompt `oo.builder.description-prompt/v2`. No second socket was opened for this review.
- Caller decisions are explicit. Party and account classified columns are approved and applied. `national_id` is deferred, classification null, and absent from every role file. Every shop.entry column is deferred. Entry `field_candidates_applied` is false and the entry role file is the same length before and after.
- Role files show approved column prose with the compiler prefix `UNREVIEWED:` because `review.state` is null and `live/s9/settings.json` lists `column_descriptions`. The restricted role omits `display_name`, `phone`, and `national_id` in the party pack, the account pack, and the entry pack. `authenticated` is false. `parent_auth` is `required`. `grants` is `not_authorized`. `publication` is `unavailable`. `model_publication` is false. `confidence_effect` is `recorded_only`.
- Review of the timeout aggregate and the unavailable aggregate exited 1 with `review.blocked`. Pack of those documents exited 1. No review file and no pack file were written for them. Protected hashes before and after the S9 run match, including curated `c7e75f0cdf6dc5c7c8634e1dd9bb34797479e035d6e31330623ecb85b8ad6a23`, `live/llm/success.json` `4106d4da79ec0f45e2faece12c907861bbab3c6dfcedf93ef4d03ac7fc36b816`, and `live/per-table/timeout.json` `f3d9d8e36fe3ddaed4d62fa33b3785526a5c81ff1a2105867680ec949d2086dd`.

## Withheld

- A validated pass 2 aggregate. `shop.party` under the untrusted comment fixture failed `evidence_identity`. That response was kept as a structural draft and was not packed.
- A live miss on a second model id. Config accepts only `grok-4.7`. The live miss is the changed pass 2 comment.
- `shop.entry.amount` numeric oid 1700. That query-executor seam is outside this unit and was not retested.
- `review.state` set to approved. The delivered pack records an override review and leaves `review.state` null. The role file therefore uses `UNREVIEWED:`.
- Live caveat text and live open-question text. The stored model arrays are empty. The unit test covers a non-empty caveat and a non-empty open question.
- `lock_timeout` of 1 second and a snapshot budget of 300 seconds. This crate does not set them.
- Parent acceptance, E47 acceptance, SP07 acceptance, and a release.

## Historical proxy metadata

Unauthenticated `GET /v1/models` returned HTTP 401, body length 27, SHA-256 `627b24a9c73f61f6f684ec9f16f7cef785a89b776737c233e098e46f0eafa965`.

The preserved per-table run recorded HTTP 426, body SHA-256 `b250c8bc7e27fa8052c6d62dc7bf47b0c35e744bbfdf3ccebaf41ad71159e2ee`, then HTTP 503, body SHA-256 `77fbab9ee88170e785090b3ece546ae691216eceaf953059455a78fc6ac2d7ec`. Those files were not rewritten.

The later readiness probe in `live/ready-probe.txt` returned HTTP 200 for request model `grok-4.7`. The envelope model was `grok-4.7-build`. That probe is not an S5 candidate.

## Seams

- `description-candidate.column-v2` is consumed at producer `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. a348 records `APPROVE_LIMITED` for the producer. That review does not accept this consumer and does not accept E47.
- `description-review.single-request` is consumed as one review and one pack command per table. Each pack still compiles the whole artifact, so the account pack and the entry pack still list allowlisted party column names from the catalog. Party labels and party descriptions are applied by the party pack.
- The delivered description-pack does not set `review.state`. Column prose is visible in the role file through `include_unreviewed` `column_descriptions`, prefixed `UNREVIEWED:`.
- Numeric oid 1700 for `shop.entry.amount` remains outside this unit.
- Production provider HTTPS and 16-byte bearer rules were not weakened. The 6-byte token is accepted only by `loopback_http_local_dev`.

## Observed and left in place

a350 smoke `docker-run.exit` 0. a350 proof, proof-2, and proof-3 each have `docker-run.exit` 101 and `clippy.exit` 101. No a350 handoff was present. a350 volumes and network were still present after a328 cleanup.

a351 `types.exit` 0, `fonts.exit` 0, `lint.exit` 0, `test.exit` 1. No a351 handoff was present. This unit did not write frontend paths.
