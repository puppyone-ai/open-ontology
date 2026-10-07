# Findings

## Gap that this repair closes

A validated generation narrows `selection.tables` to one dataset and keeps the full captured catalog in `complete_command.artifact`. `pack` / `pack_pair` reseals that shared artifact with only the current review's approvals. The role compiler then renders one table. A later pack does not pull another table's approved prose out of the curated map. a357 measured that drop. That measurement still holds for `pack_pair`. The a357 withhold is unchanged.

## Repair

`pack-combined` builds one combined command from the explicit per-table reviews. Pins must agree across reviews: current schema, current evidence, wrapper schema, wrapper evidence, curated canonical hash, artifact canonical hash, and prior-review canonical hash. A difference across reviews is `review.mixed_scope`. Agreement with a wrong expected pin is `review.stale`. The same dataset twice is `review.double`. Two classification rows for one column with different canonical JSON are `review.conflict`. A generation whose outcome is not `validated` is `review.blocked` for the whole command. An aggregate file mixed with another file is `review.mixed_scope` before rebind.

Approvals come from the existing collectors. `reseal` is called once when any approval exists. `compile_downstream` runs once for the two roles. The builder document is `oo.pg-builder.pack-combined/v1` and carries the product plus per-table provenance, including rebind. `authenticated` is false. `parent_auth` is `required`. `review.state` is not set. `replace_existing` is not authorized on this command.

## What the proof showed

On the stored pass-1 shop generations, with the stored S9 operator review and S10 classifications, one narrow role pack retained the approved party and account sentences. Deferred entry text and the deferred `national_id` description stayed out of the role files. The restricted role left out the display-name and phone descriptions. Receipt bytes were unchanged. The in-process fixture showed the same shape, and a second call without `column_descriptions` omitted the column bodies.

## What this subset does not decide

This run does not accept E47. It does not authenticate a caller. It does not publish a pack. It does not exercise full SP07. It does not start PostgreSQL. It does not call a model. A new independent review is still required. `parent_acceptance` is false.
