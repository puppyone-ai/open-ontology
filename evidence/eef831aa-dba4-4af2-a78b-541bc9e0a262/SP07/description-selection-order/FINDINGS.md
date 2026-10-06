# Findings

## Repaired: a257 F1

`selected_datasets` on every outcome is the admitted dataset strings, sorted. For physical names `A` and `b`, that order is `public.b` then `public.q_41`.

`build_request` still sorts cards by `(schema, name)` before it builds them. It then sorts the dataset strings it stores on the ready request. Success, truncated, incomplete, provider, format, and response-byte outcomes copy that list. The early table-budget outcome already used `selected_names`, which sorts the same strings. Those two paths now agree.

The contract text was not edited. Candidate order is still by `dataset`. Card order is still `(schema, name)`, so the request lists `public.q_41` (`A`) before `public.b`.

## Red, before the edit

`linux/red` ran `description_selection_order` on fixed `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a` with only the new test copied in. `phases/test-order.exit` is 101. The failure line is:

```
dataset-order mismatches: success=["public.q_41","public.b"] | truncated=["public.q_41","public.b"] | incomplete=["public.q_41","public.b"] | provider_failure=["public.q_41","public.b"] | format=["public.q_41","public.b"] | response-budget=["public.q_41","public.b"]
```

The early table budget was not in that list. Its `selected_datasets` was already `["public.b","public.q_41"]`, with `outcome=budget_failure`, `budget_reason=tables`, and `request_sha256` null. Card order and candidate order had already passed inside the same test. Author tests were 10 passed. Lib `description_candidate` tests were 2 passed. `red.exit` is 0 because that 101 is the expected dataset-order failure.

## Green

`linux/proof` copied the patched producer and the same test. The order test passed. Author tests passed. Lib tests passed. `cargo clippy -D warnings` on `--lib --test description_candidate --test description_selection_order` exited 0. The lock stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

`linux/cli` compiled the unchanged `description-candidate` binary from `827e768b9f94af6134a5734bd16e066ab0d47528` against the patched library. That source is not in the fixed archive. Stdout matched the library document for early budget, the prepared request, success, truncated, incomplete, provider failure, format rejection, and response-byte budget. The success document from the binary has `selected_datasets` and `candidates` both `public.b` then `public.q_41`.

## Limits, not findings

This repair does not accept S5-D01. a257 remains REQUEST_CHANGES until an independent review of `56b791368d7b5ec050aa36ac91d33bc2234bc4d2`. Column-level candidates, provider transport, residency, and cost stay out of scope. No model call and no PostgreSQL were used. Smoke offline fetch exited 101 on the cold volume; the following locked fetch exited 0. That is preserved under `linux/smoke` and is not a product defect.
