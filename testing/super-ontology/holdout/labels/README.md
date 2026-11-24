# Golden labels (oo.eval.docs-labels/v1)

Hand-curated labels for the synthetic golden set in `../docs/`. JSON Lines, UTF-8, one JSON object per
line, files `labels/*.jsonl` (the eval tool reads every `*.jsonl` in a directory). A predictions
directory uses exactly the same format, so `--predictions <this dir>` must score 1.0.

## Records

```json
{"kind":"entity","id":"E012","type":"Person","name":"陈子墨","aliases":["小陈"],"anchors":[{"file":"docs/People/陈子墨.md","frag":"#L5-L5"}]}
{"kind":"fact","id":"F007","subject":"E012","predicate":"owns","object":"E031","valid_from":"2026-01-01","valid_to":"2026-05-31","anchors":[...]}
{"kind":"supersede","old":"F007","new":"F008"}
{"kind":"conflict","id":"C1","constraint":"single_owner_per_project","facts":["F010","F011"]}
```

* `entity.type` is one of the D15 core types: `Person Organization Project Decision Document Topic Event Term`.
  A preference is `Decision` with `"subkind":"preference"`. `aliases` lists nicknames, English names, short names.
* `fact.predicate` is one of `owns member_of decides supersedes rejects mentions about tried_and_failed`.
  `subject` and `object` are entity ids of the same file set. `valid_from`/`valid_to` are `YYYY-MM-DD` (optional).
  A fact may carry `"value"` instead of `"object"` for literal values (unused in v1).
* `supersede`: fact `new` replaces fact `old` (the old one is no longer current).
* `conflict` (D57): the listed facts violate `constraint` at the same time; no engine should pick one.
* `anchors` (at least one on every entity and fact; predictions may omit them) point at evidence:
  `{"file": "<path relative to the golden root>", "frag": "<fragment>"}` plus `"bbox"` for pdf.

## Fragments (10 backend sec.3.5)

| format | fragment | meaning |
|---|---|---|
| md, txt | `#L12-L18` | 1-based closed line range of the file (frontmatter counted) |
| csv | `#R12` | 1-based row, header is row 1 |
| xlsx | `#'Sheet'!A1:C9` | sheet name is always single-quoted, A1 range |
| docx | `#¶12-14` | 0-based body paragraph range (table paragraphs not counted); `#T2!R3C1:R3C4` = table 2 (1-based), row 3 |
| pptx | `#s3` | 1-based slide |
| pdf | `#p5` | 1-based page; `"bbox":[x0,y0,x1,y1]` integers in 1/100 pt, origin top-left, page 595x842 pt |

Anchors overlap (provenance coverage) when: text, same file and line ranges intersect; csv/xlsx, same file
(and sheet) and ranges intersect; docx, paragraph ranges intersect or same table row; pptx, same slide;
pdf, same page and bbox IoU >= 0.5 (same page when either side has no bbox).

## Matching (10 backend sec.12.6)

Entities match on (type, normalised name or any normalised alias); normalisation is NFKC, lower-case,
trimmed, inner whitespace collapsed. Facts match on (matched subject, predicate, matched object). Matching is
one-to-one. Predictions are never required to reuse the gold ids.
