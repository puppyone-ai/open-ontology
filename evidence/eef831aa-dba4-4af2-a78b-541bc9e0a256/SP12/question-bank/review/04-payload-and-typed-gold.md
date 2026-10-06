# Review 4: payload cap and typed gold

Reviewer is the same session that wrote the crate. This is not an independent second-author acceptance.

## Defect fixed before attempt 5

Attempt 3 counted `bank_version`, the dataset id, the suite id, the question id, the role, the role group, and `as_of` in the 16_384-byte payload. `CONTRACT.md` lists question text, gold SQL, statements, annotation text, control-total values, claim strings, typed-gold text, int64 spellings, decimal coefficients, column names, and warning fields, plus one byte per typed cell. Those identity fields are not in that list. The increments were removed. `payload` now starts at 0.

`payload_cap_ignores_identity_bytes` builds one table whose contract bytes are 500 of question text, 384 of claim text, 2_000 of gold SQL, 1 typed column name, 13_472 of cell text, and 27 cell markers, summing to 16_384. That bank is fully approved. One added text byte is `budget` at `/`. Attempt 4 compiled that test: 10 bank tests and 3 CLI tests passed, clippy passed, and rustfmt rejected one `if` layout. The container diff was applied. Attempt 5 is the proof of that source. No further source change followed this pass.

## Typed gold

Row typed gold uses `ExactDecimal::new`. A leading-zero coefficient is `decimal` before that constructor, including a coefficient longer than the canonical digit cap, which is `budget`. Negative zero is `decimal` at `negative`. The stored approved cell stays coefficient `120` scale 2. The test constructs `120` scale 2 and `1200` scale 3 and checks `rescale_exact(3)` equality without rewriting the stored cell. An int64 cell must be the canonical spelling; the spaced `0120` decimal cell, rewritten to an int64 `01`, is `int64`. Ordered false on a scalar, and empty key columns on `top_k`, are `schema`. A clarify `not_found` gold that also carries `ordered` is `missing_gold`. The same document with those extra fields and `unresolved` removed, and the status set to `fully_approved`, passes the gate.

Column names are charged once, on the typed-gold column. The declared value-column name in the boundary fixture is the same name and is not charged again. Warning arrays must be empty, so a non-empty warning is `schema` and contributes no payload bytes. This pass leaves that reading in place.

This review does not accept the question bank and does not generate the 300-question set.
