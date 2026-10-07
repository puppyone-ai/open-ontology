# Handoff

Actor a328. Session `eef831aa-dba4-4af2-a78b-541bc9e0a328`. Baseline `8c2005d5d3cc48eecfb314aa278f7b1d4f00bef8`. `parent_acceptance` is false. Do not treat this unit as SP07 acceptance, full-plan completion, or a release.

## Owned commit

Commit only:

- `tools/pg-builder/**`
- `testing/real-db/pg-builder/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/**`

No push. No root manifest or lock edit. The nested `Cargo.lock` files belong to this unit. The workspace `Cargo.lock` moved under other actors after the proofs; leave it.

## Read first

- `tools/pg-builder/CONTRACT.md` protocol `oo.pg-builder/v1`
- `REPORT.md` and `FINDINGS.md` in this directory
- `linux/focused-4/` for the green 1.94 test and clippy proof
- `linux/catalog/` for the real catalog chain
- `live/llm/summary.txt` and `live/llm/endpoint-observation.txt` for the model chain
- `live/llm-attempt1-input-read/ATTEMPT.txt` for the relative-path failure
- `pg/cleanup.txt` for removed resources

## Current behavior

`pg-builder generate` against `http://127.0.0.1:8317/v1` and model `grok-4.7` writes a structural draft when the exchange exceeds `deadline_ms` (maximum 30000). It does not store that draft. A second process therefore misses and opens a socket again. Review and pack refuse any generation whose `outcome` is not `validated`.

A prepared-request-sized probe received its first byte at 97280 ms. Raising the product deadline, rewriting the model body, or storing a timeout as a validated candidate would break the frozen contract.

## Reviewer notes

- Caller review is a declared local operator. `authenticated` is false. Parent auth stays required.
- Pass 2 changes only the untrusted party comment. Its evidence hash differs from pass 1. The live call timed out before any model text could be checked for injection compliance.
- `national_id` is omitted from the operator classification and from the narrow role. The hidden-role pack was not reached.
- Field candidates, when a future response validates, stay out of the role pack until the description compiler admits column objects.
- The 6-byte local token is legal only for `loopback_http_local_dev`. Do not point the production description provider at it.

## Cleanup already done

`oo-a328-pg`, its data volume, the cargo and target volumes, the network `oo-a328-eef831aa-net`, and `/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328` are gone. The customer token file remains at its private path. No foreign containers were removed.

## Next independent review

Review the frozen contract and these three trees. A live validated candidate still needs an endpoint that finishes inside 30000 ms for this card set. That condition was not met in this session.
