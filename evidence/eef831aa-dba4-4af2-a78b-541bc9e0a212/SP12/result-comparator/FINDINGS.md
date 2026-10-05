# Findings

No product defect remains in the attempt 5 tree. The notes below are boundaries of the frozen contract, not follow-up features.

## Carried into the proof

`ExactDecimal::rescale_exact` cannot move a zero coefficient onto another scale, because stripping the only digit would empty the coefficient. The contract therefore treats two non-negative zero coefficients as equal at every pair of scales. That path is `match-zero.json`. It is exact zero, not a tolerance. Negative zero is refused before `ExactDecimal::new`, which would otherwise fold it.

Row equality does not hash the row. Unordered comparison counts canonical JSON strings in a `BTreeMap` and emits `multiplicity`. There is no `set` reason.

`oo_core::parse_json_value` folds the JSON number `-0` to an i64 zero, so a decimal scale written as `-0` is seen as scale 0. The contract forbids fraction and exponent on scale, and it forbids `-0` for int64 text. It does not name scale `-0`. Int64 `-0` is refused and tested. Scale `2.0` and `1e2`-style spellings stay non-integral because the core parser keeps those lexemes, and they are refused.

An empty warning code or an empty column name is `budget`, because the length is outside 1..=max. A pattern failure, a control byte, or `/` in a name is `schema`. Overlong values are `budget`. The contract's budget sentence names values that are too long. The empty case is the minimum length, classified with the other length misses.

A stdout or stderr write error returns token `io` and exit 2. No test installs a failing writer.

The top-level report is a serde struct whose fields are declared in alphabetical order. Nested cells, rows, warnings, and columns are `serde_json` maps, which sort keys. The match report bytes are the contract example plus a newline.

## Out of scope

The tool does not read query-pg JSONL, call a model, compute pass^k or a bootstrap, or score prose. It does not make B-D01 accepted. E1-D and M1-D are unchanged by this delivery.

Attempts 1 through 4 failed in compile or rustfmt and are kept under `linux/impl` through `linux/impl-4`. Their containers were removed. They are not the proof.
