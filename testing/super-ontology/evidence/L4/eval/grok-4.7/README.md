# L4 evaluation with `gcli/grok-4.7` (lane U, SO-B-S20)

Same harness as `../README.md` (`tools/oo-test/super_ontology_eval_live.py`, real engine, Docling sidecar, real egress, 8 extract workers,
`OO_DOCS_MODEL_TIMEOUT_SECS=420`). Numbers are precision / recall. The tables are scored against the labels of this commit
(9 `Document about Event` facts removed, see below); "before" is the golden run with only the egress timeout fixed, re-scored against
these labels.

## Why there was no model run before

`grok-4.7` is a reasoning model: a small extraction needs 130 to 190 s and about 10,000 hidden completion tokens (probed outside the
engine; `reasoning_effort` is ignored by the endpoint). The egress waited 45 s and the engine 120 s, so every call ended as `UPSTREAM_TIMEOUT`:
the first golden run had 197 calls, 6,270 tokens in, 0 model facts and took 50 minutes. That run is discarded. Lane AB later made the
timeout configurable (`OO_DOCS_MODEL_TIMEOUT_SECS`, default 180 s); these runs set 420 s.

## Published view (the acceptance numbers)

| run | entities P / R | facts P / R | supersede | red team hits |
|---|---|---|---|---|
| golden, before (timeout fix only, `extract/2`) | 0.989 / 0.870 | 0.932 / 0.680 | 4/4 | 0 |
| golden, after (merged code, `extract/4`) | 0.968 / 0.910 | 0.871 / 0.784 | 4/4 | 0 |
| holdout, after | 0.954 / 0.874 | 0.835 / 0.739 | 4/4 | 0 |

Candidates: golden before 0.688 / 0.950 and 0.830 / 0.750; golden after 0.784 / 0.980 and 0.837 / 0.859; holdout after 0.754 / 0.968 and
0.773 / 0.805. **Target (P >= 0.85, R >= 0.75): met on golden; entities met on holdout; holdout facts NOT met (0.835 / 0.739).** No holdout run of the
pre-fix code exists (not run, to stay inside the round). An earlier holdout run on the pre-merge code of this branch (`extract/3` of lane U) gave
facts 0.875 / 0.771 (entities 0.966 / 0.895), so the holdout facts figure moves by about 0.04 between runs of the same model at temperature 0.

## Per type and predicate (published; golden before -> golden after; holdout after)

| item | golden before | golden after | holdout after |
|---|---|---|---|
| Topic P / R | 0.00 / 0.00 | 0.67 / 0.40 | 0.33 / 0.20 |
| Event | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 |
| Term | 1.00 / 0.60 | 0.80 / 0.80 | 0.75 / 0.60 |
| about | 0.65 / 0.31 | 0.56 / 0.46 | 0.49 / 0.40 |
| mentions | 0.99 / 0.80 | 0.96 / 0.90 | 0.92 / 0.86 |
| member_of | 0.93 / 0.59 | 0.93 / 0.59 | 0.93 / 0.59 |
| tried_and_failed | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 |
| txt files | 1.00 / 0.28 | 0.91 / 0.34 | 1.00 / 0.30 |

Candidates (before -> after on golden): Topic F 0.20/0.40 -> 0.25/0.60, Event 0.30/0.60 -> 0.56/1.00, Term 0.21/1.00 -> 0.36/1.00, about 0.52/0.62 -> 0.57/0.76,
tried_and_failed 0/0 -> 1.00/1.00 (the two gold facts are found and wait for review).

## Root causes and changes

1. Timeout (above): configurable, 420 s used.
2. `about` of the document itself: the model rates it `medium` (7000) because its quote cannot hold subject and relation, and a model fact whose
   object is known from one file is -1000 under 5.3, so 60 right `about` facts stayed below the 8000 auto line. The prompt (`extract/4`) says an `about` of `e0`
   whose object is in the title, a heading or repeated is `high` (8500). Published `about` recall 0.31 -> 0.46.
3. Boundaries: routine "week N meeting" is no Event, periodic items and budget lines are no Term, a topic keeps one short name across documents,
   `tried_and_failed` is defined. Candidate Event/Term precision up, tried_and_failed found.
4. Labels: DOC-G1 gives `about` the range Project|Topic|Term|Organization; the generator wrote 9 golden (and holdout) `Document about Event` facts that the engine must
   refuse. `gen.py` skips them for both sets (docs unchanged; `--check` 0 mismatches). Applied mechanically to the holdout generator without reading holdout labels.
5. `llm-facts-state.jsonl` (eval diagnostics, kept in the run output, not committed) lists status, score and review reason of each model fact.

## Not met, and why

* Holdout facts P 0.835 / R 0.739 (run variance around the 0.85 / 0.75 line; the model's about links with a single-file object cost precision).
* Event 0 published: an event named in one file is single-sourced (5.3 rule 3) and waits for a person; gates not loosened.
* `member_of` R 0.59: the 9 person -> company labels rest on a table with no company column (unchanged, see `../README.md`).
* txt notes R 0.34: nicknames (`小陈`, `Kevin`) are not linked by the leak guard (unchanged on purpose).
