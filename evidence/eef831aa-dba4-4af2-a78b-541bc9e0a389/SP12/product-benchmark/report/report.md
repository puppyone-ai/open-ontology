# Product benchmark report

Loop pass: false. Integration passed: false. Parent acceptance: false.

Freeze unchanged: true. SHA-256 `f500dda158ea5220c190c92adfbbfa4687ccfb644b0a86961e3858c6f14fa268`.

Rows party Some(2000) account Some(8000) entry Some(90000). Match: true.

Model `grok-4.7` HTTP status Some(200) input SHA-256 `8b4c803d7f0676057150097f68271205aea1777970682f9ee846efa9e089ed2b`. Usage Some(2438) + Some(2621) = Some(5059) (sum ok true). Cost status `unknown`, amount null true. Cache first `miss`, hit `hit`, cache socket `not_opened`. Fresh call socket `opened`.

HTTP 200 alone is an end-to-end pass: false.

Review unauthenticated: true. Hidden national_id: true. Wide and narrow roles: true. Query closed: true. Session-close API: notrun.

## Questions

- `q-party-null` pass: cells match the freeze and the summary closed
- `q-qty` pass: cells match the freeze and the summary closed
- `q-fk` pass: cells match the freeze and the summary closed
- `q-phone` pass: cells match the freeze and the summary closed
- `q-narrow-party` pass: cells match the freeze and the summary closed
- `q-party-count` pass: cells match the freeze and the summary closed
- `q-entry-count` fail: model=grok-4.7 policy_id=e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e proposal_exit=Some(3) execution_exit=None receipt= citation_ok=false cells_ok=false
- `q-wrong-model` pass: grok-4.3 was refused, matching the frozen must_fail expectation
- `q-hidden` pass: national_id query exited 6 with empty stdout, matching the frozen must_fail expectation
- `q-citation-false` pass: citation_status invalid matches the frozen must_fail expectation

## Whole product

- `public_auth`: notrun
- `session_close_api`: notrun
- `natural_language_harness`: notrun
- `runsc`: notrun
- `wal`: notrun
- `semantic_publication`: notrun
- `customer_uat`: notrun

## Failures

- q-entry-count is not an end-to-end pass; HTTP status alone cannot pass it
- model HTTP 200 is present and the end-to-end cells are not a pass
