# Contract review

Reviewer task, outside the development cap. Scope is `testing/real-db/pg-e2e/CONTRACT.md` only. No automation source was present for this review. No product file was compared as a diff to edit.

## Checked

- M1-I01 and M1-I02 stay `not_accepted`. The counting doc still lists both as open. a274's `ready_for_trusted_query` on generation 48 is not reused as this fixture's success claim.
- The archived `pg-onboarding` commands are `inspect` and `apply-reviewed`, with `--config` and `--run`. The config protocols in the contract match `oo.onboarding.inspect/v1` and `oo.onboarding.apply-reviewed/v1`.
- `source_generation` is an operator cut. The archived checker requires the import result to echo it. 317 is that cut. The row total 100000 is a separate gold count. The contract does not set the cut to 48.
- The import body built by the archived tool omits `budget`. The identity preimage therefore uses `max_rows=10000` and `max_bytes=8388608`. The contract records those defaults and does not tell the driver to pass a larger budget.
- `query-pg` serializes cells as null, bool, int, or text. Putting `numeric(18,4)` only in the physical copy check, plus one recorded probe, matches that executor. The probe is not given an expected exit of 0.
- Embedded exits 5 and 6 match the archived oracle's policy and guard failures. `max_rows` values sit under the apply cap of 2000.
- Source `SELECT` includes `national_id` and `phone` so the snapshot can carry them. Hiding is the role visibility list, which is what produced `classification.absent` for an undecided column in a274.
- The text foreign key is `shop.account.party_key`. The int8 extremes are `shop.entry.qty` at indexes 0 and 1. Nulls are specified for `display_name`, `label`, `amount`, and `memo`.
- Owned paths, the archive commit, the image digests, the 6 GiB and 1 CPU limits, and the private resource names match the task. Cleanup is owner-and-session scoped.

## Boundary kept

The review does not approve the driver, the live chain, or a parent unit. The next code must follow this contract. A failed import or a failed authorized read stays a product seam.

## Result

The contract is frozen for implementation. No change requested.
