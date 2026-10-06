# Handoff

Verdict: REQUEST_CHANGES for frozen S5-D01 at `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a`.

The next edit is the producer, not a new unit. `selected_datasets` on the success path must be the admitted dataset strings in sorted order. The early budget outcome already sorts that way. Cards stay ordered by `(schema, name)`. Candidates stay ordered by `dataset`. Do not treat the author shop fixture as coverage for this case: its idents match the physical names.

Safety behavior that passed on `linux/proof-2` should stay: sealed captured catalog, explicit visible selection, hashed comments only, omitted unselected foreign keys, no samples, confidence `review_only`, and empty candidates when the captured response is truncated, incomplete, a provider failure, over budget, or the wrong shape. No model call and no PostgreSQL were part of this proof.

Do not start a column-level, transport, residency, cost, or CLI child from this review. Those need their own frozen contract before code.

Evidence lives only under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a257/SP07/description-candidate-review/`. `linux/smoke` is the pre-overlay MSRV smoke. `linux/proof` is the preserved harness compile failure. `linux/proof-2` is the contract failure. `SHA256SUMS` is the inventory rewrite. This handoff does not accept the parent goal and does not mark S5-D01 done.
