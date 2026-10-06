# Findings

## F1. Success-path `selected_datasets` is not sorted by dataset

REQUEST_CHANGES.

Contract `contracts/builder/description-candidate-v1.md` line 150 defines outcome field `selected_datasets` as "the admitted visible datasets, sorted". Line 81 orders cards by `(schema, name)`. Line 154 orders `candidates` by `dataset`. Those are different keys.

`selected_names` sorts the dataset strings. The early budget document uses that order. `build_request` sorts the admitted tables by `(schema, name)` and stores that order in `Ready.selected_datasets`. Success, truncated, incomplete, provider, format, and response-byte outcomes copy `Ready.selected_datasets`.

Linux `linux/proof-2/test-a257.log` on commit `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a`:

- caller `max_tables=1` outcome: `["public.b","public.q_41"]`, `outcome=budget_failure`, `budget_reason=tables`, `request_sha256` null, `candidates=[]`, structural draft kept
- success outcome after a complete captured response: `["public.q_41","public.b"]`

Physical name `A` becomes ident `q_41` and dataset `public.q_41`. Physical name `b` stays dataset `public.b`. `(schema, name)` puts `A` before `b`. Dataset sort puts `public.b` before `public.q_41`.

The author suite stays green because its fixtures use names whose stable idents equal the physical names, so both orders match. The six other independent tests passed, including card order, candidate order, draft preservation, and the closed selection errors.

Required change: every outcome's `selected_datasets` uses one sort of the admitted dataset strings, the same order `selected_names` already uses. Card order by `(schema, name)` stays as specified. This review did not patch the producer.

## Limits, not findings

Column-level candidates, provider transport, residency approval, token cost, and the description CLI are outside this frozen unit. The catalog schema rejects a samples field. A non-boolean `admitted` fails serde as schema-invalid before the admission check. Those are recorded limits.

`linux/proof` did not compile the first overlay and did not install Clippy. That directory is a harness failure, not a product defect.
