# Findings

## Reviewed cell

Frozen `e447249ab1a7f4c82df4729090b1285c9ef4a4ec` already admits `SemanticType::Quantity` on physical `int2`, `int4`, and `int8` in `compatible()`. Decision checks still run before that arm. Bool stays `boolean_flag`. Text does not gain quantity. Money stays out of the integer arm.

This review did not change that source. The archive integer arm in `linux/run/integer-arm.txt` contains `SemanticType::Quantity` and does not contain `SemanticType::Money`.

## What this run showed

Frozen tests on the archive, before the overlay: `reviewed_policy` 21 passed, `reviewed_policy_cli` 6 passed, `reviewed_policy_quantity` 6 passed. Product Clippy `-D warnings` exited 0. `Cargo.lock` sha256 stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

The independent harness uses project `qtylot`, generation 13, and a repeated `fedcba9876543210` digest. Confirmed quantity remains quantity on int2, int4, and int8. Pii none stays none. Personal quantity requires `explicit_allow` true and is emitted as declared. Missing, stale schema, and stale fact exit 10. A PII allow mismatch exits 12. Wrong physical types and the non-quantity semantics listed in the contract exit 11, except money, which fails in `reviewed-effective-oos` before the policy CLI.

The live path is one int8 table. Import source generation 48, mapping `run_generation` 48, adopted target fence 1. Policy generation is 48. Analyze source generation is 48. Query returned the int8 extremes and zero unchanged.

## Preserved review-script failures

These are harness or launch omissions. The product archive was not edited between them.

- `linux/run-attempt-1`, exit 101. Frozen `reviewed_policy` panicked on missing `OO_A227_OO_BUILDER`.
- `linux/run-attempt-2`, exit 101. Smoke had passed. The harness did not compile on rustc 1.94.0.
- `linux/run-attempt-3`, exit 1. Smoke, harness build, lock diff, and harness Clippy had passed. The leak marker was placed in the catalog owner field, so `captured-pg-draft` echoed it.

## Still open

Money plus unit. Ratio, event time, date, audit time, geo, json, and system. Float, numeric, and date codecs. Quantity on text or bool. Stamping the target fence into policy generation. Authentication, publication, and the audit WAL. Live int2 and int4 queries were not part of this bounded chain.
