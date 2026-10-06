# Handwritten comparison fixtures

Each `*.json` input is a document from `CONTRACT.md`. A `*.report.json` file is the exact stdout of a mismatch, including the final newline. Inputs that match share `report-match.json`.

`match-exact-decimal.json` rescales `1.20` onto `1.200` and `-1.50` onto `-1.5`, and it carries both int64 extremes. `mismatch-duplicate.json` is an unordered multiset: two copies against one copy. `mismatch-ordered.json` keeps that swap in order, so a tie is not inferred. `match-unordered.json` is the same swap with the unordered comparator. `mismatch-partial.json` refuses to treat a partial actual as complete.
