# Findings

Reviewer: this session, sole coder and reviewer. This is not an independent review and it is not parent acceptance.

## Verdict

The frozen child matches `linux/proof-7`. That run is Linux arm64 Rust 1.94, commit `aba067efe4b19bae868e15e23157ebbcf0a4969f`, whose ancestor is the contract commit `56710cc73cfe1663272ecaa503984e1f663428ad`. Focused tests exited 0 (2 library, 23 integration). Clippy `--all-targets -D warnings` exited 0. The provider lock's registry triples are a subset of the root lock (`missing=0 mismatch=0`). The root lock hash did not change.

I read the proof-7 phase exits, `test.log`, `clippy.log`, `lock-subset.txt`, and `lock-hash.txt` after the container exited. I did not start a second warm container.

## What the proof shows

A local TLS server receives one HTTPS POST. The client trusts only the configured CA bundle, sends the bearer from the configured file, sets temperature to the JSON lexeme 0, and asks for the description-response JSON Schema. Redirects are not followed. Proxy environment variables on the process are ignored. A valid assistant message is spliced into the fixed command and `complete_description_candidates` returns `effect=review_only`. The CLI stdout for the same captured document compares equal as JSON.

Wrong CA, redirect, HTTP 4xx, HTTP 5xx, a partial body, a malformed envelope, malformed content, oversize bytes, an extra chunk, a read deadline, and a secret echo in the body or a header produce an outcome with an empty candidate list. The structural draft hashes remain. A secret in the command returns an error before a socket. Usage is reported only when the 200 body had consistent usage, including a token budget failure and a 200 secret echo. Non-200 bodies are not parsed for usage.

## Limits that stay in force

The compiler statements are unchanged: `llm_transport=not_used`, `residency=not_approved`, `cost=not_metered`. A provider `budget_failure` is recorded by that compiler as `budget_reason=caller`. The precise reason stays on `transport.budget_reason`.

`declared_cost` is the configured price times reported tokens. It is not a billing certificate. The residency flag is the caller's explicit boolean. It is not an authenticated policy and it is not a residency certificate.

The shop cards are the existing structural fixture. Comments are hashes. The proof does not contain a customer row or a production provider call. `description_selection_order` passed on the fixed archive during preoverlay. That does not accept the concurrent a262 sort repair as full S5. This child did not edit that repair.

The retained `linux/proof` through `linux/proof-6` directories are compile or clippy failures. proof-5 and proof-6 had already passed the same 25 tests before clippy rejected style lints. Those directories are preserved.

## Not accepted here

Parent acceptance, release, full S5, a live provider, and a certificate of residency or cost are outside this child.
