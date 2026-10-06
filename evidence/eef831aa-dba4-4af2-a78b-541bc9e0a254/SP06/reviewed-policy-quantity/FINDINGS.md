# Findings

## Cause

a248 stopped because `compatible` accepted `int2`, `int4`, and `int8` only as identifier, foreign key, code, or status code. Confirmed quantity on base `int8` matched `physical_type` (`typtype` `b`, `type_name` `int8`) and then returned `ReviewedPolicyError::Semantic`. The CLI mapped that to exit 11 and `reviewed-policy: semantic`. The producer had already kept `semantic_type` `quantity`. Reclassifying the column or dropping it would have hidden the row the decision confirmed.

## Change

Commit `83b4f94a12c527ff8fd77eadd2909437e63d908c` adds `SemanticType::Quantity` to that integer arm, plus two comments. Decision checks in `admit_one`, `check_decision`, and `check_output` still run before `column_spec`. Blob `56b62fdc1ab9864b99d992ccb46782fc34c1a507` became `95e68417e6fe1be024123e3aa2b7c2301402eb21`.

The contract commit `6c426aa5839fbebaf67b84d07cfd82b8e01391af` is an ancestor of that product commit. The parent files `reviewed-policy-v1.md` and `reviewed-policy-cli-v1.md` were not edited.

## What was shown

Pre-edit smoke on archive `c828e2781fc37eb62249fab54fa0b7fff86f0249` exited 0 after a preserved attempt that false-failed on the `semantic_name` label. The archive integer arm does not contain `Quantity`.

Post-edit focused tests exited 0: existing `reviewed_policy` 21, existing `reviewed_policy_cli` 6, new `reviewed_policy_quantity` 6. Clippy `-D warnings` on the gateway lib, the `reviewed-policy` bin, and the new test exited 0. `Cargo.lock` hash stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

The new test calls the compiled producer and the reviewed-policy CLI. Positive int2, int4, and int8 quantity stay quantity in the DTO, with current PII allow rules. Generation on that fixture is 7. Wrong physical codecs, missing and stale decisions, money without a unit, and the other listed semantics refuse with the existing exit codes.

The private TLS chain exited 0. Source generation 48 and target generation 1 were read from the import result, checked against `_oo.full_import_run`, and checked against the adopted row in `_oo.target_generations`. Policy generation on the clerk DTO is 48. Clerk query amounts came back as int `-9223372036854775808`, `9223372036854775807`, and `0`.

## Boundaries of this proof

The live shop document has one amount column, `shop.order_doc.amount`, type `int8`. The clerk query is the projection that includes it. Peek's order rows are order id, customer id, and the posted flag. Int2 and int4 quantity were proved through the producer and the CLI fixture, which does not query PostgreSQL.

The library fixture's hand decimals are independent of the shop rows. Both sets include the int8 extremes and zero. The shop query also returned those three int8 texts.

Harness non-zero exits listed in `REPORT.md` are the negative proofs the a248 harness already expected. The chain console ends `quantity_chain=ok`.

Preserved failures:

- `linux/smoke-attempt-1`, exit 2, whole-file `Quantity` grep
- `linux/postedit-attempt-1`, exit 101, E0521
- `linux/postedit-attempt-2`, exit 1, clippy component missing; tests had passed
- `linux/postedit-attempt-3`, exit 101, `wrong_self_convention`; tests had passed

## Still open

Money plus unit, ratio, event time, date, audit time, geo, json, system, float and numeric and date codecs, quantity on text or bool, and policy generation taken from the target fence. Authentication, grants beyond this install, publication, the audit WAL, and live row-level security are unchanged open items.

`parent_acceptance` is false. M1-I01 and M1-I02 remain `not_accepted`. This child does not approve the component and does not release.
