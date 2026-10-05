# Findings

Verdict: **APPROVE LIMITED**.

Fixed source `613aa981dc2c57c0005cc6589e1a8ee7757f2a69`. The live oracle is `linux/overlay/live/facts.txt`. The catalog oracle is `linux/overlay/live/catalog-facts.txt`. Raw exits are the `*.exit` files under `linux/preoverlay/` and `linux/overlay/`, including `linux/overlay/attempt-1/`.

## F1. Attempt 1 failed in the catalog harness marker

Container `fc7a35f9c635e33595c654ce4692c1b590a4e65def4f4f2b548d8920ea168262`, `docker-run.exit` 101, `live.exit` 0, `catalog.exit` 101. `OOMKilled=false`.

`mapped_shadow_review_on_one_postgres` passed. `policy_catalog_reads_typed_gold` loaded the catalog and matched the three gold cells, then `finish` panicked:

```
catalog review failures:
pipeline_chain=true
failure_count=1
```

`attempt-1/live/catalog-facts.txt` has `pipeline_nosleep_kind_OpenTransaction/Reusable=false` and `gold_match=true`. The pipeline facts record two lines, `nosleep_kind=OpenTransaction` and `nosleep_disposition=Reusable`. The harness looked for one combined line, `nosleep_kind=OpenTransaction/Reusable`. The product file was not edited. The marker was changed to the lines the pipeline test writes, and the role reset revokes `oo_r_clerk` from `oo_a230_login` when both roles exist. Attempt 2 is the green overlay.

## F2. An immediate BEGIN is refused and the caller transaction stays open

`closed()` reports `ConnectionDisposition::Reusable` for `OpenTransaction` and does not roll back a transaction the call did not open.

The review opened a side session, sent `BEGIN` with no `pg_sleep`, and called `analyze_mapped_shadows` at once. `nosleep_elapsed_ms=0`. Kind `OpenTransaction`. Disposition `Reusable`. `nosleep_caller_transaction_kept=true`. Mapped `reltuples` stayed the -1 bits. After the caller sent `ROLLBACK`, the raw simple query was false. The later explicit analyze is the first transition to a known estimate (`explicit_was_first_known=true`).

That is the contract behavior for a refusal before the call's own `BEGIN`. It is not a product defect.

## F3. Statement timeout was not provoked

`statement_timeout_provoked=false`. SQLSTATE `57014` remains the classified `StatementTimeout` path. This review did not add a statement-timeout campaign. The live timeout case is the 400 ms lock timeout, `55P03`, `LockTimeout` / `Reusable`, with the estimate left unchanged.

## F4. The rounded estimate is not an exact-count claim

After `ANALYZE`, `reltuples_after_bits=1077936128` and the receipt field `estimated_rows` was 3. `estimate_equals_gold=true` is recorded with the failure check left off, so a different finite estimate would still pass that line. `estimate_is_exact=false` and `published=false` are required. The sentinel and the source table stayed at -1. The catalog records `estimate_treated_as_exact=false` and `catalog_estimated_rows=3` without requiring the estimate to equal 3. The typed gold match is the cell values returned by the guarded select.

## F5. The catalog proof is evidence, and Confirmed is a grant input

`target_statistics_a230_catalog.rs` is copied into the throwaway archive only. `oo-gateway` was not changed. `ClassStatus::Confirmed`, semantic type `text`, and `PiiClass::None` were passed to the existing `install_column_acl`. The facts do not contain a classification result. Import remained `awaiting_publish`. `statistics_published=false`.
