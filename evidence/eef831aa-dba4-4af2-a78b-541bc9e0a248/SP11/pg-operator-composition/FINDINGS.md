# Findings

## F1. reviewed-policy refuses the confirmed amount column

`shop.order_doc.amount` is base type `int8`. The fresh decision list classifies it as `quantity`, and `reviewed-effective-oos` confirmed that label (`live/steps/reviewed-effective-oos.stdout`). The catalog row in `live/steps/review-pg-draft.stdout` is `type_name` `int8`, `typtype` `b`, RLS off.

`physical_type` accepts that row. `compatible` then returns false for quantity on `int8`, `int4`, `int2`, `bool`, and `text`. The same function also omits `money`. Exit 11 follows. The token does not name the column. The column is identified by the allow list in `live/reviewed-policy-clerk.request.json` together with those two functions.

This is a compiler refusal of the classification the effective document already stored. The evidence harness does not rewrite `quantity` into another semantic type.

## F2. The policy DTO, install, mapping grant, analyze, and query steps are absent

Because exit 11 happens first, these files were not produced: an install request, a mapping-grant note, an analyze receipt, query stdout, or a local query audit file. Peek was not attempted. Peek's allow list omits `amount`, and that difference was not executed.

## F3. Earlier composition checks matched the reviewed recipe

Import before init exited 1 with `oo-pipeline: target schema is not ready`. Init reported bootstrapped and `full_import` installed, `historical_v2` false. Import state is `awaiting_publish`, source generation 48, target generation 1. Source reader and writer audit counts stayed 0 and both tables stayed at 3 rows. The second role-pack bundle write exited 1 with `io.destination` and did not replace the first bundle. Those results are under `live/steps/`.

## F4. Attempt 1 was a harness type error, preserved

`linux/chain/attempt-1/harness-build.log` is the only compiler error of that run: `operator_request` expected `RoleSpec` and the call passed `&RoleSpec`. Clippy of attempt 2, after the signature change, exited 0 for the harness and for the pipeline, builder, and gateway bins. `lock-registry-added.txt` and `lock-registry-removed.txt` are empty.

## F5. Acceptance flags stay closed

M1-I01 and M1-I02 need the source through a controlled query and that chain consumed by the compiled CLIs. The query CLI did not run. `parent_acceptance`, `development_unit`, and `component_approval` are false. Component reviews were not substituted.
