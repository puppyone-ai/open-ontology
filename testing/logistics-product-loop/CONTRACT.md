# Logistics product loop card

External fixture, gold, and judge for one logistics source. It does not accept the parent plan, production runsc, or an arbitrary database.

`logistics-product-loop card --out <dir>` with no observation exits 4. Every MP check is `pending`. S0–S10 are `notdone`. `parent_plan_acceptance`, `runsc_certified`, and `arbitrary_database` stay false.

`card --observation <file> --out <dir>` reads `oo.logistics-product-loop.observation/v1`. Exit 0 only when provenance is `live` and MP-01a through MP-01d and MP-01 through MP-06 are all `pass`, and no stage claims approval or publication without a separate operator. This session has no human operator, so an `approved` or `published` flag is `fake_approval` and the card fails.

MP-04 compares every cited row with the independent cells from the fixture formulas. HTTP 200, a matching status flag, and the first scalar are not enough. One missed question fails the card.

Question text must not contain SQL or the gold cells. `oracle.json` is not a prompt, a pack input, or a runtime file.

S9 and S10 stay `notdone` for a development preview. `draft_ready` and `generated_unreviewed` are not approval or publication.

Exit codes: 0 pass, 1 a measured check failed, 2 usage, 4 pending.
