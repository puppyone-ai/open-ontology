# L4 evaluation through the real pipeline, model off and on

`tools/oo-test/super_ontology_eval_live.py` (wrapping `cargo test -p oo-docs --test eval_live`): a real
`oo-docs-engine`, the Docling sidecar container and, for the ON run, the real model egress configured through
`PUT /v1/docs/config/model` with `file:` references only (the owner's endpoint and key files, never read or
printed by the tooling). All 51 golden files (md, txt, csv, xlsx, docx, pptx, pdf) are parsed by their real
parser. The engine's output is written in the labels format (`predictions/`) and scored by `oo-eval docs`
against `golden/labels`.

Two views of the same run:

* `report.*`: every candidate the engine produced that was not rejected (the extraction output, drafts that wait
  for a person included). This is the number comparable with the earlier rules-only evaluation.
* `report-published.*`: only facts in a published version, and the entities they connect plus every Document. This is
  what a pack can contain.

| run | view | entities P / R | facts P / R |
|---|---|---|---|
| model off (rules) | candidates | 0.874 / 0.817 | 0.703 / 0.263 |
| model off (rules) | published | 0.874 / 0.817 | 0.701 / 0.261 |
| model on (`gpt-6.1-sol`) | candidates | 0.594 / 0.882 | 0.467 / 0.308 |
| model on (`gpt-6.1-sol`) | published | 0.769 / 0.860 | 0.568 / 0.333 |

L4 target with the model: P >= 0.85, R >= 0.75. **Not met**, recorded as measured:

* Entities: recall is above target in every run (0.82 off, 0.86 on, published). Precision meets it only with the model
  off (0.874). With the model it falls to 0.77 (published) because the model names Organization, Event and Topic
  entities the labels do not have (Organization 8 FP, Event 2, Topic 1 among the published; 8, 19 and 14 among all
  candidates). The held candidates are what keeps published precision above the all-candidates number.
* Facts: precision with the model is 0.57 and recall 0.33. The gold set has 441 facts, 292 of them `mentions` and
  113 `about` links from a Document to an entity; the rules find 0.29 and 0.08 of them and the model adds to
  `mentions` (0.37) but little to `about` (0.15). `owns` and `member_of` lose precision with the model (0.19 and 0.20
  published, against 0.25 and 0.29 off): the model writes ownership and membership statements the labels do not.
* Supersession: gold 4, predicted 3, scored `correct` 0 in both runs, although the `supersedes` facts themselves match
  (3 of 3 with the model off, `supersedes` row above); the evaluator's link matcher and the exported
  `superseded_by` pairs disagree and this was not investigated here. Conflict (D57): gold 1, predicted 0 (the
  evaluator reads predicted `conflict` records; the engine's `constraint_violation` rows are not exported).
* Red team: 0 canary and 0 red-team-only name hits in every view.

Run facts (model on): 205 calls (this run) against the Drive's daily limit of 2,000 and the per file cap of 20, none
limited (`files_limited` 0), 77,626 tokens in and 47,878 out, about 11 minutes with 4 extract_llm workers. 47 files
`published` and 4 `indexed` (every fact of those files waits for a person in the review queue). The owner's endpoint
drops roughly half of its TLS connections (`SSL: UNEXPECTED_EOF` after 5 s, observed outside the engine with a
two-line probe too), so a call that fails on the wire is retried up to four times with three seconds between tries;
the counters in `run.json` count every call. Two earlier ON runs were discarded: the first produced
no model fact at all because the strict schema rejected `""` in optional fields (fixed, with a test), the second
had an egress that served one request at a time and refused the engine's parallel workers (fixed, with a test).

Causes worth a decision, not hidden by the numbers: the prompt (`extract/1`) lets the model create Topic and Event
entities freely, and the confidence rule of 5.3 holds only a part of them; recall of `about`/`mentions` needs
document-level links the rules do not make. Tuning the prompt and the thresholds is a separate card; nothing here
was tuned on the evaluation set.

`model-off/` and `model-on/` hold `report.md`, `report.json`, `report-published.md`, `report-published.json` and
`run.json`. `rules-only/` is the earlier evaluation of the rule output only (28 files, no sidecar).
