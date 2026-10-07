# Findings

## What the component does

`tools/metric-reconcile` reads one metric YAML document, an explicit project/role/policy/knowledge tuple, a closed anchor, and enum documents. `:name` outside strings and comments becomes a PostgreSQL bind. Identifiers go through `oo_gateway::guard_select` on a sentinel probe, then the restored `SELECT` is executed with typed binds. The control statement is a second operator SQL text. Whitespace-normalized equality or equal guard fingerprints refuse `same_path` before execute.

Enum class resolution is local. It does not depend on `tools/enum-history`. Requested classes that are missing, reserved, inferred without an approved review, or empty at `as_of` refuse certification. Positive codes are reported and are not written into SQL.

Decimal comparison uses `oo_core::ExactDecimal`. Abs and rel together are an AND. The hand literal and the last-digit miss were decoded from PostgreSQL numeric OID 1700, not from `f64`.

`EXPLAIN (FORMAT TEXT)` runs only on guard-restored SQL, under `SET ROLE` of the checked `oo_r_` role, in its own read-only transaction. `query-pg` is not used. `query_portal` is called with a positive row cap.

## Failures that stayed failed

`linux/overlay` did not compile. The errors were a client borrow across `Transaction::commit`, `read_bytes` given a `String` instead of a `Path`, `fold_status` comparing `&&str`, and `as_of` compared with `&String`. Root lock hash did not change.

`linux/overlay-2` compiled. Two tests failed: a comment `:no` is preserved on purpose, and the unknown-class fixture was not a core-valid enum because the value class was not in the class map, so the refusal was `enum_document`. Clippy `-D warnings` also failed (`result_large_err`, identical `if` bodies, collapsible `if`, manual range, unused variable). Those were fixed in source. The failed logs were not overwritten.

## What overlay-4 showed

The finance example `examples/schema/metrics-finance.yaml` from the fixed archive is refused with reason `guard` before any socket. Its `GROUP BY` and `date_trunc` stay in `bound_sql`. Executed SQL is empty. The unit test covers that.

Compile of the happy metric returns `outcome` compiled, both gates `block` / `not_executed`, and `metric_certified` false.

The live tolerance run keeps G5a passed and sets G5b `failed` / `tolerance`. It does not flip the outcome to certified. Exit of an evaluated report is 0 even when G5b failed. That matches the contract.

Changed scale `1000000000000000000.010` against `1000000000000000000.01` passes with difference `0.000`. The metric value stays the source text `.01`.

Guard deparse qualifies columns (`receipt.amount AS amount`) and still leaves `$n`. The probe sentinels are not in the executed SQL.

`SET ROLE oo_r_finance` inside `SET LOCAL transaction_read_only` succeeded for the test, the explain, and both value queries. The login role is a member of `oo_r_finance` and has `SELECT` only.

## Limits

`analyze` returns `Result<Analysis, Report>` and allows `clippy::result_large_err`. The report is the refusal body and is moved once. Boxing it would not change the JSON.

G1–G4 and G6–G13 are `not_run`. `full_gates`, `publication`, and `owner_authenticated` stay false. A passing `metric_certified` flag is not an SP08 certificate and not a release.

Credential bytes for the fixture were not found in this evidence tree. Config and argv carry paths only. The password file mode is checked by the crate. Connect errors are static tokens.

No new crate was added. No root manifest or root lock edit was made.
