# Review 2 — CLI fixture

Scope: compiled producer to `reviewed-policy` for quantity, including PII allow rules. No builder for this note.

## Checks

Frozen `reviewed_policy_quantity` exited 0 with 6 passed. The independent fixture is a different project, table, digest, and generation: `qtylot`, `public.lot`, digest `fedcba9876543210` repeated to 64 hex digits, generation 13.

`classify/policy.stdout` is one policy. Generation is 13. Role `clerk`, login `lot_login`, `row_filter` null. Columns:

| Column | Status | Semantic | PII |
|---|---|---|---|
| `amount_i2` | confirmed | quantity | none |
| `amount_i4` | confirmed | quantity | none |
| `amount_i8` | confirmed | quantity | declared, explicit_allow true |
| `batch_i8` | confirmed | identifier | none |

Hand decimals in the harness are the signed extremes and zero for int16, int32, and int64. The harness stops if those strings do not parse to the bounds. `classify=ok` means that check passed.

A second `reviewed-policy` invocation matched the first stdout. `compile_column_acl_set` accepted the set. The leak marker is absent from that stdout.

## Verdict

APPROVE_LIMITED. The CLI keeps quantity and the current PII binding on the integer columns. Generation 13 is the binding this fixture supplied. It is not the live mapping generation and not the target fence.
