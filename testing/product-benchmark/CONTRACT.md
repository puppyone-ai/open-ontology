# SP12 100k real-model product benchmark

Session `eef831aa-dba4-4af2-a78b-541bc9e0a389`. Contract `oo.product-benchmark/v1`.

This file and `FREEZE.json` are frozen before implementation source. The freeze names the questions, the exact gold cells, and the acceptance metrics. A later edit of `FREEZE.json` fails the score. This card does not accept SP12, a parent goal, or whole-product integration.

## Owned paths

- `testing/product-benchmark/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a389/**`

No other product, frontend, or root path is edited. The root manifest and root lock stay untouched. No `cargo update`. No `cargo generate` of the root lock.

## Producers this card calls

| Producer | Pin | Role |
| --- | --- | --- |
| 100k fixture and gold | `f4381843c7f376f196bc30c78c18d9701dc29a6d`, tree `79e2ddc120b2117f90d3e79ff61571490243f23b` | Source rows and exact cells |
| Product archive for query-pg and onboarding | `6f87942f1c66ba1a9787b7d7194e58a507af63ef` | The a349-approved chain |
| pg-builder | `be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba`, tree `10b5375dc0356116322ce98b363118924e03c591` | Real grok-4.7 request, cost, input SHA-256, cache |
| ontology-dialogue | `e31635b213911c725159a23dbccc8463cc45cdd7` | Propose, then approved `query-pg` execution |
| answer-citations | the dialogue commit's tool | A real invalid citation report |

The bearer file is `/tmp/oo-customer-demo-20261007/llm-token`. Runs do not print it, do not pass it as an argument, and do not copy it into evidence.

## Acceptance, item by item

1. Actual row counts are read from `import-result.json`: party 2000, account 8000, entry 90000, total 100000.
2. Fixture queries `q-party-null`, `q-qty`, `q-fk`, `q-phone`, `q-narrow-party`, and `q-party-count` match the frozen cells. Exit is 0. Summary `end_audit_ok` is true. That true value is the query close record.
3. `q-entry-count` is the model end-to-end question. Pass needs a `grok-4.7` proposal, execution exit 0, `query_receipt` `clean`, citation status `ok`, and the same cells. HTTP 200 by itself is not a pass.
4. The builder's opened call records `model_id` `grok-4.7`, HTTP status, `request_input_sha256`, usage whose prompt plus completion equals total, and `declared_cost`. `status` `unknown` keeps `amount` null. That is not an invoice.
5. The second builder generate is a cache hit: cache `hit` and socket `not_opened`.
6. `q-wrong-model` uses model id `grok-4.3`. Its trial verdict is failure. A zero exit does not make it a pass.
7. `q-hidden` is `SELECT national_id FROM shop.party`. Exit 6 and empty stdout. The trial verdict is failure.
8. `q-citation-false` is a real `answer-citations` report whose `citation_status` is `invalid`. The trial verdict is failure. `authenticated_audit: false` is not that failure.
9. Review stays unauthenticated. `shop.party.national_id` stays hidden. Wide and narrow roles are both present in the onboarding outcome or the builder pack.
10. Whole-product APIs with no runtime record stay `notrun`: public auth, session-close API, natural-language harness, runsc, WAL, semantic publication, customer UAT. `integration_passed` is false. `parent_acceptance` is false.

A missing producer file fails the loop. It is not rewritten into a pass and it is not filled with a stub result.

## Score inputs

The scorer reads only the file names listed in `FREEZE.json` `score_inputs`, plus the freeze file. It writes `report.json` and `report.md`.

## Resources

One Linux arm64 Rust builder, 6 GiB, 1 CPU, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. One private PostgreSQL 17.10, 1 GiB, 1 CPU, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Toolchain text starts with `rustc 1.94.0 `. No privileged mode, host network, published port, Docker socket, host home, shared cargo cache, auth environment, or password on argv.

Private names use owner `a389` and this UUID. The frozen fixture driver still says host `oo-a317-pg` and roles `oo_a317_*`. Those strings stay inside the database network. The container name is `oo-a389-pg`.

## Pre-edit smoke

Before `src/` exists, one builder prints `rustc 1.94.0` on linux/arm64 and records that `testing/product-benchmark/src` is absent and that `FREEZE.json` is present. A different toolchain stops the card.
